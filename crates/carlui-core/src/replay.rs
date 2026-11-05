//! Bu modül "anlık tekrar kaydının ayarları nerede durur?" sorununu çözüyor:
//! `config.toml`'daki `[replay]` bölümü (açık/kapalı, kaç dakika geriye,
//! kare hızı, kalite, ses) ve kaydetme kısayolunun (`shortcuts.replay`)
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
    /// Bilgisayarın sesini de kaydet.
    pub audio: bool,
    /// Kayıtların klasörü; boşsa Videolar\CarlUI.
    pub folder: String,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        Self { enabled: false, minutes: 5, fps: 60, quality: Quality::Medium, audio: true, folder: String::new() }
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

    /// Bit hızı (bit/sn): 1080p60 için orta kalitede ~20 Mbps.
    pub fn bitrate(&self, w: u32, h: u32) -> u32 {
        let per_px = match self.quality {
            Quality::Low => 0.08,
            Quality::Medium => 0.16,
            Quality::High => 0.30,
        };
        let b = w as f64 * h as f64 * self.fps() as f64 * per_px;
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

impl ShortcutsConfig {
    /// Anlık tekrarı kaydetme kısayolu (boşsa None; geçersizse varsayılan Alt+F10).
    pub fn replay_combo(&self) -> Option<crate::keys::Combo> {
        if self.replay.trim().is_empty() {
            return None;
        }
        crate::keys::parse_combo(&self.replay)
            .map_err(|e| crate::warn!("anlık tekrar kısayolu '{}' kullanılamaz: {e}; varsayılan kullanılıyor", self.replay))
            .ok()
            .or_else(|| crate::keys::parse_combo(&Self::default().replay).ok())
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
    }

    #[test]
    fn toml_roundtrip() {
        let r = ReplayConfig::default();
        let s = toml::to_string(&r).unwrap();
        assert_eq!(toml::from_str::<ReplayConfig>(&s).unwrap(), r);
    }
}
