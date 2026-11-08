//! Bu modül "sesli asistana söylenen cümle ('YouTube'u aç', 'open spotify',
//! 'YouTube'da lofi ara', 'sesi 30 yap', 'sonraki şarkı') hangi işe karşılık
//! gelir?" sorununu çözüyor. Whisper'ın yazdığı metin kelimelere ayrılır,
//! nezaket sözleri atılır, fiil (Türkçede sonda, İngilizcede başta) bulunur;
//! hedef kurulu uygulamalarla ve bilinen sitelerle `fuzzy` benzerliğine göre
//! eşleştirilir. Böylece "yutub", "spotifay", "krom" da doğru yere gider.
//! Eylemler aramanın kendi eylemleridir (`actions::run`, `quickset`).

use crate::{
    app_sound::{self, Sound},
    arrange, fuzzy,
    index::{self, App, Running},
    quickset::{self, Quick},
    search::{self, enc, norm, Act, Sys},
    spotify,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Media {
    PlayPause,
    Next,
    Prev,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Intent {
    Open(String),
    Close(String),
    /// `site` boşsa web araması.
    Search { site: Option<String>, query: String },
    Quick(Quick),
    /// Sesi bir kademe aç (+1) / kıs (-1).
    Step(i8),
    Media(Media),
    Lock,
    /// Spotify'da şarkı, sanatçı ya da tür.
    Play(String),
    /// Yalnızca bir uygulamanın sesi.
    AppSound { app: String, what: Sound },
    /// Uygulamaları ekranda yan yana diz; 0 ekran söylenmedi demektir.
    Split { apps: Vec<String>, screen: u8 },
    Move { app: String, screen: u8 },
    /// Döviz kuru sorusu: "dolar kaç TL" (kodlar: USD, TRY…).
    Rate { amount: f64, from: &'static str, to: &'static str },
    /// Dil modeli bunu komut saymadı.
    Nothing,
    /// Fiil yok, kısa: büyük olasılıkla bir ad ("spotify").
    Guess(String),
    /// Anlaşılamadı; uzunsa web'de aranır.
    Unknown(String),
}

/// Bilinen siteler. `search`: `{q}` aranacak metin, `{lang}` arayüz dili; boşsa arama yok.
struct Site {
    names: &'static [&'static str],
    title: &'static str,
    url: &'static str,
    search: &'static str,
}

const SITES: &[Site] = &[
    Site { names: &["youtube", "yt"], title: "YouTube", url: "https://www.youtube.com", search: "https://www.youtube.com/results?search_query={q}" },
    Site { names: &["google"], title: "Google", url: "https://www.google.com", search: "https://www.google.com/search?q={q}" },
    Site { names: &["gmail", "e posta", "mail"], title: "Gmail", url: "https://mail.google.com", search: "https://mail.google.com/mail/u/0/#search/{q}" },
    Site { names: &["github"], title: "GitHub", url: "https://github.com", search: "https://github.com/search?q={q}" },
    Site { names: &["chatgpt", "chat gpt"], title: "ChatGPT", url: "https://chatgpt.com", search: "https://chatgpt.com/?q={q}" },
    Site { names: &["claude"], title: "Claude", url: "https://claude.ai/new", search: "https://claude.ai/new?q={q}" },
    Site { names: &["perplexity"], title: "Perplexity", url: "https://www.perplexity.ai", search: "https://www.perplexity.ai/search?q={q}" },
    Site { names: &["grok"], title: "Grok", url: "https://grok.com", search: "https://grok.com/?q={q}" },
    Site { names: &["gemini"], title: "Gemini", url: "https://gemini.google.com/app", search: "" },
    Site { names: &["whatsapp"], title: "WhatsApp", url: "https://web.whatsapp.com", search: "" },
    Site { names: &["instagram"], title: "Instagram", url: "https://www.instagram.com", search: "" },
    Site { names: &["twitter"], title: "X (Twitter)", url: "https://x.com", search: "https://x.com/search?q={q}" },
    Site { names: &["facebook"], title: "Facebook", url: "https://www.facebook.com", search: "" },
    Site { names: &["netflix"], title: "Netflix", url: "https://www.netflix.com", search: "https://www.netflix.com/search?q={q}" },
    Site { names: &["reddit"], title: "Reddit", url: "https://www.reddit.com", search: "https://www.reddit.com/search/?q={q}" },
    Site { names: &["wikipedia", "vikipedi"], title: "Wikipedia", url: "https://www.wikipedia.org", search: "https://{lang}.wikipedia.org/w/index.php?search={q}" },
    Site { names: &["linkedin"], title: "LinkedIn", url: "https://www.linkedin.com", search: "" },
    Site { names: &["twitch"], title: "Twitch", url: "https://www.twitch.tv", search: "https://www.twitch.tv/search?term={q}" },
    Site { names: &["spotify"], title: "Spotify", url: "https://open.spotify.com", search: "https://open.spotify.com/search/{q}" },
    Site { names: &["harita", "haritalar", "maps", "google maps"], title: "Google Maps", url: "https://www.google.com/maps", search: "https://www.google.com/maps/search/{q}" },
    Site { names: &["ceviri", "translate", "google ceviri", "google translate"], title: "Google Çeviri", url: "https://translate.google.com", search: "https://translate.google.com/?text={q}" },
];

/// Komuta anlam katmayan sözler.
const FILLER: &[&str] = &[
    "lutfen", "please", "hey", "carl", "karl", "bana", "hemen", "simdi", "sunu", "su", "bir", "misin", "musun", "misiniz", "musunuz", "mi", "mu", "tamam", "ok", "okay", "can", "could",
    "would", "will", "you", "the", "a", "an", "for", "me", "my", "just", "now", "up", "benim", "icin",
];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Verb {
    Open,
    Close,
    Search,
    Play,
}

/// İngilizce fiiller (cümle başında). "opin": Türkçe yazıya dökülmüş "open".
const EN_VERBS: &[(&str, Verb)] = &[
    ("open", Verb::Open), ("opin", Verb::Open), ("opun", Verb::Open), ("launch", Verb::Open), ("start", Verb::Open), ("run", Verb::Open), ("go", Verb::Open),
    ("show", Verb::Open), ("close", Verb::Close), ("quit", Verb::Close), ("exit", Verb::Close), ("kill", Verb::Close), ("search", Verb::Search),
    ("find", Verb::Search), ("google", Verb::Search), ("look", Verb::Search), ("play", Verb::Play),
];

/// Türkçe fiil kökleri (cümle sonunda; "aç", "açar", "açsana", "açın" …).
const TR_VERBS: &[(&str, Verb)] = &[
    ("calistir", Verb::Open), ("baslat", Verb::Open), ("ac", Verb::Open), ("git", Verb::Open), ("gir", Verb::Open), ("goster", Verb::Open),
    ("kapat", Verb::Close), ("sonlandir", Verb::Close), ("arat", Verb::Search), ("ara", Verb::Search), ("bul", Verb::Search), ("oynat", Verb::Play),
    ("cal", Verb::Play), ("dinlet", Verb::Play),
];

fn tr_verb(w: &str) -> Option<Verb> {
    TR_VERBS.iter().find(|(stem, _)| w.starts_with(stem) && w.len() - stem.len() <= 6).map(|(_, v)| *v)
}

fn en_verb(w: &str) -> Option<Verb> {
    EN_VERBS.iter().find(|(v, _)| *v == w).map(|(_, v)| *v)
}

/// Bir kelime: özgün yazım, kesmeden önceki kök ve kesmeden sonraki ek.
#[derive(Clone, Debug)]
struct Tok {
    orig: String,
    base: String,
    suf: String,
}

fn tokens(text: &str) -> Vec<Tok> {
    text.split_whitespace()
        .filter_map(|w| {
            let w = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '’').replace('’', "'");
            let w = w.trim_matches('\'');
            let n = norm(w);
            let (b, s) = n.split_once('\'').unwrap_or((&n, ""));
            let base = fuzzy::compact(b);
            (!base.is_empty()).then(|| Tok { orig: w.split('\'').next().unwrap_or(w).to_string(), base, suf: fuzzy::compact(s) })
        })
        .filter(|t| !FILLER.contains(&t.base.as_str()))
        .collect()
}

