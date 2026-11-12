//! Hazır görünümler. Her birinin ayrı bir karakteri var; adları seçili dile
//! göre verilir, kimlikleri (`id`) dilden bağımsız ve kalıcıdır.

use super::{Look, LookCursor, LookDock, LookIsland, LookTheme};
use crate::{
    config::{AccentSource, CursorStyle, DockPosition, IslandStyle, TaskbarSize},
    theme::{Palette, ThemeMode},
};

/// Hazır görünümlerin kalıcı kimlikleri (`all()` ile aynı sırada).
pub const IDS: [&str; 6] = ["carlui", "night", "cloud", "neon", "taskbar", "wallpaper"];

fn look(name: &str, theme: LookTheme, dock: LookDock, island: LookIsland, cursor: LookCursor) -> Look {
    Look { name: name.to_string(), theme, dock, island, cursor }
}

fn cursor(style: CursorStyle, color: &str, glow: bool, scale: f32) -> LookCursor {
    LookCursor { style, color: color.into(), glow, edge: "auto".into(), scale }
}

/// Hazır görünümler; ilk sıradaki CarlUI'nin varsayılanlarıyla birebir aynıdır.
pub fn all() -> Vec<Look> {
    vec![
        // Varsayılan: ilk kurulumdaki görünüş.
        Look { name: "CarlUI".into(), ..Look::default() },
        // Gece: sade, koyu, renksiz; dikkat dağıtmayan grafit cam.
        look(
            t!("Gece", "Night"),
            LookTheme { mode: ThemeMode::Dark, palette: Palette::Graphite, accent_source: AccentSource::Palette, custom_accent: String::new(), glass_density: 0.8, corner_radius: 0.8, animation_speed: 1.0 },
            LookDock { position: DockPosition::Bottom, taskbar: false, taskbar_size: TaskbarSize::Full, icon_size: 28.0, bottom_margin: 10.0, separators: false, line_indicator: true, show_labels: false, menu_accent: "off".into() },
            LookIsland { style: IslandStyle::Pill, media_tint: false },
            cursor(CursorStyle::Ink, "#1c1c22", false, 0.7),
        ),
        // Bulut: açık, ferah; saydam cam, yumuşak köşeler, sakin hareket.
        look(
            t!("Bulut", "Cloud"),
            LookTheme { mode: ThemeMode::Light, palette: Palette::IceBlue, accent_source: AccentSource::Palette, custom_accent: String::new(), glass_density: 0.25, corner_radius: 1.4, animation_speed: 0.9 },
            LookDock { position: DockPosition::Bottom, taskbar: false, taskbar_size: TaskbarSize::Full, icon_size: 34.0, bottom_margin: 16.0, separators: true, line_indicator: false, show_labels: true, menu_accent: "system".into() },
            LookIsland { style: IslandStyle::Pill, media_tint: true },
            cursor(CursorStyle::Soft, "#f7f7f8", false, 0.8),
        ),
        // Neon: canlı; orkide vurgu, iri simgeler, ışıyan imleç, hızlı hareket.
        look(
            "Neon",
            LookTheme { mode: ThemeMode::Dark, palette: Palette::Orchid, accent_source: AccentSource::Palette, custom_accent: String::new(), glass_density: 0.4, corner_radius: 1.2, animation_speed: 1.3 },
            LookDock { position: DockPosition::Bottom, taskbar: false, taskbar_size: TaskbarSize::Full, icon_size: 38.0, bottom_margin: 14.0, separators: true, line_indicator: true, show_labels: true, menu_accent: "system".into() },
            LookIsland { style: IslandStyle::Card, media_tint: true },
            cursor(CursorStyle::Neon, "#ff4fd8", true, 0.9),
        ),
        // Görev çubuğu: tanıdık ve sıkı; kenardan kenara çubuk, çentik ada.
        look(
            t!("Görev çubuğu", "Taskbar"),
            LookTheme { mode: ThemeMode::Dark, palette: Palette::OceanBlue, accent_source: AccentSource::Palette, custom_accent: String::new(), glass_density: 0.65, corner_radius: 0.7, animation_speed: 1.4 },
            LookDock { position: DockPosition::Bottom, taskbar: true, taskbar_size: TaskbarSize::Full, icon_size: 26.0, bottom_margin: 0.0, separators: true, line_indicator: true, show_labels: false, menu_accent: "system".into() },
            LookIsland { style: IslandStyle::Notch, media_tint: false },
            cursor(CursorStyle::Neon, "#3aa0ff", false, 0.7),
        ),
        // Duvar kâğıdı: renkler duvar kâğıdından, açık/koyu Windows'tan gelir.
        look(
            t!("Duvar kâğıdı", "Wallpaper"),
            LookTheme { mode: ThemeMode::System, palette: Palette::BluePurple, accent_source: AccentSource::Wallpaper, custom_accent: String::new(), glass_density: 0.45, corner_radius: 1.0, animation_speed: 1.0 },
            LookDock { position: DockPosition::Bottom, taskbar: false, taskbar_size: TaskbarSize::Full, icon_size: 32.0, bottom_margin: 12.0, separators: true, line_indicator: true, show_labels: true, menu_accent: "system".into() },
            LookIsland { style: IslandStyle::Pill, media_tint: true },
            cursor(CursorStyle::Zen, "#bcd6ff", true, 0.75),
        ),
    ]
}

/// Kimliğe göre hazır görünüm (ilk açılış akışı gibi yerler için).
pub fn by_id(id: &str) -> Option<Look> {
    IDS.iter().position(|i| *i == id).map(|i| all().swap_remove(i))
}
