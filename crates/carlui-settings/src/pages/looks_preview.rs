//! Görünüm kartındaki küçük önizleme: resim kullanılmadan, boya katmanıyla
//! çizilen minik bir masaüstü. Üstte ada (stiline göre hap, çentik ya da
//! kart), altta ya da yanda dock (simge boyutu, ayraç, çalışan çizgisi/
//! noktası, etiketler, görev çubuğu biçimi), ortada imleç noktası. Renkler
//! görünümün kendi teması ve vurgusundan gelir; ayarlar penceresinin açık/koyu
//! temasından etkilenmesin diye `tone::literal` içinde çizilir.

use carlui_core::{
    accent::parse_hex,
    config::{DockPosition, IslandStyle, TaskbarSize},
    looks::{self, Look},
    theme::ThemeMode,
};
use carlui_ui::paint::{mix, Painter, R};

/// Ayarlar penceresi de karşılama penceresi de (farklı eylem türleriyle) çizer.
type P<'a, A> = Painter<'a, A>;

/// Önizlemede gösterilen dock simgesi sayısı.
const ICONS: usize = 5;

/// Görünümün açık temalı olup olmadığı ("Sistem" ise şu anki Windows ayarı).
fn is_light(look: &Look) -> bool {
    match look.theme.mode {
        ThemeMode::Dark => false,
        ThemeMode::Light => true,
        ThemeMode::System => carlui_ui::tone::light(),
    }
}

/// `r` alanına görünümün önizlemesini çizer. `wallpaper`: ayardaki hesaplanmış
/// duvar kâğıdı rengi (duvar kâğıdını izleyen görünümler için).
pub fn draw<A: Clone>(p: &P<A>, r: R, look: &Look, wallpaper: &str) {
    let light = is_light(look);
    let (a, b) = looks::preview_accent(look, wallpaper);
    carlui_ui::tone::literal(|| {
        let base = if light { 0xE6EAF1 } else { 0x15171D };
        p.round(r, 6.0, base, 1.0);
        // Duvar kâğıdı yerine vurgu tonlu yumuşak bir leke.
        let blob = R::new(r.x + r.w * 0.12, r.y + r.h * 0.18, r.w * 0.76, r.h * 0.62);
        p.round(blob, blob.h / 2.0, mix(base, a, 0.22), 1.0);
        p.round(blob.inset(blob.h * 0.18), blob.h * 0.32, mix(base, b, 0.22), 1.0);

        let s = Style {
            glass: if light { 0xFFFFFF } else { 0x23252D },
            glass_a: 0.55 + 0.4 * look.theme.glass_density,
            ink: if light { 0x1A1C22 } else { 0xF2F3F5 },
            a,
            b,
            round: look.theme.corner_radius,
        };
        island(p, r, look, &s);
        dock(p, r, look, &s);
        cursor(p, r, look);
    });
}

struct Style {
    glass: u32,
    glass_a: f32,
    ink: u32,
    a: u32,
    b: u32,
    round: f32,
}

fn island<A: Clone>(p: &P<A>, r: R, look: &Look, s: &Style) {
    let cx = r.x + r.w / 2.0;
    let body = match look.island.style {
        IslandStyle::Pill => {
            let b = R::new(cx - 22.0, r.y + 6.0, 44.0, 10.0);
            p.round(b, 5.0, 0x0B0C10, 0.92);
            b
        }
        IslandStyle::Card => {
            let b = R::new(cx - 25.0, r.y + 6.0, 50.0, 13.0);
            p.round(b, 2.5 * s.round, 0x0B0C10, 0.92);
            b
        }
        IslandStyle::Notch => {
            // Üst kenara yapışık: üst köşeler düz, alt köşeler yuvarlak.
            let b = R::new(cx - 24.0, r.y, 48.0, 11.0);
            p.round(b, 4.0 * s.round, 0x0B0C10, 0.92);
            p.fill(R::new(b.x, b.y, b.w, b.h / 2.0), 0x0B0C10, 0.92);
            b
        }
    };
    // Çalan medya: renklendirme açıksa vurgu, değilse soluk beyaz.
    let (c, al) = if look.island.media_tint { (s.a, 1.0) } else { (0xFFFFFF, 0.55) };
    let d = 4.0;
    p.round(R::new(body.x + 5.0, body.y + (body.h - d) / 2.0, d, d), d / 2.0, c, al);
    p.fill(R::new(body.right() - 13.0, body.y + body.h / 2.0 - 0.75, 8.0, 1.5), c, al * 0.8);
}