/// "%30", "30", "otuz", "thirty" → 30.
fn number(w: &str) -> Option<u8> {
    if let Ok(n) = w.parse::<u32>() {
        return Some(n.min(100) as u8);
    }
    const WORDS: &[(&[&str], u8)] = &[
        (&["sifir", "zero"], 0), (&["on", "ten"], 10), (&["yirmi", "twenty"], 20), (&["otuz", "thirty"], 30), (&["kirk", "forty"], 40), (&["elli", "fifty"], 50),
        (&["altmis", "sixty"], 60), (&["yetmis", "seventy"], 70), (&["seksen", "eighty"], 80), (&["doksan", "ninety"], 90), (&["yuz", "hundred"], 100),
    ];
    WORDS.iter().find(|(ws, _)| ws.contains(&w)).map(|(_, n)| *n)
}

const LOCATIVE: &[&str] = &["da", "de", "ta", "te", "dan", "den", "tan", "ten", "ndan", "nden"];

/// Bilinen sitelerden söylenen ada en çok benzeyen (eşik üstüyse).
fn site_of(said: &str) -> Option<(f32, &'static Site)> {
    SITES
        .iter()
        .map(|s| (s.names.iter().map(|n| fuzzy::sim(said, n)).fold(0.0, f32::max), s))
        .filter(|(sc, _)| *sc >= fuzzy::threshold(said))
        .max_by(|a, b| a.0.total_cmp(&b.0))
}

