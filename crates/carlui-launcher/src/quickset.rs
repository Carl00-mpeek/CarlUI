//! Bu modül "ayarlar uygulamasını açmadan Wi-Fi'yi kapat, sesi %30 yap,
//! karanlık moda geç" sorununu şu yöntemle çözüyor: aramaya yazılan kısa
//! komutlar ("wifi kapat", "ses 30", "parlaklık 70", "karanlık mod",
//! "sessiz") hızlı ayar sonucu olarak çıkar; Enter'da işlem ayrı bir iş
//! parçacığında `carlui_core::quick` ile yapılır (arayüz beklemez).

use crate::search::{norm, score, Act, Icon, Item};
use carlui_core::quick;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Quick {
    Dark(bool),
    Wifi(bool),
    Bluetooth(bool),
    Mute(bool),
    /// 0..=100
    Volume(u8),
    /// 0..=100
    Brightness(u8),
}

/// (başlık, İngilizce başlık, aranacak ifadeler, simge, komut)
const CMDS: &[(&str, &str, &[&str], char, Quick)] = &[
    ("Karanlık moda geç", "Switch to dark mode", &["karanlik mod", "dark mode", "koyu tema", "gece modu"], '\u{E708}', Quick::Dark(true)),
    ("Açık moda geç", "Switch to light mode", &["acik mod", "light mode", "aydinlik mod", "acik tema"], '\u{E706}', Quick::Dark(false)),
    ("Wi-Fi'yi aç", "Turn on Wi-Fi", &["wifi ac", "wi-fi ac", "kablosuz ac", "wifi on", "turn on wifi"], '\u{E701}', Quick::Wifi(true)),
    ("Wi-Fi'yi kapat", "Turn off Wi-Fi", &["wifi kapat", "wi-fi kapat", "kablosuz kapat", "wifi off", "turn off wifi"], '\u{EB5E}', Quick::Wifi(false)),
    ("Bluetooth'u aç", "Turn on Bluetooth", &["bluetooth ac", "bt ac", "bluetooth on"], '\u{E702}', Quick::Bluetooth(true)),
    ("Bluetooth'u kapat", "Turn off Bluetooth", &["bluetooth kapat", "bt kapat", "bluetooth off"], '\u{E702}', Quick::Bluetooth(false)),
    ("Sesi kapat", "Mute sound", &["sessiz", "sesi kapat", "mute"], '\u{E74F}', Quick::Mute(true)),
    ("Sesi aç", "Unmute sound", &["sesi ac", "unmute"], '\u{E767}', Quick::Mute(false)),
];

fn item(title: String, glyph: char, q: Quick) -> Item {
    let mut it = Item::new(title, t!("Hızlı ayar · Enter ile uygula", "Quick setting · Enter to apply"), Icon::Glyph(glyph), Act::Quick(q), "Ayar");
    it.key = format!("quick:{q:?}");
    it
}

/// "ses 30", "volume %30", "parlaklık 70" → sayılı komut.
fn numeric(q: &str) -> Option<Item> {
    let (word, rest) = q.split_once(' ')?;
    let n: u8 = rest.trim().trim_start_matches('%').trim_end_matches('%').trim().parse().ok()?;
    let n = n.min(100);
    match word {
        "ses" | "volume" | "vol" => Some(item(t!(format!("Sesi %{n} yap"), format!("Set volume to {n}%")), '\u{E767}', Quick::Volume(n))),
        "parlaklik" | "brightness" | "isik" => Some(item(t!(format!("Parlaklığı %{n} yap"), format!("Set brightness to {n}%")), '\u{E706}', Quick::Brightness(n))),
        _ => None,
    }
}

/// Aramaya uyan hızlı ayarlar (puanıyla). `q` normalleştirilmiş olmalı.
pub(crate) fn matches(q: &str) -> Vec<(i32, Item)> {
    if let Some(it) = numeric(q) {
        return vec![(1100, it)];
    }
    if q.chars().count() < 3 {
        return Vec::new();
    }
    CMDS.iter()
        .filter_map(|(title, en, phrases, glyph, cmd)| {
            let s = phrases.iter().map(|p| score(p, q)).max().unwrap_or(0).max(score(&norm(title), q)).max(score(&norm(en), q));
            (s >= 650).then(|| (s, item(t!(title, en).to_string(), *glyph, *cmd)))
        })
        .collect()
}

/// Komutu arka planda uygular.
pub fn run(q: Quick) {
    std::thread::spawn(move || apply(q));
}

/// Sesli komut: metinde bir hızlı ayar ifadesi geçiyor mu ("wifi'yi kapat",
/// "turn on bluetooth", "karanlık moda geç")? İfadenin her kelimesi sırayla
/// bir kelimenin başı olmalı. `words` normalleştirilmiş kelimeler.
pub fn spoken(words: &[&str]) -> Option<(String, Quick)> {
    CMDS.iter().find_map(|(title, en, phrases, _, cmd)| {
        phrases
            .iter()
            .any(|p| {
                let mut it = words.iter();
                p.split(' ').all(|pw| it.any(|w| w.starts_with(&pw.replace('-', ""))))
            })
            .then(|| (t!(title, en).to_string(), *cmd))
    })
}

/// Komutu bu iş parçacığında uygular (bitene dek bekler).
pub fn apply(q: Quick) {
    {
        use windows::Win32::System::{Com::*, WinRT::*};
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
        carlui_core::info!("hızlı ayar: {q:?}");
        match q {
            Quick::Dark(on) => quick::set_dark_mode(on),
            Quick::Wifi(on) => {
                if !quick::set_radio(false, on) {
                    carlui_core::warn!("Wi-Fi aygıtı bulunamadı");
                }
            }
            Quick::Bluetooth(on) => {
                if !quick::set_radio(true, on) {
                    carlui_core::warn!("Bluetooth aygıtı bulunamadı");
                }
            }
            Quick::Mute(on) => {
                if let Some(v) = quick::Volume::new() {
                    v.set_mute(on);
                }
            }
            Quick::Volume(n) => {
                if let Some(v) = quick::Volume::new() {
                    v.set(n as f32 / 100.0);
                }
            }
            Quick::Brightness(n) => {
                if !quick::brightness_set(n as f32 / 100.0) {
                    carlui_core::warn!("monitör parlaklık ayarını (DDC/CI) desteklemiyor");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands() {
        assert_eq!(matches("ses 30")[0].1.act, Act::Quick(Quick::Volume(30)));
        assert_eq!(matches("parlaklik %70")[0].1.act, Act::Quick(Quick::Brightness(70)));
        assert_eq!(matches("volume 250")[0].1.act, Act::Quick(Quick::Volume(100)));
        let wifi = matches("wifi");
        assert!(wifi.iter().any(|(_, i)| i.act == Act::Quick(Quick::Wifi(true))) && wifi.iter().any(|(_, i)| i.act == Act::Quick(Quick::Wifi(false))));
        assert_eq!(matches("wifi kapat").iter().max_by_key(|(s, _)| *s).unwrap().1.act, Act::Quick(Quick::Wifi(false)));
        assert!(matches(&norm("karanlık")).iter().any(|(_, i)| i.act == Act::Quick(Quick::Dark(true))));
        assert!(matches("discord").is_empty());
    }
}