fn dock<A: Clone>(p: &P<A>, r: R, look: &Look, s: &Style) {
    let d = &look.dock;
    // Gerçek simge boyutu 20..64 → önizlemede 5..12 px.
    let icon = (5.0 + (d.icon_size - 20.0) / 44.0 * 7.0).clamp(5.0, 12.0);
    let gap = (icon * 0.3).max(2.0);
    let pad = gap + 1.0;
    let sep = if d.separators { gap + 1.0 } else { 0.0 };
    let label = if d.show_labels && !d.position.vertical() && !d.taskbar { 3.0 } else { 0.0 };
    let len = ICONS as f32 * icon + (ICONS as f32 - 1.0) * gap + sep + pad * 2.0;
    let thick = icon + pad * 2.0 + label;

    if d.taskbar {
        let h = thick;
        let y = r.bottom() - h;
        let w = if d.taskbar_size == TaskbarSize::Full { r.w } else { len + 16.0 };
        let x = r.x + (r.w - w) / 2.0;
        // Alt köşeler kartla birlikte yuvarlak, üstü düz.
        p.round(R::new(x, y, w, h), 6.0, s.glass, s.glass_a);
        p.fill(R::new(x, y, w, h / 2.0), s.glass, s.glass_a);
        p.fill(R::new(x, y, w, 0.8), s.ink, 0.12);
        icons(p, r.x + (r.w - len) / 2.0 + pad, y + pad, icon, gap, sep, false, d.line_indicator, false, s);
        return;
    }

    // Kenar boşluğu 0..60 → 2..10 px.
    let m = 2.0 + d.bottom_margin.clamp(0.0, 60.0) / 60.0 * 8.0;
    let rad = (thick * 0.32 * s.round).min(thick / 2.0);
    let body = match d.position {
        DockPosition::Bottom => R::new(r.x + (r.w - len) / 2.0, r.bottom() - m - thick, len, thick),
        DockPosition::Left => R::new(r.x + m, r.y + (r.h - len) / 2.0 + 4.0, thick, len),
        DockPosition::Right => R::new(r.right() - m - thick, r.y + (r.h - len) / 2.0 + 4.0, thick, len),
    };
    p.round(body, rad, s.glass, s.glass_a);
    p.outline(body, rad, s.ink, 0.10);
    icons(p, body.x + pad, body.y + pad, icon, gap, sep, d.position.vertical(), d.line_indicator, label > 0.0, s);
}

/// Dock simgeleri; ilk ikisi çalışıyor sayılır (çizgi ya da nokta).
#[allow(clippy::too_many_arguments)]
fn icons<A: Clone>(p: &P<A>, x: f32, y: f32, icon: f32, gap: f32, sep: f32, vertical: bool, line: bool, labels: bool, s: &Style) {
    let colors = [(s.a, 1.0), (s.ink, 0.30), (s.b, 1.0), (s.ink, 0.30), (s.ink, 0.30)];
    let mut at = 0.0;
    for (i, (c, al)) in colors.into_iter().enumerate() {
        if i == 3 && sep > 0.0 {
            // Ayraç: iki grup arasında ince çizgi.
            let mid = at + (sep - gap) / 2.0 - 0.5;
            let line_r = if vertical { R::new(x, y + mid, icon, 1.0) } else { R::new(x + mid, y, 1.0, icon) };
            p.fill(line_r, s.ink, 0.25);
            at += sep;
        }
        let ir = if vertical { R::new(x, y + at, icon, icon) } else { R::new(x + at, y, icon, icon) };
        p.round(ir, icon * 0.28 * s.round.min(1.6), c, al);
        if labels {
            p.fill(R::new(ir.x + icon * 0.15, ir.bottom() + 1.8, icon * 0.7, 0.9), s.ink, 0.35);
        }
        if i < 2 {
            let mark = match (vertical, line) {
                (false, true) => R::new(ir.x + icon * 0.3, ir.bottom() + 1.0, icon * 0.4, 1.2),
                (false, false) => R::new(ir.x + icon / 2.0 - 0.8, ir.bottom() + 0.8, 1.6, 1.6),
                (true, true) => R::new(ir.x - 2.2, ir.y + icon * 0.3, 1.2, icon * 0.4),
                (true, false) => R::new(ir.x - 2.4, ir.y + icon / 2.0 - 0.8, 1.6, 1.6),
            };
            p.round(mark, 0.8, s.a, 0.95);
        }
        at += icon + gap;
    }
}

fn cursor<A: Clone>(p: &P<A>, r: R, look: &Look) {
    let c = &look.cursor;
    let color = parse_hex(&c.color).unwrap_or(0xFFFFFF);
    let k = 4.5 * c.scale.clamp(0.5, 1.6);
    let (cx, cy) = (r.x + r.w * 0.7, r.y + r.h * 0.42);
    let dot = R::new(cx - k, cy - k, k * 2.0, k * 2.0);
    if c.glow {
        p.round(dot.inset(-k * 0.9), k * 1.9, color, 0.22);
    }
    let edge = parse_hex(&c.edge).unwrap_or(if color_light(color) { 0x111111 } else { 0xFFFFFF });
    p.round(dot.inset(-1.0), k + 1.0, edge, 0.9);
    p.round(dot, k, color, 1.0);
}

/// Renk açık mı (kenar rengi "auto" iken karşıtı seçilir).
fn color_light(c: u32) -> bool {
    let (r, g, b) = ((c >> 16) & 0xFF, (c >> 8) & 0xFF, c & 0xFF);
    r * 299 + g * 587 + b * 114 > 150_000
}
