//! Bu modül "tüm yüzeyler aynı görünsün" sorununu, renkleri ve cam
//! parametrelerini tek bir `Theme` değerinden türeterek çözüyor. Modüller
//! renk sabiti tutmaz; her zaman buradan okur.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Palette {
    #[default]
    BluePurple,
    OceanBlue,
    Violet,
    IceBlue,
    Orchid,
    Emerald,
    Graphite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

impl Palette {
    pub const ALL: [Palette; 7] = [
        Palette::BluePurple,
        Palette::OceanBlue,
        Palette::Violet,
        Palette::IceBlue,
        Palette::Orchid,
        Palette::Emerald,
        Palette::Graphite,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Palette::BluePurple => t!("Mavi-Mor", "Blue-Purple"),
            Palette::OceanBlue => "Ocean Blue",
            Palette::Violet => "Violet",
            Palette::IceBlue => "Ice Blue",
            Palette::Orchid => "Orchid",
            Palette::Emerald => "Emerald",
            Palette::Graphite => "Graphite",
        }
    }

    /// Vurgu degradesinin iki ucu (başlangıç, bitiş).
    pub fn accent(self) -> (Rgba, Rgba) {
        match self {
            Palette::BluePurple => (Rgba(0x4F, 0x8B, 0xFF, 255), Rgba(0x8B, 0x5C, 0xF6, 255)),
            Palette::OceanBlue => (Rgba(0x1E, 0x90, 0xFF, 255), Rgba(0x00, 0xB4, 0xD8, 255)),
            Palette::Violet => (Rgba(0x7C, 0x3A, 0xED, 255), Rgba(0xA8, 0x55, 0xF7, 255)),
            Palette::IceBlue => (Rgba(0x7D, 0xD3, 0xFC, 255), Rgba(0x93, 0xC5, 0xFD, 255)),
            Palette::Orchid => (Rgba(0xC0, 0x84, 0xFC, 255), Rgba(0xF0, 0xAB, 0xFC, 255)),
            Palette::Emerald => (Rgba(0x10, 0xB9, 0x81, 255), Rgba(0x34, 0xD3, 0x99, 255)),
            Palette::Graphite => (Rgba(0x9C, 0xA3, 0xAF, 255), Rgba(0x6B, 0x72, 0x80, 255)),
        }
    }
}

/// sRGB göreli parlaklığı (WCAG); `c` = 0xRRGGBB.
fn luminance(c: u32) -> f32 {
    let lin = |v: u32| {
        let v = (v & 0xFF) as f32 / 255.0;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c >> 16) + 0.7152 * lin(c >> 8) + 0.0722 * lin(c)
}

/// İki rengin WCAG kontrast oranı.
pub fn contrast(a: u32, b: u32) -> f32 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Rengi tonunu koruyarak `on` üstünde `ratio` kontrastına ulaşana dek koyulaştırır.
pub fn deepen(c: u32, on: u32, ratio: f32) -> u32 {
    let mut k = 1.0f32;
    let mut out = c;
    while contrast(out, on) < ratio && k > 0.1 {
        k -= 0.03;
        let s = |v: u32| ((v & 0xFF) as f32 * k).round() as u32;
        out = s(c >> 16) << 16 | s(c >> 8) << 8 | s(c);
    }
    out
}

/// Açık temada vurgu: beyaz zeminde görünür, üstündeki beyaz yazı okunur.
/// Gri vurgu serin bir arduvaz tonuna oturur; yoksa yüzey gibi çevrilirdi.
pub fn light_accent(c: u32) -> u32 {
    // Üstündeki yazı yarı saydam olabilir; hedef bu payı içerir.
    let out = deepen(c, 0xFFFFFF, 5.2);
    let ch = [out >> 16 & 0xFF, out >> 8 & 0xFF, out & 0xFF];
    let spread = ch.iter().max().unwrap_or(&0) - ch.iter().min().unwrap_or(&0);
    if spread > 48 { out } else { 0x3A4A6B }
}

/// `light_accent`in `Rgba` biçimi; alfa korunur.
pub fn light_rgba(c: Rgba) -> Rgba {
    let v = light_accent((c.0 as u32) << 16 | (c.1 as u32) << 8 | c.2 as u32);
    Rgba((v >> 16) as u8, (v >> 8) as u8, v as u8, c.3)
}

/// Ayarlardan türetilmiş, çizime hazır tema.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub accent: (Rgba, Rgba),
    /// Koyu füme cam rengi; alfa = cam yoğunluğu.
    pub glass_tint: Rgba,
    /// Üst kenar yansıması ve alt kenar.
    pub rim_top: Rgba,
    pub rim_bottom: Rgba,
    pub gloss: Rgba,
    pub hover: Rgba,
    pub separator: Rgba,
    /// 1.0 = varsayılan köşe yarıçapı.
    pub radius_scale: f32,
    /// Spring periyotlarının çarpanı: küçük = hızlı, 0 = hareket azaltılmış.
    pub anim_scale: f32,
}

