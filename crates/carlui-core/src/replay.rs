//! Bu modül "anlık tekrar kaydının ayarları nerede durur?" sorununu çözüyor:
//! `config.toml`'daki `[replay]` bölümü (açık/kapalı, kaç dakika geriye,
//! kare hızı, kalite, çözünürlük, sıkıştırma, ses, mikrofon) ve kaydetme /
//! panel kısayollarının (`shortcuts.replay`, `shortcuts.replay_panel`)
//! doğrulanması. `config.rs` büyümesin diye ayrı dosyadadır.

use crate::config::ShortcutsConfig;
use serde::{Deserialize, Serialize};

/// Kayıt kalitesi; bit hızı çözünürlüğe göre ölçeklenir.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    Low,
    #[default]
    Medium,
    High,
}

/// Kaydın çözünürlüğü; ekran daha küçükse ekranın kendi çözünürlüğü kullanılır.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Resolution {
    #[default]
    Native,
    P1440,
    P1080,
    P720,
}

/// Sıkıştırma: H.264 her yerde açılır; H.265 aynı kalitede ~%35 daha küçük.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    #[default]
    H264,
    Hevc,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct ReplayConfig {
    /// Anlık tekrar arka planda kayıt yapsın.
    pub enabled: bool,
    /// Kısayola basınca kaydedilecek süre (dakika, 1–30).
    pub minutes: u32,
    /// 30 ya da 60.
    pub fps: u32,
    pub quality: Quality,
    pub resolution: Resolution,
    pub codec: Codec,
    /// Bilgisayarın sesini de kaydet.
    pub audio: bool,
    /// Varsayılan mikrofonu da kaydet (bilgisayar sesiyle karıştırılır).
    pub mic: bool,
    /// Kayıtların klasörü; boşsa Videolar\CarlUI.
    pub folder: String,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        Self { enabled: false, minutes: 5, fps: 60, quality: Quality::Medium, resolution: Resolution::Native, codec: Codec::H264, audio: true, mic: false, folder: String::new() }
    }
}

impl ReplayConfig {
    pub const MAX_MINUTES: u32 = 30;

    pub fn minutes(&self) -> u32 {
        self.minutes.clamp(1, Self::MAX_MINUTES)
    }

    pub fn fps(&self) -> u32 {
        if self.fps <= 30 {
            30
        } else {
            60
        }
    }

    /// Ekran `w`x`h` iken kaydın boyutu (çift sayılar, en-boy oranı korunur).
    pub fn out_size(&self, w: u32, h: u32) -> (u32, u32) {
        let target = match self.resolution {
            Resolution::Native => 0,
            Resolution::P1440 => 1440,
            Resolution::P1080 => 1080,
            Resolution::P720 => 720,
        };
        if target == 0 || h <= target {
            return (w & !1, h & !1);
        }
        let nw = (w as u64 * target as u64 / h.max(1) as u64) as u32;
        (nw & !1, target)
    }

    /// Bit hızı (bit/sn): 1080p60 için orta kalitede H.264 ~20 Mbps.
    pub fn bitrate(&self, w: u32, h: u32) -> u32 {
        let per_px = match self.quality {
            Quality::Low => 0.08,
            Quality::Medium => 0.16,
            Quality::High => 0.30,
        };
        let codec = if self.codec == Codec::Hevc { 0.65 } else { 1.0 };
        let b = w as f64 * h as f64 * self.fps() as f64 * per_px * codec;
        (b as u32).clamp(2_000_000, 100_000_000)
    }
}

impl ReplayConfig {
    /// Kaydedilen videoların klasörü (`folder` boşsa Videolar\CarlUI).
    pub fn folder(&self) -> std::path::PathBuf {
        if !self.folder.trim().is_empty() {
            return std::path::PathBuf::from(self.folder.trim());
        }
        crate::win::videos_dir().unwrap_or_else(|| crate::paths::data_dir()).join("CarlUI")
    }
}

fn combo(s: &str, default: &str, what: &str) -> Option<crate::keys::Combo> {
    if s.trim().is_empty() {
        return None;
    }
    crate::keys::parse_combo(s)
        .map_err(|e| crate::warn!("{what} kısayolu '{s}' kullanılamaz: {e}; varsayılan kullanılıyor"))
        .ok()
        .or_else(|| crate::keys::parse_combo(default).ok())
}

impl ShortcutsConfig {
    /// Anlık tekrarı kaydetme kısayolu (boşsa None; geçersizse varsayılan Alt+F10).
    pub fn replay_combo(&self) -> Option<crate::keys::Combo> {
        combo(&self.replay, &Self::default().replay, "anlık tekrar")
    }

    /// Anlık tekrar paneli kısayolu (boşsa None; geçersizse varsayılan Alt+F8).
    pub fn replay_panel_combo(&self) -> Option<crate::keys::Combo> {
        combo(&self.replay_panel, &Self::default().replay_panel, "anlık tekrar paneli")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits() {
        let mut r = ReplayConfig::default();
        assert_eq!(r.minutes(), 5);
        r.minutes = 99;
        assert_eq!(r.minutes(), 30);
        r.minutes = 0;
        assert_eq!(r.minutes(), 1);
        let b = r.bitrate(1920, 1080);
        assert!((15_000_000..25_000_000).contains(&b), "{b}");
        assert!(ShortcutsConfig::default().replay_combo().is_some());
        assert!(ShortcutsConfig::default().replay_panel_combo().is_some());
    }

    #[test]
    fn sizes() {
        let mut r = ReplayConfig::default();
        assert_eq!(r.out_size(2560, 1440), (2560, 1440));
        r.resolution = Resolution::P1080;
        assert_eq!(r.out_size(2560, 1440), (1920, 1080));
        assert_eq!(r.out_size(1366, 768), (1366, 768));
        r.resolution = Resolution::P720;
        assert_eq!(r.out_size(3440, 1440), (1720, 720));
    }

    #[test]
    fn toml_roundtrip() {
        let r = ReplayConfig { mic: true, codec: Codec::Hevc, resolution: Resolution::P1080, ..Default::default() };
        let s = toml::to_string(&r).unwrap();
        assert_eq!(toml::from_str::<ReplayConfig>(&s).unwrap(), r);
        // Eski ayar dosyası (yeni alanlar yok) varsayılanlarla okunur.
        let old: ReplayConfig = toml::from_str("enabled = true\nminutes = 3").unwrap();
        assert_eq!((old.minutes, old.mic, old.codec), (3, false, Codec::H264));
    }
}
