//! Bu modül "dil modelinin yazdığı satırlar hangi işe karşılık gelir?" sorusunu
//! çözüyor. Model yalnızca `open|Discord` gibi satırlar yazar; burada tanınmayan
//! satır atılır, yani izin listesi budur. Kısa komutlar modele hiç gitmez, kurallar
//! anında çözer. Model yoksa ya da yanıt vermezse eski davranış sürer.

use crate::{
    app_sound::{self, Sound},
    index::{App, Running},
    quickset::Quick,
    search::norm,
    voice::{self, best_app, plan, How, Intent, Media, Plan},
};

/// Bir komutta en çok bu kadar eylem.
const MAX_ACTIONS: usize = 3;
/// Bu kadar kelimeye kadar kurallar yeter.
const SHORT: usize = 2;

/// Söylendiğinde bir uygulamayı değil tüm bilgisayarı anlatan sözler.
const WHOLE_PC: &[&str] = &["bilgisayar", "sistem", "genel", "pc", "ses", "windows", "computer", "system"];
/// Şarkı adı değil, "müzik çal" demek.
const NOT_A_SONG: &[&str] = &["sarki", "sarkiyi", "muzik", "muzigi", "parca", "parcayi", "song", "music", "spotify"];

/// Boş ya da yalnızca noktalama ise tüm bilgisayar.
fn target(s: &str) -> Option<String> {
    let n = norm(s);
    (n.chars().any(char::is_alphanumeric) && !WHOLE_PC.contains(&n.as_str())).then_some(n)
}

fn sound(who: &str, what: Sound) -> Intent {
    match (target(who), what) {
        (Some(app), what) => Intent::AppSound { app, what },
        (None, Sound::Level(n)) => Intent::Quick(Quick::Volume(n)),
        (None, Sound::Step(d)) => Intent::Step(d),
        (None, Sound::Mute(on)) => Intent::Quick(Quick::Mute(on)),
    }
}

fn line(l: &str) -> Option<Intent> {
    let p: Vec<&str> = l.split('|').map(str::trim).collect();
    let name = |s: &str| Some(norm(s)).filter(|n| n.chars().any(char::is_alphanumeric));
    Some(match p.as_slice() {
        ["media", "next"] => Intent::Media(Media::Next),
        ["media", "prev"] => Intent::Media(Media::Prev),
        ["media", "toggle"] => Intent::Media(Media::PlayPause),
        ["open", a] => Intent::Open(name(a)?),
        ["close", a] => Intent::Close(name(a)?),
        ["play", q] if NOT_A_SONG.contains(&norm(q).as_str()) => Intent::Media(Media::PlayPause),
        ["play", q] => Intent::Play(q.to_string()).filter_empty()?,
        ["search", site, q] => {
            let site = voice::site_title(&norm(site));
            Intent::Search { site, query: q.to_string() }
        }
        ["volume", who, n] => sound(who, Sound::Level(n.parse::<u8>().ok().filter(|n| *n <= 100)?)),
        ["step", who, d] => sound(who, Sound::Step(d.parse::<i8>().ok()?.signum())),
        ["mute", who, "on"] => sound(who, Sound::Mute(true)),
        ["mute", who, "off"] => sound(who, Sound::Mute(false)),
        ["split", apps, screen] => {
            let apps: Vec<String> = apps.split(',').filter_map(name).collect();
            if !(2..=4).contains(&apps.len()) {
                return None;
            }
            Intent::Split { apps, screen: screen.parse().ok()? }
        }
        ["move", app, screen] => Intent::Move { app: name(app)?, screen: screen.parse().ok()? },
        _ => return None,
    })
}

impl Intent {
    fn filter_empty(self) -> Option<Self> {
        match &self {
            Intent::Play(q) if q.trim().is_empty() => None,
            _ => Some(self),
        }
    }
}

/// Modelin yanıtı; "none" ve tanınmayan satırlar boş liste verir.
pub fn read_lines(out: &str) -> Vec<Intent> {
    out.lines().filter_map(line).take(MAX_ACTIONS).collect()
}

/// Bu cümleyi kurallar tek başına çözer mi? Modelde karşılığı olmayan ayarlar
/// (Wi-Fi, parlaklık…) ona gitmez; yoksa "Wi-Fi'yi kapat" bir uygulama sanılır.
fn rules_enough(text: &str, rule: &Intent) -> bool {
    use Quick::{Bluetooth, Brightness, Dark, Wifi};
    matches!(rule, Intent::Lock | Intent::Quick(Bluetooth(_) | Brightness(_) | Dark(_) | Wifi(_))) || text.split_whitespace().count() <= SHORT
}

