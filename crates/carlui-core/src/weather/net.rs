//! Open-Meteo adresleri ve indirme. Windows'un kendi curl.exe'si tam yoluyla,
//! sertleştirilmiş bayraklarla ve konsol penceresi açmadan çalışır
//! (carlui-ai/src/net.rs ile aynı yöntem). Yalnızca iki sunucuya istek atılır.
//! Arka plan iş parçacığından çağrılmalı; hiçbiri panik etmez.

use super::{parse, Place, Weather};
use std::{os::windows::process::CommandExt, path::PathBuf, process::Command};

const NO_WINDOW: u32 = 0x0800_0000;
const MAX_BODY: usize = 256 * 1024;
const HOSTS: [&str; 2] = ["api.open-meteo.com", "geocoding-api.open-meteo.com"];

fn system32(name: &str) -> PathBuf {
    let root = std::env::var_os("SystemRoot").filter(|s| !s.is_empty()).unwrap_or_else(|| r"C:\Windows".into());
    PathBuf::from(root).join("System32").join(name)
}

/// Koordinatlar gizlilik için bilerek 2 ondalığa yuvarlanır (~1 km).
pub fn forecast_url(lat: f64, lon: f64) -> String {
    format!("https://api.open-meteo.com/v1/forecast?latitude={lat:.2}&longitude={lon:.2}&current=weather_code,precipitation,snowfall,cloud_cover,is_day&timezone=auto")
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn search_url(name: &str, lang: &str) -> String {
    // Dil kodu yalnızca harf olabilir; başka bir şey varsa "en".
    let lang = if !lang.is_empty() && lang.len() <= 8 && lang.bytes().all(|b| b.is_ascii_alphabetic()) { lang.to_ascii_lowercase() } else { "en".into() };
    format!("https://geocoding-api.open-meteo.com/v1/search?name={}&count=8&language={lang}&format=json", percent_encode(name.trim()))
}

fn get(url: &str) -> Option<String> {
    let host = url.strip_prefix("https://")?.split(['/', '?']).next()?;
    if !HOSTS.contains(&host) {
        return None;
    }
    let out = Command::new(system32("curl.exe"))
        // -q en başta: kullanıcının .curlrc'si okunmaz.
        .args(["-q", "--silent", "--fail", "--location", "--proto", "=https", "--proto-redir", "=https", "--max-redirs", "3", "--max-time", "15", "--url", url])
        .creation_flags(NO_WINDOW)
        .output()
        .ok()?;
    if !out.status.success() || out.stdout.is_empty() || out.stdout.len() > MAX_BODY {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

pub fn fetch(lat: f64, lon: f64) -> Option<Weather> {
    if !lat.is_finite() || !lon.is_finite() || lat.abs() > 90.0 || lon.abs() > 180.0 {
        return None;
    }
    parse::parse_forecast(&get(&forecast_url(lat, lon))?)
}

pub fn search(name: &str, lang: &str) -> Vec<Place> {
    if name.trim().is_empty() {
        return Vec::new();
    }
    get(&search_url(name, lang)).map(|b| parse::parse_places(&b)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forecast_rounding() {
        let u = forecast_url(51.50735, -0.12776);
        assert!(u.contains("latitude=51.51&longitude=-0.13&"), "{u}");
        assert!(u.starts_with("https://api.open-meteo.com/v1/forecast?"));
        assert!(u.ends_with("&timezone=auto"));
    }

    #[test]
    fn search_encoding() {
        let u = search_url("İstanbul", "tr");
        assert!(u.contains("name=%C4%B0stanbul&count=8&language=tr&format=json"), "{u}");
        assert!(search_url("Şanlıurfa", "tr").contains("name=%C5%9Eanl%C4%B1urfa&"));
        assert!(search_url("New York & Co", "en").contains("name=New%20York%20%26%20Co&"));
        assert!(search_url("x", "e&n=1").contains("language=en&"));
    }

    #[test]
    fn hosts_only() {
        assert!(get("https://example.com/").is_none());
        assert!(get("http://api.open-meteo.com/").is_none());
        assert!(get("https://api.open-meteo.com@evil.com/").is_none());
    }
}
