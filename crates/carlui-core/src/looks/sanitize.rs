//! Görünüm değerlerini güvenli aralığa çeker. Hem içe aktarılan dosyalar hem
//! elle yazılmış `looks.json` hem de hazırlar buradan geçer; aralıklar
//! ayarlar uygulamasındaki kaydırıcılarla aynıdır.

use super::{default_style_color, norm_hex, Look};
use crate::config::{AccentSource, DockPosition};

/// Ad en çok bu kadar karakter.
pub const NAME_MAX: usize = 40;

pub const GLASS: (f32, f32) = (0.0, 1.0);
pub const CORNER: (f32, f32) = (0.5, 2.0);
pub const ANIM: (f32, f32) = (0.5, 2.5);
pub const ICON: (f32, f32) = (20.0, 64.0);
pub const MARGIN: (f32, f32) = (0.0, 60.0);
pub const SCALE: (f32, f32) = (0.5, 1.6);

/// Sayıyı aralığa çeker; sonlu değilse (NaN, sonsuz) varsayılan kullanılır.
pub fn clamp(v: f32, (lo, hi): (f32, f32), default: f32) -> f32 {
    if v.is_finite() { v.clamp(lo, hi) } else { default }
}

/// Görünüm adını temizler: denetim ve dosya adına yakışmayan karakterler
/// atılır, boşluklar teke iner, en çok `NAME_MAX` karakter.
pub fn clean_name(s: &str) -> String {
    let kept: String = s.chars().map(|c| if c.is_control() || "\\/:*?\"<>|".contains(c) { ' ' } else { c }).collect();
    kept.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(NAME_MAX).collect()
}

/// Menü başlık rengi: "system", "off" ya da "#RRGGBB"; tanınmayan "system" olur.
pub fn menu_accent(s: &str) -> String {
    let v = s.trim().to_lowercase();
    match v.as_str() {
        "off" | "kapalı" | "kapali" | "none" => "off".into(),
        "system" | "" => "system".into(),
        _ => norm_hex(&v).unwrap_or_else(|| "system".into()),
    }
}

/// İmleç renkleri "#rrggbb" (küçük harf) tutulur; ayarlar sayfası da böyle yazar.
fn cursor_hex(s: &str) -> Option<String> {
    norm_hex(s).map(|h| h.to_lowercase())
}

/// Görünümün aralığa çekilmiş kopyası.
pub fn sanitized(look: &Look) -> Look {
    let d = Look::default();
    let mut l = look.clone();
    l.name = clean_name(&l.name);

    let t = &mut l.theme;
    t.glass_density = clamp(t.glass_density, GLASS, d.theme.glass_density);
    t.corner_radius = clamp(t.corner_radius, CORNER, d.theme.corner_radius);
    t.animation_speed = clamp(t.animation_speed, ANIM, d.theme.animation_speed);
    t.custom_accent = norm_hex(&t.custom_accent).unwrap_or_default();
    if t.accent_source == AccentSource::Custom && t.custom_accent.is_empty() {
        // Özel renk okunamadıysa palete düşülür (ayarların kendisi de böyle yapar).
        t.accent_source = AccentSource::Palette;
    }

    let k = &mut l.dock;
    k.icon_size = clamp(k.icon_size, ICON, d.dock.icon_size);
    k.bottom_margin = clamp(k.bottom_margin, MARGIN, d.dock.bottom_margin);
    k.menu_accent = menu_accent(&k.menu_accent);
    if k.taskbar {
        // Görev çubuğu görünümü yalnızca alttadır.
        k.position = DockPosition::Bottom;
    }

    let c = &mut l.cursor;
    c.color = cursor_hex(&c.color).unwrap_or_else(|| default_style_color(c.style));
    c.edge = if c.edge.trim().eq_ignore_ascii_case("auto") { "auto".into() } else { cursor_hex(&c.edge).unwrap_or_else(|| "auto".into()) };
    c.scale = clamp(c.scale, SCALE, d.cursor.scale);
    l
}