/// Modelin yazdığı site adı; "web" ya da bilinmeyen ad genel aramadır.
pub(crate) fn site_title(said: &str) -> Option<String> {
    site_of(said).map(|(_, s)| s.title.to_string())
}

/// "youtube'da", "youtubeda" → YouTube.
fn site_with_locative(t: &Tok) -> Option<&'static Site> {
    if LOCATIVE.contains(&t.suf.as_str()) {
        return site_of(&t.base).map(|(_, s)| s);
    }
    // "claude" adın kendisi: sonundaki "de" ek değil.
    if site_of(&t.base).is_some_and(|(sc, _)| sc >= 0.95) {
        return None;
    }
    LOCATIVE.iter().filter_map(|l| t.base.strip_suffix(l)).filter(|b| b.len() >= 3).find_map(|b| site_of(b).filter(|(sc, _)| *sc >= 0.85).map(|(_, s)| s))
}

/// Cümledeki siteyi çıkarır: "… on youtube", "youtube'da …".
fn pull_site(r: &mut Vec<Tok>) -> Option<String> {
    if r.len() >= 2 && matches!(r[r.len() - 2].base.as_str(), "on" | "in" | "at") {
        if let Some((_, s)) = site_of(&r[r.len() - 1].base) {
            r.truncate(r.len() - 2);
            return Some(s.title.to_string());
        }
    }
    let i = r.iter().position(|t| site_with_locative(t).is_some())?;
    let site = site_with_locative(&r[i]).map(|s| s.title.to_string());
    r.remove(i);
    site
}

/// "YouTube'u aç ve X'i arat", "open youtube and search X": önce site, sonra aranan.
fn site_then_query(toks: &[Tok]) -> Option<(String, String)> {
    let (site, mut rest) = match toks {
        [s, v, rest @ ..] if tr_verb(&v.base) == Some(Verb::Open) => (s, rest),
        // "open google chrome" tek bir ad: arama için bağlaç ya da fiil gerekir.
        [v, s, w, rest @ ..] if en_verb(&v.base) == Some(Verb::Open) && (matches!(w.base.as_str(), "and" | "then") || matches!(en_verb(&w.base), Some(Verb::Search | Verb::Play))) => (s, rest),
        _ => return None,
    };
    let (_, site) = site_of(&site.base)?;
    while let [w, tail @ ..] = rest {
        if !matches!(w.base.as_str(), "ve" | "and" | "sonra" | "then") && !matches!(en_verb(&w.base), Some(Verb::Search | Verb::Play)) {
            break;
        }
        rest = tail;
    }
    if let [head @ .., last] = rest {
        if tr_verb(&last.base).is_some() {
            rest = head;
        }
    }
    let query = rest.iter().map(|t| t.orig.as_str()).collect::<Vec<_>>().join(" ");
    (!query.is_empty()).then(|| (site.title.to_string(), query))
}

