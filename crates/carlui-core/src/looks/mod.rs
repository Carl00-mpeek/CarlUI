//! Bu modül "görünümü tek tıkla değiştirmek, beğendiğini saklamak ve
//! paylaşmak" sorununu çözüyor. Bir *görünüm* (look) yalnızca göze görünen
//! ayarların adlı bir demetidir: tema, dock'un görünüşü, adanın stili ve imleç.
//! Sabitlenmiş uygulamalar, yığınlar, davranış anahtarları, kısayollar, dil,
//! duvar kâğıdı rengi ve "hareketi azalt" gibi erişilebilirlik ayarlarına asla
//! dokunulmaz.
//!
//! Parçalar:
//! - `builtin`: hazır görünümler,
//! - `sanitize`: değerleri güvenli aralığa çekme ve ad temizliği,
//! - `file`: paylaşılan `.carllook` dosya biçimi,
//! - `store`: kullanıcının kendi görünümleri (`looks.json`).
//!
//! Hangi görünümün "seçili" olduğu saklanmaz; ayarlara bakılarak hesaplanır
//! (`current`). Bir değer elle değiştirilince sonuç `Which::Custom` olur.

pub mod builtin;
pub mod file;
pub mod sanitize;
pub mod store;

#[cfg(test)]
mod tests;

use crate::{
    accent::{parse_hex, to_hex},
    config::{AccentSource, Config, CursorConfig, CursorStyle, DockConfig, DockPosition, IslandConfig, IslandStyle, TaskbarSize, ThemeConfig},
    theme::{Palette, ThemeMode},
};
use serde::{Deserialize, Serialize};

/// Görünümün tema kısmı.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct LookTheme {
    pub mode: ThemeMode,
    pub palette: Palette,
    pub accent_source: AccentSource,
    /// Yalnızca `accent_source = custom` iken anlamlı; boşsa mevcut özel renk korunur.
    pub custom_accent: String,
    #[serde(serialize_with = "round")]
    pub glass_density: f32,
    #[serde(serialize_with = "round")]
    pub corner_radius: f32,
    #[serde(serialize_with = "round")]
    pub animation_speed: f32,
}

/// Sayılar dosyada "0.800000011920929" yerine "0.8" görünsün.
fn round<S: serde::Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_f64((*v as f64 * 1000.0).round() / 1000.0)
}

/// Görünümün dock kısmı: yalnızca şekil, boyut ve süs; içerik ve davranış yok.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct LookDock {
    pub position: DockPosition,
    pub taskbar: bool,
    pub taskbar_size: TaskbarSize,
    #[serde(serialize_with = "round")]
    pub icon_size: f32,
    #[serde(serialize_with = "round")]
    pub bottom_margin: f32,
    pub separators: bool,
    pub line_indicator: bool,
    pub show_labels: bool,
    pub menu_accent: String,
}

/// Görünümün ada kısmı.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct LookIsland {
    pub style: IslandStyle,
    pub media_tint: bool,
}

/// Görünümün imleç kısmı. `color` seçili stilin rengidir (Neon rengi,
/// Mürekkep gövdesi…); öteki stillerin renkleri olduğu gibi kalır. İmlecin
/// açık/kapalı olması görünüme dahil değildir: kapatan kişi bunu bilerek yapmıştır.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct LookCursor {
    pub style: CursorStyle,
    pub color: String,
    pub glow: bool,
    pub edge: String,
    #[serde(serialize_with = "round")]
    pub scale: f32,
}

/// Adlı görünüm demeti.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Look {
    pub name: String,
    pub theme: LookTheme,
    pub dock: LookDock,
    pub island: LookIsland,
    pub cursor: LookCursor,
}

impl Default for LookTheme {
    fn default() -> Self {
        let t = ThemeConfig::default();
        Self { mode: t.mode, palette: t.palette, accent_source: t.accent_source, custom_accent: String::new(), glass_density: t.glass_density, corner_radius: t.corner_radius, animation_speed: t.animation_speed }
    }
}

impl Default for LookDock {
    fn default() -> Self {
        let d = DockConfig::default();
        Self { position: d.position, taskbar: d.taskbar, taskbar_size: d.taskbar_size, icon_size: d.icon_size, bottom_margin: d.bottom_margin, separators: d.separators, line_indicator: d.line_indicator, show_labels: d.show_labels, menu_accent: d.menu_accent }
    }
}

impl Default for LookIsland {
    fn default() -> Self {
        let i = IslandConfig::default();
        Self { style: i.style, media_tint: i.media_tint }
    }
}

impl Default for LookCursor {
    fn default() -> Self {
        let c = CursorConfig::default();
        Self { style: c.style, color: c.style_color().to_string(), glow: c.glow, edge: c.edge, scale: c.scale }
    }
}