/// Cümleyi eylemlere çevirir. `ask` modele sorar; hazır değilse `None` döner.
pub fn understand(text: &str, ask: impl FnOnce(&str) -> Option<String>) -> Vec<Intent> {
    let rule = voice::parse(text);
    if rules_enough(text, &rule) {
        return vec![rule];
    }
    let Some(out) = ask(text) else { return vec![rule] };
    let said = read_lines(&out);
    match (said.is_empty(), rule) {
        (false, _) => said,
        // Model bunu komut saymadı: sohbet web'de aranmaz.
        (true, Intent::Unknown(_) | Intent::Guess(_)) => vec![Intent::Nothing],
        (true, rule) => vec![rule],
    }
}

/// "Steam'i aç ve ikinci ekrana al": yerleştirme uygulamayı zaten açar.
fn placed_later(said: &str, later: &[Intent], apps: &[App]) -> bool {
    let Some((_, app)) = best_app(said, apps) else { return false };
    let same = |s: &String| best_app(s, apps).is_some_and(|(_, a)| a.name == app.name);
    later.iter().any(|i| match i {
        Intent::Move { app, .. } => same(app),
        Intent::Split { apps, .. } => apps.iter().any(same),
        _ => false,
    })
}

/// Eylem listesini tek işe çevirir. Yapılamayan eylem öbürlerini durdurmaz.
pub fn plan_all(list: &[Intent], apps: &[App], run: &[Running], web: &str) -> Plan {
    let needed = |(i, it): &(usize, &Intent)| !matches!(it, Intent::Open(s) if placed_later(s, &list[i + 1..], apps));
    let mut plans: Vec<Plan> = list.iter().enumerate().filter(needed).map(|(_, it)| plan(it, apps, run, web)).collect();
    match plans.len() {
        0 => plan(&Intent::Nothing, apps, run, web),
        1 => plans.remove(0),
        _ => {
            let title = plans.iter().map(|p| p.title.as_str()).collect::<Vec<_>>().join(" · ");
            let hows: Vec<How> = plans.into_iter().filter(Plan::ok).map(|p| p.how).collect();
            Plan { title, how: if hows.is_empty() { How::None } else { How::Seq(hows) } }
        }
    }
}