/// Konuşmayı niyete çevirir.
pub fn parse(text: &str) -> Intent {
    let toks = tokens(text);
    let words: Vec<&str> = toks.iter().map(|t| t.base.as_str()).collect();
    let all = toks.iter().map(|t| t.orig.as_str()).collect::<Vec<_>>().join(" ");
    if toks.is_empty() {
        return Intent::Unknown(String::new());
    }
    let has = |ws: &[&str]| words.iter().any(|w| ws.iter().any(|x| w.starts_with(x)));
    // Kilit: kapatma/yeniden başlatma bilerek yok (yanlış duyulursa iş kaybettirir).
    if has(&["kilitle", "lock"]) {
        return Intent::Lock;
    }
    let origs: Vec<&str> = toks.iter().map(|t| t.orig.as_str()).collect();
    if let Some((amount, from, to)) = crate::voice_rate::parse(&words, &origs) {
        return Intent::Rate { amount, from, to };
    }
    // Ses düzeyi.
    let vol = words.iter().any(|w| (w.starts_with("ses") && !w.starts_with("sessiz")) || matches!(*w, "volume" | "sound"));
    if vol {
        if let Some(n) = words.iter().find_map(|w| number(w)) {
            return Intent::Quick(Quick::Volume(n));
        }
        if has(&["artir", "arttir", "yukselt", "louder", "increase", "raise"]) || words.contains(&"up") {
            return Intent::Step(1);
        }
        if has(&["azalt", "kis", "dusur", "alcalt", "quieter", "decrease", "lower", "down"]) {
            return Intent::Step(-1);
        }
        if has(&["kapat", "mute"]) || words.contains(&"off") {
            return Intent::Quick(Quick::Mute(true));
        }
        if has(&["ac", "unmute"]) || words.contains(&"on") {
            return Intent::Quick(Quick::Mute(false));
        }
    }
    // Parlaklık: "parlaklığı yüzde elli yap", "brightness 70".
    if has(&["parlak", "brightness"]) {
        if let Some(n) = words.iter().find_map(|w| number(w)) {
            return Intent::Quick(Quick::Brightness(n));
        }
    }
    if has(&["louder"]) {
        return Intent::Step(1);
    }
    if has(&["quieter"]) {
        return Intent::Step(-1);
    }
    if has(&["sessiz", "unmute", "mute"]) {
        return Intent::Quick(Quick::Mute(!has(&["unmute"])));
    }
    if let Some((_, q)) = quickset::spoken(&words) {
        return Intent::Quick(q);
    }
    // Medya: kısa ve hedefsiz cümleler.
    if words.len() <= 3 {
        if has(&["sonraki", "siradaki", "next", "skip"]) {
            return Intent::Media(Media::Next);
        }
        if has(&["onceki", "previous"]) {
            return Intent::Media(Media::Prev);
        }
        if has(&["durdur", "duraklat", "pause", "devam", "resume"]) || (words.len() == 1 && matches!(words[0], "play" | "oynat" | "stop")) {
            return Intent::Media(Media::PlayPause);
        }
    }
    if let Some((site, query)) = site_then_query(&toks) {
        return Intent::Search { site: Some(site), query };
    }
    // Fiil: İngilizcede başta, Türkçede sonda (bazen başta).
    // "google" fiil de olur ad da: "Google Chrome'u aç", "Google'da kedi ara" fiil değil.
    let google_is_name = words[0] == "google" && words.len() > 1 && (words[1] == "chrome" || tr_verb(words[words.len() - 1]).is_some());
    let (verb, rest): (Option<Verb>, Vec<Tok>) = if let Some(v) = en_verb(words[0]).filter(|_| !google_is_name) {
        let mut r = toks[1..].to_vec();
        // "go to", "look up", "search for"
        if r.first().is_some_and(|t| matches!(t.base.as_str(), "to" | "up")) {
            r.remove(0);
        }
        (Some(v), r)
    } else if let Some(v) = tr_verb(words[words.len() - 1]).filter(|_| words.len() > 1) {
        (Some(v), toks[..toks.len() - 1].to_vec())
    } else if let Some(v) = tr_verb(words[0]).filter(|_| words.len() > 1) {
        (Some(v), toks[1..].to_vec())
    } else {
        (None, toks.clone())
    };
    let target = |r: &[Tok]| r.iter().map(|t| t.base.as_str()).collect::<Vec<_>>().join(" ");
    match verb {
        Some(Verb::Search) | Some(Verb::Play) => {
            let mut r = rest;
            let mut site = pull_site(&mut r);
            if words[0] == "google" && site.is_none() {
                site = Some("Google".into());
                if r.first().is_some_and(|t| t.base == "google") {
                    r.remove(0);
                }
            }
            if verb == Some(Verb::Play) && site.is_none() {
                site = Some("YouTube".into());
            }
            let query = r.iter().map(|t| t.orig.as_str()).collect::<Vec<_>>().join(" ");
            if query.is_empty() {
                return match site {
                    Some(s) => Intent::Open(norm(&s)),
                    None if verb == Some(Verb::Play) => Intent::Media(Media::PlayPause),
                    None => Intent::Unknown(all),
                };
            }
            Intent::Search { site, query }
        }
        Some(Verb::Open) if !rest.is_empty() => {
            // "youtube'da X'i aç": sitede aranır.
            let mut r = rest.clone();
            match pull_site(&mut r) {
                Some(site) if !r.is_empty() => Intent::Search { site: Some(site), query: r.iter().map(|t| t.orig.as_str()).collect::<Vec<_>>().join(" ") },
                _ => Intent::Open(target(&rest)),
            }
        }
        Some(Verb::Close) if !rest.is_empty() => Intent::Close(target(&rest)),
        Some(_) => Intent::Unknown(all),
        None if words.len() <= 3 => Intent::Guess(target(&rest)),
        None => Intent::Unknown(all),
    }
}

