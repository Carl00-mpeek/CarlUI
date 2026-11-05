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
        Self {
            accent: t.palette.accent(),
            glass_tint: Rgba(14, 16, 28, (110.0 + density * 120.0) as u8),
            rim_top: Rgba(255, 255, 255, 0x50),
            rim_bottom: Rgba(255, 255, 255, 0x12),
            gloss: Rgba(255, 255, 255, 0x18),
            hover: Rgba(255, 255, 255, 0x1E),
            separator: Rgba(255, 255, 255, 0x2A),
            radius_scale: t.corner_radius.clamp(0.3, 1.5),
            anim_scale: 1.0 / t.animation_speed.clamp(0.25, 3.0),
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