/// Stilin varsayılan rengi (bozuk renk gelince buna düşülür).
pub(crate) fn default_style_color(style: CursorStyle) -> String {
    let d = CursorConfig { style, ..CursorConfig::default() };
    d.style_color().to_string()
}

/// Şu anki ayarlardan görünüm çıkarır (değerler aralığa çekilmiş olarak).
pub fn capture(cfg: &Config, name: &str) -> Look {
    let t = &cfg.theme;
    let d = &cfg.dock;
    let c = &cfg.cursor;
    let raw = Look {
        name: sanitize::clean_name(name),
        theme: LookTheme {
            mode: t.mode,
            palette: t.palette,
            accent_source: t.accent_source,
            // Palet ya da duvar kâğıdı seçiliyken özel renk görünüme taşınmaz.
            custom_accent: if t.accent_source == AccentSource::Custom { t.custom_accent.clone() } else { String::new() },
            glass_density: t.glass_density,
            corner_radius: t.corner_radius,
            animation_speed: t.animation_speed,
        },
        dock: LookDock {
            position: d.position,
            taskbar: d.taskbar,
            taskbar_size: d.taskbar_size,
            icon_size: d.icon_size,
            bottom_margin: d.bottom_margin,
            separators: d.separators,
            line_indicator: d.line_indicator,
            show_labels: d.show_labels,
            menu_accent: d.menu_accent.clone(),
        },
        island: LookIsland { style: cfg.island.style, media_tint: cfg.island.media_tint },
        cursor: LookCursor { style: c.style, color: c.style_color().to_string(), glow: c.glow, edge: c.edge.clone(), scale: c.scale },
    };
    sanitize::sanitized(&raw)
}

/// Görünümü ayarlara uygular. Değerler önce güvenli aralığa çekilir; görünüme
/// ait olmayan hiçbir alana dokunulmaz.
pub fn apply(look: &Look, cfg: &mut Config) {
    let l = sanitize::sanitized(look);
    let t = &mut cfg.theme;
    t.mode = l.theme.mode;
    t.palette = l.theme.palette;
    t.accent_source = l.theme.accent_source;
    if !l.theme.custom_accent.is_empty() {
        t.custom_accent = l.theme.custom_accent.clone();
    }
    t.glass_density = l.theme.glass_density;
    t.corner_radius = l.theme.corner_radius;
    t.animation_speed = l.theme.animation_speed;

    let d = &mut cfg.dock;
    d.position = l.dock.position;
    d.taskbar = l.dock.taskbar;
    d.taskbar_size = l.dock.taskbar_size;
    d.icon_size = l.dock.icon_size;
    d.bottom_margin = l.dock.bottom_margin;
    d.separators = l.dock.separators;
    d.line_indicator = l.dock.line_indicator;
    d.show_labels = l.dock.show_labels;
    d.menu_accent = l.dock.menu_accent.clone();

    cfg.island.style = l.island.style;
    cfg.island.media_tint = l.island.media_tint;

    let c = &mut cfg.cursor;
    c.style = l.cursor.style;
    let color = l.cursor.color.clone();
    match c.style {
        CursorStyle::Neon => c.color = color,
        CursorStyle::Ink => c.ink_color = color,
        CursorStyle::Soft => c.soft_color = color,
        CursorStyle::Zen => c.zen_color = color,
    }
    c.glow = l.cursor.glow;
    c.edge = l.cursor.edge.clone();
    c.scale = l.cursor.scale;
}

/// Görünüm uygulanmadan önceki görünüş: tek adımlık "Geri al" için. Görünüme
/// giren her alanı (öteki imleç renkleri ve özel renk dahil) birebir saklar.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    theme: ThemeConfig,
    dock: DockConfig,
    island: IslandConfig,
    cursor: CursorConfig,
}

impl Snapshot {
    pub fn take(cfg: &Config) -> Self {
        // Sabitlenmişler gibi büyük listeler geri almaya gerekmez.
        let dock = DockConfig { pinned: Vec::new(), stacks: Vec::new(), ..cfg.dock.clone() };
        Self { theme: cfg.theme.clone(), dock, island: cfg.island.clone(), cursor: cfg.cursor.clone() }
    }

