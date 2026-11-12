//! Dock ile ada arasında paylaşılan son hava durumu. Yalnızca dock ağdan
//! okur ve `save` eder; ada ve diğer süreçler yalnızca `effective` ile bu
//! küçük JSON dosyasını okur (`%LOCALAPPDATA%\CarlUI\weather.json`).
//! Yazma atomiktir: önce geçici dosya, sonra yeniden adlandırma.

use super::{debug_override, Weather};
use crate::live_weather::LiveWeather;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// Bundan eski kayıt kullanılmaz (bağlantı uzun süredir yoksa elle seçilen sahne).
pub const MAX_AGE_SECS: u64 = 3 * 3600;

#[derive(Serialize, Deserialize)]
struct File {
    /// Kaydın Unix saniyesi.
    at: u64,
    /// Yer anahtarı (gizlilik için 2 ondalığa yuvarlı "lat,lon"): yer
    /// değişince eski yerin havası kullanılmasın.
    key: String,
    weather: Weather,
}

pub fn path() -> PathBuf {
    crate::paths::data_dir().join("weather.json")
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Ağ isteğiyle aynı yuvarlama (~1 km).
pub fn place_key(lat: f64, lon: f64) -> String {
    format!("{lat:.2},{lon:.2}")
}

fn write_to(path: &Path, at: u64, key: &str, w: &Weather) -> bool {
    let Ok(json) = serde_json::to_string(&File { at, key: key.to_string(), weather: *w }) else { return false };
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).is_ok() && std::fs::rename(&tmp, path).is_ok()
}

/// (hava, yaş saniye, yer anahtarı); dosya yoksa ya da bozuksa `None`.
fn read_from(path: &Path, now: u64) -> Option<(Weather, u64, String)> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > 4096 {
        return None;
    }
    let f: File = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    if !f.weather.intensity.is_finite() {
        return None;
    }
    Some((f.weather, now.saturating_sub(f.at), f.key))
}

/// `fresh` kuralı: yer aynı ve kayıt `MAX_AGE_SECS`'ten genç.
fn pick(saved: Option<(Weather, u64, String)>, key: &str) -> Option<(Weather, u64)> {
    let (w, age, k) = saved?;
    (k == key && age <= MAX_AGE_SECS).then_some((w, age))
}

/// Son havayı kaydeder (yalnızca dock'un hava iş parçacığı çağırır).
pub fn save(w: &Weather, lat: f64, lon: f64) -> bool {
    write_to(&path(), now_secs(), &place_key(lat, lon), w)
}

/// Kaydedilmiş son hava ve yaşı (saniye); yerden bağımsız.
pub fn load() -> Option<(Weather, u64)> {
    read_from(&path(), now_secs()).map(|(w, age, _)| (w, age))
}

/// Verilen yer için kaydedilmiş hava ve yaşı (yer farklıysa `None`).
pub fn load_for(lat: f64, lon: f64) -> Option<(Weather, u64)> {
    pick(read_from(&path(), now_secs()), &place_key(lat, lon))
}

/// Sahneyi belirleyecek hava: hata ayıklama ortam değişkeni varsa o; yoksa
/// özellik açık, yer seçili ve 3 saatten genç bir kayıt varsa o; yoksa `None`
/// (çağıran kullanıcının elle seçtiği sahneye döner).
pub fn effective(cfg: &LiveWeather) -> Option<Weather> {
    if let Some(w) = debug_override() {
        return Some(w);
    }
    if !cfg.active() {
        return None;
    }
    load_for(cfg.lat, cfg.lon).map(|(w, _)| w)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weather::Sky;

    fn sample() -> Weather {
        Weather { sky: Sky::Rain, intensity: 0.6, is_day: false, thunder: true }
    }

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("carlui-weather-test-{}-{name}.json", std::process::id()))
    }

    #[test]
    fn gidip_gelir() {
        let p = tmp("rt");
        assert!(write_to(&p, 1000, "39.93,32.86", &sample()));
        let (w, age, key) = read_from(&p, 1250).unwrap();
        assert_eq!(w, sample());
        assert_eq!(age, 250);
        assert_eq!(key, "39.93,32.86");
        // Geçici dosya kalmaz.
        assert!(!p.with_extension("json.tmp").exists());
        // Saat geriye giderse yaş sıfır.
        assert_eq!(read_from(&p, 900).unwrap().1, 0);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn dosya_yok_ya_da_bozuk() {
        let p = tmp("bad");
        let _ = std::fs::remove_file(&p);
        assert!(read_from(&p, 0).is_none());
        std::fs::write(&p, "{ bozuk").unwrap();
        assert!(read_from(&p, 0).is_none());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn bayatlik_ve_yer_kurali() {
        let s = |age| Some((sample(), age, "1.00,2.00".to_string()));
        assert!(pick(s(0), "1.00,2.00").is_some());
        assert!(pick(s(MAX_AGE_SECS), "1.00,2.00").is_some());
        assert!(pick(s(MAX_AGE_SECS + 1), "1.00,2.00").is_none());
        assert!(pick(s(10), "3.00,4.00").is_none(), "başka yerin havası kullanılmaz");
        assert!(pick(None, "1.00,2.00").is_none());
    }

    #[test]
    fn kapaliyken_hava_yok() {
        // Testler CARLUI_LIVE_WEATHER koymaz.
        if debug_override().is_none() {
            assert!(effective(&LiveWeather::default()).is_none());
        }
    }

    #[test]
    fn yer_anahtari_yuvarlar() {
        assert_eq!(place_key(51.50735, -0.12776), "51.51,-0.13");
    }
}
