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

/// Bir manifest indirme sürüyorsa (ad, inen, toplam, uygulama no).
fn parse(text: &str) -> Option<(String, u64, u64, u32)> {
    let total: u64 = kv(text, "BytesToDownload")?.parse().ok()?;
    let got: u64 = kv(text, "BytesDownloaded")?.parse().ok()?;
    if total == 0 || got >= total {
        return None;
    }
    let id = kv(text, "appid").and_then(|v| v.parse().ok()).unwrap_or(0);
    Some((kv(text, "name").unwrap_or("Steam").to_string(), got, total, id))
}

/// "2026-11-03 10:48:17" → gün başından beri saniye (yerel saat; yalnızca farkı anlamlı).
fn stamp(s: &str) -> Option<i64> {
    let (d, t) = s.split_once(' ')?;
    let mut d = d.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let mut t = t.split(':').map(|p| p.parse::<i64>().ok());
    let (hh, mm, ss) = (t.next()??, t.next()??, t.next()??);
    // Gregoryen takvimde gün sayısı (Howard Hinnant'ın formülü).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let days = era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(days * 86400 + hh * 3600 + mm * 60 + ss)
}

fn now_stamp() -> i64 {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    stamp(&format!("{}-{}-{} {}:{}:{}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)).unwrap_or(0)
}

/// Steam inen miktarı manifeste yalnızca başlarken yazar; arası
/// `logs\content_log.txt`'den hesaplanır: son "update started : download X/…"
/// satırındaki X'e, ondan sonraki dakikalık "Current download rate" hızları
/// süreyle çarpılıp eklenir.
fn from_log(log: &str, id: u32, now: i64) -> Option<u64> {
    let start_pat = format!("AppID {id} update started : download ");
    let mut base: Option<(i64, f64)> = None;
    let mut last = (0i64, 0.0f64);
    for l in log.lines() {
        let Some((ts, msg)) = l.strip_prefix('[').and_then(|r| r.split_once("] ")) else { continue };
        let Some(t) = stamp(ts) else { continue };
        if let Some(rest) = msg.strip_prefix(&start_pat) {
            let got: f64 = rest.split('/').next()?.trim().parse().ok()?;
            base = Some((t, got));
            last = (t, 0.0);
        } else if let (Some((_, got)), Some(r)) = (base.as_mut(), msg.strip_prefix("Current download rate: ")) {
            let mbps: f64 = r.trim().trim_end_matches("Mbps").trim().parse().unwrap_or(0.0);
            let bps = mbps * 1e6 / 8.0;
            *got += bps * (t - last.0).max(0) as f64;
            last = (t, bps);
        }
    }
    let (_, got) = base?;
    Some((got + last.1 * (now - last.0).clamp(0, 120) as f64) as u64)
}

/// Şu an inen oyun: en son güncellenen, yarım kalmış manifest.
/// (ad, inen, toplam); inen, manifest ve günlükten hangisi ilerideyse o.
pub fn downloading() -> Option<(String, u64, u64)> {
    let steam = steam_dir()?;
    let (name, got, total, id) = manifest(&steam)?;
    let est = tail(&steam.join("logs").join("content_log.txt"), 1 << 20).and_then(|l| from_log(&l, id, now_stamp())).unwrap_or(0);
    Some((name, got.max(est).min(total.saturating_sub(1)), total))
}

/// Dosyanın son `max` baytı (günlük büyüyebilir; her saniye tamamı okunmasın).
fn tail(p: &Path, max: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(p).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(max))).ok()?;
    let mut b = Vec::new();
    f.read_to_end(&mut b).ok()?;
    Some(String::from_utf8_lossy(&b).into_owned())
}

fn manifest(steam: &Path) -> Option<(String, u64, u64, u32)> {
    let mut best: Option<(SystemTime, (String, u64, u64, u32))> = None;
    for lib in libraries(steam) {
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
        assert_eq!(parse(ACF), Some(("Cyberpunk 2077".into(), 17179869184, 68719476736, 1091500)));
        assert_eq!(parse(&ACF.replace("17179869184", "68719476736")), None);
        assert_eq!(kv(ACF, "appid"), Some("1091500"));
    }

    #[test]
    fn log() {
        let log = "[2026-11-03 10:48:17] AppID 393380 update started : download 1000/66940920656, store 0/0\n\
                   [2026-11-03 10:49:17] Current download rate: 80.000 Mbps\n\
                   [2026-11-03 10:50:17] Current download rate: 160.000 Mbps\n";
        let now = stamp("2026-11-03 10:50:47").unwrap();
        // 1000 + 10 MB/sn × 60 + 20 MB/sn × 60 + 20 MB/sn × 30
        assert_eq!(from_log(log, 393380, now), Some(1000 + 600_000_000 + 1_200_000_000 + 600_000_000));
        assert_eq!(from_log(log, 730, now), None);
        assert_eq!(stamp("2026-03-01 00:00:00").unwrap() - stamp("2026-02-28 00:00:00").unwrap(), 86400);
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