/// Uygulama adlarında Türkçe ekli söyleyişler: "not defterini" → "not defteri".
fn variants(said: &str) -> Vec<String> {
    let c = fuzzy::compact(said);
    let mut v = vec![c.clone()];
    for suf in ["ini", "unu", "ina", "una", "ine", "yi", "yu", "ya", "ye", "ni", "nu", "na", "ne", "i", "u", "a", "e"] {
        if let Some(b) = c.strip_suffix(suf).filter(|b| b.chars().count() >= 3) {
            v.push(b.to_string());
        }
    }
    v
}

fn noisy(name: &str) -> bool {
    let n = norm(name);
    ["uninstall", "kaldir", "readme", "help", "yardim", "documentation", "website", "release notes", "manual"].iter().any(|x| n.contains(x))
}

/// Uygulama adına benzerlik: tam ad, ya da adın tek bir kelimesi ("chrome" → Google Chrome).
fn app_score(vs: &[String], name: &str) -> f32 {
    let n = norm(name);
    let full = vs.iter().map(|v| fuzzy::sim(v, &n)).fold(0.0, f32::max);
    let word = n
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 3)
        .flat_map(|w| vs.iter().map(move |v| fuzzy::sim(v, w)))
        .fold(0.0, f32::max)
        * 0.92;
    full.max(word)
}

pub(crate) fn best_app<'a>(said: &str, apps: &'a [App]) -> Option<(f32, &'a App)> {
    let vs = variants(said);
    let thr = fuzzy::threshold(said);
    apps.iter()
        .filter(|a| !noisy(&a.name))
        .map(|a| (app_score(&vs, &a.name), a))
        .filter(|(s, _)| *s >= thr)
        // Eşit puanda kısa ad: "Spotify" "Spotify Widget"tan önce.
        .max_by(|a, b| a.0.total_cmp(&b.0).then(b.1.name.len().cmp(&a.1.name.len())))
}

fn best_site(said: &str) -> Option<(f32, &'static Site)> {
    variants(said).iter().filter_map(|v| site_of(v)).max_by(|a, b| a.0.total_cmp(&b.0))
}

/// Yapılacak iş.
#[derive(Clone, Debug, PartialEq)]
pub enum How {
    Act(Act),
    Quick(Quick),
    Step(i8),
    Media(Media),
    /// `app` Spotify masaüstü uygulamasıdır; yoksa tarayıcıda aranır.
    Play { query: String, app: Option<App> },
    AppSound { app: App, what: Sound },
    Split { apps: Vec<App>, screen: u8 },
    Move { app: App, screen: u8 },
    /// Kur sorulur (`voice_lines::check` yanıta çevirir).
    Rate { amount: f64, from: &'static str, to: &'static str },
    /// Yalnızca söylenecek yanıt; yapılacak iş yok.
    Say(String),
    /// Sırayla yapılacak işler.
    Seq(Vec<How>),
    /// Hiçbir şey yapılmaz (bulunamadı).
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// Kartta görünen açıklama: "YouTube açılıyor".
    pub title: String,
    pub how: How,
}

impl Plan {
    pub(crate) fn new(title: String, how: How) -> Self {
        Self { title, how }
    }

    pub fn ok(&self) -> bool {
        self.how != How::None
    }
}

fn fill(tpl: &str, q: &str) -> String {
    tpl.replace("{lang}", t!("tr", "en")).replace("{q}", &enc(q))
}

fn web(web_search: &str, q: &str) -> Plan {
    Plan::new(t!(format!("Web'de aranıyor: {q}"), format!("Searching the web: {q}")), How::Act(Act::Open(fill(web_search, q))))
}

/// Açılacak şey: uygulama mı site mi? İkisi de uyuyorsa kurulu uygulama öne geçer
/// (Spotify uygulaması varsa o açılır), ama site adı tam söylendiyse ("google")
/// adının yalnızca bir kelimesi uyan uygulama (Google Chrome) değil site açılır.
fn open_plan(said: &str, apps: &[App], run: &[Running]) -> Option<Plan> {
    let app = best_app(said, apps);
    let site = best_site(said);
    match (app, site) {
        (Some((a, app)), s) if s.is_none_or(|(s, _)| a >= s) => {
            let it = search::app_item(app, run);
            let title = if it.run.is_some() { t!(format!("{} öne getiriliyor", app.name), format!("Bringing {} to front", app.name)) } else { t!(format!("{} açılıyor", app.name), format!("Opening {}", app.name)) };
            Some(Plan::new(title, How::Act(it.act)))
        }
        (_, Some((_, s))) => Some(Plan::new(t!(format!("{} açılıyor", s.title), format!("Opening {}", s.title)), How::Act(Act::Open(s.url.into())))),
        _ => None,
    }
}