    /// Görünüşü geri koyar; arada değişmiş olabilecek görünüm dışı alanlar
    /// (duvar kâğıdı rengi, hareketi azalt, imleç açık/kapalı…) korunur.
    pub fn restore(&self, cfg: &mut Config) {
        let (s, t) = (&self.theme, &mut cfg.theme);
        t.mode = s.mode;
        t.palette = s.palette;
        t.accent_source = s.accent_source;
        t.custom_accent = s.custom_accent.clone();
        t.glass_density = s.glass_density;
        t.corner_radius = s.corner_radius;
        t.animation_speed = s.animation_speed;

        let (s, d) = (&self.dock, &mut cfg.dock);
        d.position = s.position;
        d.taskbar = s.taskbar;
        d.taskbar_size = s.taskbar_size;
        d.icon_size = s.icon_size;
        d.bottom_margin = s.bottom_margin;
        d.separators = s.separators;
        d.line_indicator = s.line_indicator;
        d.show_labels = s.show_labels;
        d.menu_accent = s.menu_accent.clone();

        cfg.island.style = self.island.style;
        cfg.island.media_tint = self.island.media_tint;

        let (s, c) = (&self.cursor, &mut cfg.cursor);
        c.style = s.style;
        c.color = s.color.clone();
        c.ink_color = s.ink_color.clone();
        c.soft_color = s.soft_color.clone();
        c.zen_color = s.zen_color.clone();
        c.glow = s.glow;
        c.edge = s.edge.clone();
        c.scale = s.scale;
    }
}

/// Görünümü uygular ve geri alma için önceki görünüşü döndürür.
pub fn apply_undoable(look: &Look, cfg: &mut Config) -> Snapshot {
    let snap = Snapshot::take(cfg);
    apply(look, cfg);
    snap
}

/// Ayarlar şu an hangi görünüme denk geliyor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    Builtin(usize),
    User(usize),
    /// Hiçbirine denk gelmiyor ("Özel").
    Custom,
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.011
}

fn same_color(a: &str, b: &str) -> bool {
    match (parse_hex(a), parse_hex(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a.trim().eq_ignore_ascii_case(b.trim()),
    }
}

/// Ayarlar bu görünüme denk mi? Görünüşü etkilemeyen farklar sayılmaz
/// (palet seçiliyken saklı özel renk, görev çubuğu kapalıyken boyutu,
/// ışımasız stilde ışıma anahtarı).
pub fn matches(look: &Look, cfg: &Config) -> bool {
    let l = sanitize::sanitized(look);
    let now = capture(cfg, "");
    let (a, b) = (&l.theme, &now.theme);
    let theme = a.mode == b.mode
        && a.palette == b.palette
        && a.accent_source == b.accent_source
        && (a.accent_source != AccentSource::Custom || a.custom_accent.is_empty() || same_color(&a.custom_accent, &b.custom_accent))
        && near(a.glass_density, b.glass_density)
        && near(a.corner_radius, b.corner_radius)
        && near(a.animation_speed, b.animation_speed);
    let (a, b) = (&l.dock, &now.dock);
    let dock = a.position == b.position
        && a.taskbar == b.taskbar
        && (!a.taskbar || a.taskbar_size == b.taskbar_size)
        && near(a.icon_size, b.icon_size)
        && near(a.bottom_margin, b.bottom_margin)
        && a.separators == b.separators
        && a.line_indicator == b.line_indicator
        && a.show_labels == b.show_labels
        && sanitize::menu_accent(&a.menu_accent).eq_ignore_ascii_case(&sanitize::menu_accent(&b.menu_accent));
    let island = l.island == now.island;
    let (a, b) = (&l.cursor, &now.cursor);
    let glows = matches!(a.style, CursorStyle::Neon | CursorStyle::Zen);
    let cursor = a.style == b.style
        && same_color(&a.color, &b.color)
        && (!glows || a.glow == b.glow)
        && same_color(&a.edge, &b.edge)
        && near(a.scale, b.scale);
    theme && dock && island && cursor
}

/// Ayarlara denk gelen ilk görünüm: önce hazırlar, sonra kullanıcınınkiler.
pub fn current(cfg: &Config, builtins: &[Look], user: &[Look]) -> Which {
    if let Some(i) = builtins.iter().position(|l| matches(l, cfg)) {
        return Which::Builtin(i);
    }
    if let Some(i) = user.iter().position(|l| matches(l, cfg)) {
        return Which::User(i);
    }
    Which::Custom
}

/// Görünümün kartta gösterilecek vurgu rengi (0xRRGGBB, koyu tema ucu).
/// Duvar kâğıdını izleyen görünümde `wallpaper` (ayardaki hesaplanmış renk) kullanılır.
pub fn preview_accent(look: &Look, wallpaper: &str) -> (u32, u32) {
    let t = ThemeConfig {
        palette: look.theme.palette,
        accent_source: look.theme.accent_source,
        custom_accent: look.theme.custom_accent.clone(),
        wallpaper_accent: wallpaper.to_string(),
        ..ThemeConfig::default()
    };
    let (a, b) = t.accent_pair();
    (crate::accent::from_rgba(a), crate::accent::from_rgba(b))
}

/// Renk kodunu "#RRGGBB" biçimine getirir; bozuksa `None`.
pub(crate) fn norm_hex(s: &str) -> Option<String> {
    parse_hex(s).map(to_hex)
}