impl Theme {
    pub fn from_config(t: &crate::config::ThemeConfig) -> Self {
        let density = t.glass_density.clamp(0.0, 1.0);
        let (radius_scale, anim_scale) = (t.corner_radius.clamp(0.3, 1.5), t.motion_scale());
        let (a0, a1) = t.accent_pair();
        if t.is_light() {
            // Beyaz cam: süt beyazı tint, parlak üst kenar, serin ince alt kenar.
            return Self {
                accent: (light_rgba(a0), light_rgba(a1)),
                glass_tint: Rgba(255, 255, 255, (190.0 + density * 50.0) as u8),
                rim_top: Rgba(255, 255, 255, 0xE6),
                rim_bottom: Rgba(20, 24, 38, 0x26),
                gloss: Rgba(255, 255, 255, 0x50),
                hover: Rgba(40, 60, 110, 0x16),
                separator: Rgba(20, 24, 38, 0x22),
                radius_scale,
                anim_scale,
            };
        }
        Self {
            accent: (a0, a1),
            glass_tint: Rgba(14, 16, 28, (110.0 + density * 120.0) as u8),
            rim_top: Rgba(255, 255, 255, 0x50),
            rim_bottom: Rgba(255, 255, 255, 0x12),
            gloss: Rgba(255, 255, 255, 0x18),
            hover: Rgba(255, 255, 255, 0x1E),
            separator: Rgba(255, 255, 255, 0x2A),
            radius_scale,
            anim_scale,
        }
    }
}

/// Sürecin hareket çarpanı (f32 bitleri): sabit süreli animasyonlar
/// (başlat menüsü, ızgara, dock uçuşu) temayı taşımadan buradan okur.
/// `tone::follow` / `tone::restart_on_change` ayarlardan günceller.
static MOTION: AtomicU32 = AtomicU32::new(0x3F80_0000); // 1.0

/// Hareket çarpanını ayarlar (bkz. `ThemeConfig::motion_scale`).
pub fn set_motion(scale: f32) {
    MOTION.store(scale.max(0.0).to_bits(), Ordering::Relaxed);
}

pub fn motion_scale() -> f32 {
    f32::from_bits(MOTION.load(Ordering::Relaxed))
}

/// Hareket azaltılmış mı (animasyonlar anında bitmeli)?
pub fn reduced_motion() -> bool {
    motion_scale() <= 0.0
}

/// Sabit bir süreyi (ms) hız ayarına ve hareket azaltmaya göre ölçekler;
/// en az 1 ms (bölme hatası olmasın, animasyon ilk karede biter).
pub fn motion_ms(ms: f32) -> f32 {
    scale_ms(ms, motion_scale())
}

/// `motion_ms`in saf hali (sınanabilsin diye küreselden bağımsız).
pub fn scale_ms(ms: f32, scale: f32) -> f32 {
    (ms * scale).max(1.0)
}

/// Arayüz teması: koyu (varsayılan), açık ya da Windows'un uygulama teması.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
    System,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AccentSource, Config, ThemeConfig};

    /// Yeni alanlardan önceki ayar dosyası değişmeden yüklenmeli.
    #[test]
    fn old_theme_section_loads() {
        let old = "[theme]\npalette = \"emerald\"\nglass_density = 0.7\ncorner_radius = 1.2\nanimation_speed = 1.5\nmode = \"light\"\n";
        let c: Config = toml::from_str(old).expect("eski tema bölümü yüklenmeli");
        let t = &c.theme;
        assert_eq!(t.palette, Palette::Emerald);
        assert_eq!(t.mode, ThemeMode::Light);
        assert_eq!(t.accent_source, AccentSource::Palette);
        assert!(t.custom_accent.is_empty() && t.wallpaper_accent.is_empty());
        assert!(!t.reduce_motion);
        assert_eq!(t.accent_pair(), Palette::Emerald.accent());
    }

    #[test]
    fn new_fields_roundtrip() {
        let mut c = Config::default();
        c.theme.accent_source = AccentSource::Wallpaper;
        c.theme.custom_accent = "#FF8800".into();
        c.theme.wallpaper_accent = "#3366CC".into();
        c.theme.reduce_motion = true;
        let s = toml::to_string(&c).unwrap();
        let back: Config = toml::from_str(&s).unwrap();
        assert_eq!(back.theme, c.theme);
    }

    #[test]
    fn accent_source_and_fallback() {
        let mut t = ThemeConfig { palette: Palette::Violet, ..Default::default() };
        // Palet seçiliyken özel renk yok sayılır.
        t.custom_accent = "#FF8800".into();
        assert_eq!(t.accent_pair(), Palette::Violet.accent());
        t.accent_source = AccentSource::Custom;
        assert_eq!(t.accent_pair(), crate::accent::derive(0xFF8800));
        // Bozuk renk ya da henüz hesaplanmamış duvar kâğıdı → palet.
        t.custom_accent = "turuncu".into();
        assert_eq!(t.accent_pair(), Palette::Violet.accent());
        t.accent_source = AccentSource::Wallpaper;
        assert_eq!(t.accent_pair(), Palette::Violet.accent());
        t.wallpaper_accent = "#2A9D8F".into();
        assert_eq!(t.accent_pair(), crate::accent::derive(0x2A9D8F));
    }

    #[test]
    fn reduce_motion_multiplier() {
        let mut t = ThemeConfig::default();
        assert_eq!(t.motion_scale(), 1.0);
        t.animation_speed = 2.0;
        assert_eq!(t.motion_scale(), 0.5);
        t.animation_speed = 100.0; // sınırlanır
        assert!((t.motion_scale() - 1.0 / 3.0).abs() < 1e-6);
        t.reduce_motion = true;
        assert_eq!(t.motion_scale(), 0.0);
        assert_eq!(Theme::from_config(&t).anim_scale, 0.0);
        assert_eq!(scale_ms(260.0, 1.0), 260.0);
        assert_eq!(scale_ms(260.0, 0.5), 130.0);
        // Hareket azaltılınca süre 1 ms'ye iner (ilk karede biter, sıfıra bölme yok).
        assert_eq!(scale_ms(260.0, 0.0), 1.0);
    }
}
