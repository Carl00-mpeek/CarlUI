//! Bu modül "anlık tekrar kaydının ayarları nerede durur?" sorununu çözüyor:
//! `config.toml`'daki `[replay]` bölümü (açık/kapalı, kaç dakika geriye,
//! kare hızı, kalite, çözünürlük, sıkıştırma, ses, mikrofon ve düzeyleri) ve kaydetme /
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
    /// 30, 45, 60, 90, 120 ya da 144.
    pub fps: u32,
    pub quality: Quality,
    pub resolution: Resolution,
    pub codec: Codec,
    /// Bilgisayarın sesini de kaydet.
    pub audio: bool,
    /// Varsayılan mikrofonu da kaydet (bilgisayar sesiyle karıştırılır).
    pub mic: bool,
    /// Bilgisayar sesinin kayıttaki düzeyi (yüzde, 0–200).
    pub audio_volume: u32,
    /// Mikrofonun kayıttaki düzeyi (yüzde, 0–200).
    pub mic_volume: u32,
    /// Kayıtların klasörü; boşsa Videolar\CarlUI.
    pub folder: String,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        Self { enabled: false, minutes: 5, fps: 60, quality: Quality::Medium, resolution: Resolution::Native, codec: Codec::H264, audio: true, mic: false, audio_volume: 100, mic_volume: 100, folder: String::new() }
    }
}

impl ReplayConfig {
    pub const MAX_MINUTES: u32 = 30;
    pub const MAX_VOLUME: u32 = 200;

    pub fn minutes(&self) -> u32 {
        self.minutes.clamp(1, Self::MAX_MINUTES)
    }

    /// Seçilebilen kare hızları; ayardaki değer en yakınına yuvarlanır.
    pub const FPS: [u32; 6] = [30, 45, 60, 90, 120, 144];

    pub fn fps(&self) -> u32 {
        *Self::FPS.iter().min_by_key(|&&f| f.abs_diff(self.fps)).unwrap()
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
        // 60 fps üstünde kareler birbirine çok benzer: bit hızı yarı eğimle artar.
        let fps = self.fps() as f64;
        let fps = if fps > 60.0 { 60.0 + (fps - 60.0) * 0.5 } else { fps };
        let b = w as f64 * h as f64 * fps * per_px * codec;
        (b as u32).clamp(2_000_000, 100_000_000)
    }

    /// Ekran `w`x`h` iken bir dakikalık kaydın yaklaşık boyutu (MB), ses dahil.
    pub fn mb_per_minute(&self, w: u32, h: u32) -> f64 {
        let (w, h) = self.out_size(w, h);
        let audio = if self.audio || self.mic { 192_000.0 } else { 0.0 };
        (self.bitrate(w, h) as f64 + audio) / 8.0 * 60.0 / 1e6
    }

    /// Yüksek kare hızı (> 60) ya da 1080p üstü çözünürlük: diskte çok yer
    /// kaplar, oyunla aynı diskteyse takılmaya yol açabilir.
    pub fn heavy(&self, w: u32, h: u32) -> bool {
        self.fps() > 60 || self.out_size(w, h).1 > 1080
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