/// Niyeti işe çevirir. `apps`/`run` aramanın uygulama dizini ve açık pencereleri,
/// `web_search` ayarlardaki arama adresi (`{q}`).
pub fn plan(i: &Intent, apps: &[App], run: &[Running], web_search: &str) -> Plan {
    let not_found = |s: &str| Plan::new(t!(format!("Bulunamadı: {s}"), format!("Not found: {s}")), How::None);
    match i {
        Intent::Open(s) => open_plan(s, apps, run).unwrap_or_else(|| not_found(s)),
        Intent::Guess(s) => open_plan(s, apps, run).unwrap_or_else(|| if s.contains(' ') { web(web_search, s) } else { not_found(s) }),
        Intent::Close(s) => match best_app(s, apps) {
            Some((_, a)) => match index::window_of(a, run) {
                Some(w) => Plan::new(t!(format!("{} kapatılıyor", a.name), format!("Closing {}", a.name)), How::Act(Act::Close(w.hwnd))),
                None => Plan::new(t!(format!("{} açık değil", a.name), format!("{} isn't open", a.name)), How::None),
            },
            None => not_found(s),
        },
        Intent::Search { site: None, query } => web(web_search, query),
        Intent::Search { site: Some(site), query } if site == "Spotify" => plan(&Intent::Play(query.clone()), apps, run, web_search),
        Intent::Play(q) => {
            let app = best_app("spotify", apps).map(|(_, a)| a.clone());
            let title = if app.is_some() { t!(format!("Spotify'da çalınıyor: {q}"), format!("Playing on Spotify: {q}")) } else { t!(format!("Spotify'da aranıyor: {q}"), format!("Searching Spotify: {q}")) };
            Plan::new(title, How::Play { query: q.clone(), app })
        }
        Intent::AppSound { app, what } => match best_app(app, apps) {
            Some((_, a)) => Plan::new(sound_title(&a.name, *what), How::AppSound { app: a.clone(), what: *what }),
            None => not_found(app),
        },
        Intent::Split { apps: said, screen } => {
            let mut found: Vec<App> = Vec::new();
            for s in said {
                match best_app(s, apps) {
                    Some((_, a)) if !found.contains(a) => found.push(a.clone()),
                    Some(_) => {}
                    None => return not_found(s),
                }
            }
            if found.len() < 2 {
                return not_found(said.join(", ").as_str());
            }
            if !arrange::has_screen(*screen) {
                return no_screen(*screen);
            }
            let names = found.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ");
            let title = match screen {
                0 => t!(format!("Yan yana diziliyor: {names}"), format!("Placing side by side: {names}")),
                n => t!(format!("{n}. ekrana diziliyor: {names}"), format!("Placing on screen {n}: {names}")),
            };
            Plan::new(title, How::Split { apps: found, screen: *screen })
        }
        Intent::Move { app, screen } => match best_app(app, apps) {
            Some(_) if !arrange::has_screen(*screen) => no_screen(*screen),
            Some((_, a)) => Plan::new(t!(format!("{} {screen}. ekrana taşınıyor", a.name), format!("Moving {} to screen {screen}", a.name)), How::Move { app: a.clone(), screen: *screen }),
            None => not_found(app),
        },
        Intent::Nothing => Plan::new(t!("Anlaşılamadı", "Didn't catch that").into(), How::None),
        Intent::Search { site: Some(site), query } => match SITES.iter().find(|s| s.title == site) {
            Some(s) if !s.search.is_empty() => Plan::new(t!(format!("{}'da aranıyor: {query}", s.title), format!("Searching {}: {query}", s.title)), How::Act(Act::Open(fill(s.search, query)))),
            Some(s) => web(web_search, &format!("{} {query}", s.title)),
            None => web(web_search, query),
        },
        Intent::Quick(q) => Plan::new(quick_title(*q), How::Quick(*q)),
        Intent::Step(d) => Plan::new(if *d > 0 { t!("Ses açılıyor", "Volume up") } else { t!("Ses kısılıyor", "Volume down") }.into(), How::Step(*d)),
        Intent::Media(m) => Plan::new(
            match m {
                Media::PlayPause => t!("Oynat / duraklat", "Play / pause"),
                Media::Next => t!("Sonraki parça", "Next track"),
                Media::Prev => t!("Önceki parça", "Previous track"),
            }
            .into(),
            How::Media(*m),
        ),
        Intent::Lock => Plan::new(t!("Bilgisayar kilitleniyor", "Locking the PC").into(), How::Act(Act::System(Sys::Lock))),
        Intent::Rate { amount, from, to } => Plan::new(t!("Kur alınıyor…", "Getting the rate…").into(), How::Rate { amount: *amount, from: *from, to: *to }),
        Intent::Unknown(s) if s.split_whitespace().count() >= 2 => web(web_search, s),
        Intent::Unknown(s) => Plan::new(if s.is_empty() { t!("Anlaşılamadı", "Didn't catch that").into() } else { t!(format!("Anlaşılamadı: {s}"), format!("Didn't understand: {s}")) }, How::None),
    }
}

