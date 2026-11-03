//! Bu modül "Steam indiriyor ama hangi oyunu, yüzde kaçta?" sorununu çözüyor:
//! Steam her oyun için kütüphane klasöründe `steamapps\appmanifest_<id>.acf`
//! dosyası tutar ve indirme sürerken içine oyunun adını, inen ve toplam baytı
//! yazar. Bu dosyalar yalnızca okunur (Steam'e dokunulmaz, yönetici izni
//! gerekmez). Kütüphaneler `steamapps\libraryfolders.vdf` içinde listelidir.

use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

/// VDF/ACF satırından `"anahtar"  "değer"` değerini okur (ilk eşleşme).
fn kv<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("\"{key}\"");
    text.lines().find_map(|l| {
        let l = l.trim();
        let rest = l.strip_prefix(&pat)?.trim();
        rest.strip_prefix('"')?.strip_suffix('"')
    })
}

fn steam_dir() -> Option<PathBuf> {
    use windows::{core::w, Win32::System::Registry::*};
    let mut buf = [0u16; 520];
    let mut len = (buf.len() * 2) as u32;
    unsafe { RegGetValueW(HKEY_CURRENT_USER, w!("Software\\Valve\\Steam"), w!("SteamPath"), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as _), Some(&mut len)) }.ok().ok()?;
    let s = String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]);
    let p = PathBuf::from(s.replace('/', "\\"));
    p.exists().then_some(p)
}

/// Bütün kütüphanelerin `steamapps` klasörleri.
fn libraries(steam: &Path) -> Vec<PathBuf> {
    let main = steam.join("steamapps");
    let mut out = vec![main.clone()];
    if let Ok(text) = std::fs::read_to_string(main.join("libraryfolders.vdf")) {
        for l in text.lines() {
            let l = l.trim();
            if let Some(v) = l.strip_prefix("\"path\"").map(str::trim).and_then(|v| v.strip_prefix('"')?.strip_suffix('"')) {
                let p = PathBuf::from(v.replace("\\\\", "\\")).join("steamapps");
                // Steam aynı klasörü farklı harf büyüklüğüyle yazabilir.
                let lower = p.to_string_lossy().to_lowercase();
                if !out.iter().any(|o| o.to_string_lossy().to_lowercase() == lower) && p.exists() {
                    out.push(p);
                }
            }
        }
    }
    out
}

/// Bir manifest indirme sürüyorsa (ad, inen, toplam).
fn parse(text: &str) -> Option<(String, u64, u64)> {
    let total: u64 = kv(text, "BytesToDownload")?.parse().ok()?;
    let got: u64 = kv(text, "BytesDownloaded")?.parse().ok()?;
    if total == 0 || got >= total {
        return None;
    }
    Some((kv(text, "name").unwrap_or("Steam").to_string(), got, total))
}

/// Şu an inen oyun: en son güncellenen, yarım kalmış manifest.
pub fn downloading() -> Option<(String, u64, u64)> {
    let steam = steam_dir()?;
    let mut best: Option<(SystemTime, (String, u64, u64))> = None;
    for lib in libraries(&steam) {
        let Ok(rd) = std::fs::read_dir(&lib) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.starts_with("appmanifest_") || !name.ends_with(".acf") {
                continue;
            }
            let Ok(m) = e.metadata().and_then(|m| m.modified()) else { continue };
            // Bir gündür dokunulmamış yarım indirme şu an sürüyor olamaz.
            if m.elapsed().unwrap_or_default() > Duration::from_secs(24 * 3600) {
                continue;
            }
            if let Some(d) = std::fs::read_to_string(e.path()).ok().and_then(|t| parse(&t)) {
                if best.as_ref().is_none_or(|b| m > b.0) {
                    best = Some((m, d));
                }
            }
        }
    }
    best.map(|b| b.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACF: &str = "\"AppState\"\n{\n\t\"appid\"\t\t\"1091500\"\n\t\"name\"\t\t\"Cyberpunk 2077\"\n\t\"StateFlags\"\t\t\"1026\"\n\t\"BytesToDownload\"\t\t\"68719476736\"\n\t\"BytesDownloaded\"\t\t\"17179869184\"\n}\n";

    #[test]
    fn manifest() {
        assert_eq!(parse(ACF), Some(("Cyberpunk 2077".into(), 17179869184, 68719476736)));
        assert_eq!(parse(&ACF.replace("17179869184", "68719476736")), None);
        assert_eq!(kv(ACF, "appid"), Some("1091500"));
    }

    /// Bu bilgisayarda: `cargo test -p carlui-island -- --ignored steam_live --nocapture`
    #[test]
    #[ignore]
    fn steam_live() {
        let s = steam_dir();
        println!("{s:?}");
        if let Some(s) = s {
            println!("{:?}", libraries(&s));
        }
        println!("{:?}", downloading());
    }
}