/// Tek başına ses komutunda uygulama ses çıkarmıyorsa bunu söyler: Windows sesi
/// olmayan uygulamanın düzeyini tutmaz. COM bu iş parçacığında başlatılmış olmalı.
pub fn check(p: Plan) -> Plan {
    match &p.how {
        How::AppSound { app, .. } if !app_sound::has_sound(app) => Plan { title: t!(format!("{} şu an ses çıkarmıyor", app.name), format!("{} isn't playing any sound", app.name)), how: How::None },
        _ => p,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str) -> App {
        App { name: name.into(), parsing: format!("{name}.exe"), target: format!("C:\\{name}.exe"), aumid: String::new() }
    }

    fn one(s: &str) -> Intent {
        let v = read_lines(s);
        assert_eq!(v.len(), 1, "{s}");
        v[0].clone()
    }

    #[test]
    fn reads_allowed_lines() {
        assert_eq!(one("open|Hesap Makinesi"), Intent::Open("hesap makinesi".into()));
        assert_eq!(one("close|Discord"), Intent::Close("discord".into()));
        assert_eq!(one("play|Tarkan Şımarık"), Intent::Play("Tarkan Şımarık".into()));
        assert_eq!(one("play|Şarkı"), Intent::Media(Media::PlayPause));
        assert_eq!(one("media|next"), Intent::Media(Media::Next));
        assert_eq!(one("search|web|hava durumu"), Intent::Search { site: None, query: "hava durumu".into() });
        assert_eq!(one("search|YouTube|lofi"), Intent::Search { site: Some("YouTube".into()), query: "lofi".into() });
        assert_eq!(one("volume|Discord|30"), Intent::AppSound { app: "discord".into(), what: Sound::Level(30) });
        assert_eq!(one("volume||80"), Intent::Quick(Quick::Volume(80)));
        assert_eq!(one("volume|.|80"), Intent::Quick(Quick::Volume(80)));
        assert_eq!(one("volume|bilgisayar|80"), Intent::Quick(Quick::Volume(80)));
        assert_eq!(one("step||-10"), Intent::Step(-1));
        assert_eq!(one("step|Chrome|10"), Intent::AppSound { app: "chrome".into(), what: Sound::Step(1) });
        assert_eq!(one("mute||on"), Intent::Quick(Quick::Mute(true)));
        assert_eq!(one("mute|Steam|off"), Intent::AppSound { app: "steam".into(), what: Sound::Mute(false) });
        assert_eq!(one("split|Discord,Steam|1"), Intent::Split { apps: vec!["discord".into(), "steam".into()], screen: 1 });
        assert_eq!(one("move|Chrome|2"), Intent::Move { app: "chrome".into(), screen: 2 });
    }

    #[test]
    fn drops_anything_else() {
        for bad in ["none", "", "shutdown|now", "run|cmd.exe", "open|", "open|...", "volume||101", "volume||x", "split|Discord|1", "split|a,b,c,d,e|1", "split|a,b|x", "mute||belki", "media|stop"] {
            assert!(read_lines(bad).is_empty(), "{bad}");
        }
        assert_eq!(read_lines("open|a\nopen|b\nopen|c\nopen|d").len(), MAX_ACTIONS);
    }

    #[test]
    fn asks_the_model_only_when_needed() {
        let never = |_: &str| -> Option<String> { panic!("modele gitmemeliydi") };
        assert_eq!(understand("steami aç", never), vec![Intent::Open("steami".into())]);
        assert_eq!(understand("bilgisayarı hemen şimdi kilitle lütfen", never), vec![Intent::Lock]);
        assert_eq!(understand("wi-fi'yi hemen şimdi kapat lütfen", never), vec![Intent::Quick(Quick::Wifi(false))]);
        assert_eq!(understand("parlaklığı yüzde elli yap", never), vec![Intent::Quick(Quick::Brightness(50))]);
        let said = understand("discordun sesini yüzde otuz yap", |_| Some("volume|Discord|30".into()));
        assert_eq!(said, vec![Intent::AppSound { app: "discord".into(), what: Sound::Level(30) }]);
    }

    #[test]
    fn falls_back_to_rules() {
        let open = Intent::Open("not defterini".into());
        assert_eq!(understand("şimdi not defterini aç lütfen hemen", |_| None), vec![open.clone()]);
        assert_eq!(understand("şimdi not defterini aç lütfen hemen", |_| Some("none".into())), vec![open]);
        assert_eq!(understand("bugün çok yorgunum ben", |_| Some("none".into())), vec![Intent::Nothing]);
    }

    #[test]
    fn plans_several_actions() {
        let apps = vec![app("Steam"), app("Discord")];
        let split = Intent::Split { apps: vec!["discord".into(), "steam".into()], screen: 1 };
        let p = plan_all(&[Intent::Open("steam".into()), split], &apps, &[], "");
        assert!(matches!(p.how, How::Split { ref apps, screen: 1 } if apps.len() == 2 && apps[0].name == "Discord"), "{p:?}");

        let p = plan_all(&[Intent::Open("steam".into()), Intent::Step(-1)], &apps, &[], "");
        assert!(matches!(&p.how, How::Seq(v) if v.len() == 2), "{p:?}");
        // Bulunamayan eylem kalanını durdurmaz.
        let p = plan_all(&[Intent::Open("zıpzıp".into()), Intent::Step(1)], &apps, &[], "");
        assert_eq!(p.how, How::Seq(vec![How::Step(1)]));
        assert!(!plan_all(&[], &apps, &[], "").ok());
    }

    #[test]
    fn split_needs_real_apps() {
        let apps = vec![app("Steam")];
        assert!(!plan(&Intent::Split { apps: vec!["steam".into(), "zıpzıp".into()], screen: 1 }, &apps, &[], "").ok());
        assert!(!plan(&Intent::Split { apps: vec!["steam".into(), "stem".into()], screen: 1 }, &apps, &[], "").ok(), "aynı uygulama iki kez");
        assert!(!plan(&Intent::Split { apps: vec!["steam".into(), "steam".into()], screen: 6 }, &apps, &[], "").ok());
    }

    #[test]
    fn spotify_goes_to_the_app_or_the_web() {
        let with = vec![app("Spotify")];
        let q = Intent::Search { site: Some("Spotify".into()), query: "duman".into() };
        assert!(matches!(plan(&q, &with, &[], "").how, How::Play { app: Some(_), .. }));
        assert!(matches!(plan(&q, &[], &[], "").how, How::Play { app: None, .. }));
    }
}