fn no_screen(n: u8) -> Plan {
    Plan::new(t!(format!("{n}. ekran yok"), format!("There's no screen {n}")), How::None)
}

fn sound_title(app: &str, what: Sound) -> String {
    match what {
        Sound::Level(n) => t!(format!("{app} sesi %{n}"), format!("{app} volume {n}%")),
        Sound::Step(d) if d > 0 => t!(format!("{app} sesi açılıyor"), format!("{app} volume up")),
        Sound::Step(_) => t!(format!("{app} sesi kısılıyor"), format!("{app} volume down")),
        Sound::Mute(true) => t!(format!("{app} sesi kapatıldı"), format!("{app} muted")),
        Sound::Mute(false) => t!(format!("{app} sesi açıldı"), format!("{app} unmuted")),
    }
}

fn quick_title(q: Quick) -> String {
    match q {
        Quick::Volume(n) => t!(format!("Ses %{n}"), format!("Volume {n}%")),
        Quick::Mute(true) => t!("Ses kapatıldı", "Muted").into(),
        Quick::Mute(false) => t!("Ses açıldı", "Unmuted").into(),
        Quick::Brightness(n) => t!(format!("Parlaklık %{n}"), format!("Brightness {n}%")),
        Quick::Dark(true) => t!("Karanlık mod", "Dark mode").into(),
        Quick::Dark(false) => t!("Açık mod", "Light mode").into(),
        Quick::Wifi(on) => if on { t!("Wi-Fi açılıyor", "Turning Wi-Fi on") } else { t!("Wi-Fi kapatılıyor", "Turning Wi-Fi off") }.into(),
        Quick::Bluetooth(on) => if on { t!("Bluetooth açılıyor", "Turning Bluetooth on") } else { t!("Bluetooth kapatılıyor", "Turning Bluetooth off") }.into(),
    }
}

