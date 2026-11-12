//! Canlı dock'un "gerçek havayı izle" ayarı (`[dock.live.weather]`).
//! Varsayılan kapalı; eski ayar dosyalarında bölüm yoksa kapalı gelir.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct LiveWeather {
    /// Sahne gerçek havayı izlesin.
    pub enabled: bool,
    /// Seçilen yerin görünen adı ("İstanbul, İstanbul, Türkiye").
    pub place: String,
    pub lat: f64,
    pub lon: f64,
    /// Gece ve gündüz döngüsü: geceyse ay ve yıldızlar da çizilir.
    pub day_night: bool,
}

impl LiveWeather {
    /// Özellik gerçekten çalışıyor mu: açık ve geçerli bir yer seçilmiş.
    pub fn active(&self) -> bool {
        self.enabled && !self.place.trim().is_empty() && self.lat.is_finite() && self.lon.is_finite() && self.lat.abs() <= 90.0 && self.lon.abs() <= 180.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etkinlik() {
        let mut w = LiveWeather::default();
        assert!(!w.active());
        w.enabled = true;
        assert!(!w.active(), "yer seçilmeden etkin olmaz");
        w.place = "Ankara".into();
        w.lat = 39.9;
        w.lon = 32.8;
        assert!(w.active());
        w.lat = 120.0;
        assert!(!w.active());
        w.lat = f64::NAN;
        assert!(!w.active());
    }
}
