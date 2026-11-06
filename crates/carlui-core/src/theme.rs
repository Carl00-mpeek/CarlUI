//! Bu modül "tüm yüzeyler aynı görünsün" sorununu, renkleri ve cam
//! parametrelerini tek bir `Theme` değerinden türeterek çözüyor. Modüller
//! renk sabiti tutmaz; her zaman buradan okur.

use serde::{Deserialize, Serialize};

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
    /// Spring periyotlarının çarpanı: küçük = hızlı.
    pub anim_scale: f32,
}

impl Theme {
    pub fn from_config(t: &crate::config::ThemeConfig) -> Self {
        let density = t.glass_density.clamp(0.0, 1.0);
        let (radius_scale, anim_scale) = (t.corner_radius.clamp(0.3, 1.5), 1.0 / t.animation_speed.clamp(0.25, 3.0));
        if t.is_light() {
            // Beyaz cam: süt beyazı tint, parlak üst kenar, serin ince alt kenar.
            let (a0, a1) = t.palette.accent();
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
            accent: t.palette.accent(),
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

/// Arayüz teması: koyu (varsayılan), açık ya da Windows'un uygulama teması.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
    System,
}