/// İşi bu iş parçacığında yapar (süreç hemen kapanabilir; arka plana atmaz).
pub fn execute(h: &How) {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    match h {
        How::Act(a) => {
            crate::actions::run(a);
        }
        How::Quick(q) => quickset::apply(*q),
        How::Step(d) => {
            if let Some(v) = carlui_core::quick::Volume::new() {
                let (cur, _) = v.get();
                v.set((cur + 0.1 * *d as f32).clamp(0.0, 1.0));
                v.set_mute(false);
            }
        }
        How::Media(m) => {
            let vk = match m {
                Media::PlayPause => VK_MEDIA_PLAY_PAUSE,
                Media::Next => VK_MEDIA_NEXT_TRACK,
                Media::Prev => VK_MEDIA_PREV_TRACK,
            };
            let key = |up: bool| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, ..Default::default() } },
            };
            unsafe {
                SendInput(&[key(false), key(true)], std::mem::size_of::<INPUT>() as i32);
            }
        }
        How::Play { query, app } => spotify::play(query, app.as_ref()),
        How::AppSound { app, what } => app_sound::apply(app, *what),
        How::Split { apps, screen } => arrange::split(apps, *screen),
        How::Move { app, screen } => arrange::move_to(app, *screen),
        How::Seq(list) => list.iter().for_each(execute),
        How::Rate { .. } | How::Say(_) | How::None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str) -> App {
        App { name: name.into(), parsing: format!("{name}.exe"), target: format!("C:\\{name}.exe"), aumid: String::new() }
    }

    #[test]
    fn parses_commands() {
        assert_eq!(parse("YouTube'u aç."), Intent::Open("youtube".into()));
        assert_eq!(parse("Open YouTube"), Intent::Open("youtube".into()));
        assert_eq!(parse("Lütfen Spotify'ı açar mısın?"), Intent::Open("spotify".into()));
        assert_eq!(parse("Chrome'u kapat"), Intent::Close("chrome".into()));
        assert_eq!(parse("not defterini başlat"), Intent::Open("not defterini".into()));
        assert_eq!(parse("YouTube'da kedi videoları ara"), Intent::Search { site: Some("YouTube".into()), query: "kedi videoları".into() });
        assert_eq!(parse("search cats on youtube"), Intent::Search { site: Some("YouTube".into()), query: "cats".into() });
        assert_eq!(parse("play lofi music"), Intent::Search { site: Some("YouTube".into()), query: "lofi music".into() });
        assert_eq!(parse("hava durumu ara"), Intent::Search { site: None, query: "hava durumu".into() });
        let yt = |q: &str| Intent::Search { site: Some("YouTube".into()), query: q.into() };
        assert_eq!(parse("YouTube'da Ebru Gündeş'i aç."), yt("Ebru Gündeş"));
        assert_eq!(parse("youtube'dan lofi müzik aç"), yt("lofi müzik"));
        assert_eq!(parse("YouTube'ı aç ve Ebru Gündeş'i arat."), yt("Ebru Gündeş"));
        assert_eq!(parse("YouTube'u aç Ebru Gündeş"), yt("Ebru Gündeş"));
        assert_eq!(parse("open cats on youtube"), yt("cats"));
        assert_eq!(parse("open youtube and search for cats"), yt("cats"));
        assert_eq!(parse("youtube'da aç"), Intent::Open("youtube".into()));
        assert_eq!(parse("Claude Code'u aç"), Intent::Open("claude code".into()));
        assert_eq!(parse("Google Chrome'u aç"), Intent::Open("google chrome".into()));
        assert_eq!(parse("google chrome aç"), Intent::Open("google chrome".into()));
        assert_eq!(parse("open google chrome"), Intent::Open("google chrome".into()));
        assert_eq!(parse("Google Chrome"), Intent::Guess("google chrome".into()));
        assert_eq!(parse("Google Chrome'u kapat"), Intent::Close("google chrome".into()));
        assert_eq!(parse("Google'ı aç"), Intent::Open("google".into()));
        let g = |q: &str| Intent::Search { site: Some("Google".into()), query: q.into() };
        assert_eq!(parse("Google'da kedi ara"), g("kedi"));
        assert_eq!(parse("google kedi ara"), g("kedi"));
        assert_eq!(parse("google cats"), g("cats"));
        assert_eq!(parse("Sesi 30 yap"), Intent::Quick(Quick::Volume(30)));
        assert_eq!(parse("sesi otuz yap"), Intent::Quick(Quick::Volume(30)));
        assert_eq!(parse("sesi artır"), Intent::Step(1));
        assert_eq!(parse("volume down"), Intent::Step(-1));
        assert_eq!(parse("sesi kapat"), Intent::Quick(Quick::Mute(true)));
        assert_eq!(parse("Sesi aç"), Intent::Quick(Quick::Mute(false)));
        assert_eq!(parse("Wi-Fi'yi kapat"), Intent::Quick(Quick::Wifi(false)));
        assert_eq!(parse("karanlık moda geç"), Intent::Quick(Quick::Dark(true)));
        assert_eq!(parse("sonraki şarkı"), Intent::Media(Media::Next));
        assert_eq!(parse("durdur"), Intent::Media(Media::PlayPause));
        assert_eq!(parse("bilgisayarı kilitle"), Intent::Lock);
        assert_eq!(parse("spotify"), Intent::Guess("spotify".into()));
    }

    #[test]
    fn resolves_targets() {
        let apps = vec![app("Google Chrome"), app("Spotify"), app("Not Defteri"), app("Visual Studio Code"), app("WhatsApp"), app("Uninstall Spotify")];
        let web = "https://www.google.com/search?q={q}";
        let open = |s: &str| plan(&parse(s), &apps, &[], web);
        let launch = |n: &str| How::Act(Act::App { launch: format!("shell:AppsFolder\\{n}.exe"), hwnd: None });
        assert_eq!(open("Spotifay'ı aç").how, launch("Spotify"));
        assert_eq!(open("krom aç").how, launch("Google Chrome"));
        assert_eq!(open("open chrome").how, launch("Google Chrome"));
        assert_eq!(open("Google Chrome'u aç").how, launch("Google Chrome"));
        assert_eq!(open("open google chrome").how, launch("Google Chrome"));
        assert_eq!(open("Google'ı aç").how, How::Act(Act::Open("https://www.google.com".into())));
        assert_eq!(open("not defterini aç").how, launch("Not Defteri"));
        assert_eq!(open("vatsap aç").how, launch("WhatsApp"));
        assert_eq!(open("visual studio aç").how, launch("Visual Studio Code"));
        assert_eq!(open("yutubu aç").how, How::Act(Act::Open("https://www.youtube.com".into())));
        assert_eq!(open("open google").how, How::Act(Act::Open("https://www.google.com".into())));
        assert_eq!(open("YouTube'da lofi ara").how, How::Act(Act::Open("https://www.youtube.com/results?search_query=lofi".into())));
        assert_eq!(open("hava durumu ara").how, How::Act(Act::Open("https://www.google.com/search?q=hava+durumu".into())));
        assert!(!open("zıpzıp aç").ok());
        assert!(!open("chrome'u kapat").ok(), "açık değil");
    }
}
