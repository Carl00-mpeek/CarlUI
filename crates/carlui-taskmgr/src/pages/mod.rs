//! Sekmeler. Her sekme `paint(app, painter, alan)` ile çizilir; tablolar ve
//! başlıklar buradaki ortak parçaları kullanır.

pub mod cpu;
pub mod disk;
pub mod gpu;
pub mod processes;
pub mod ram;
pub mod routing;
pub mod startup;

use crate::{
    draw::{Align, Painter, R, TEXT},
    Act,
};

pub const ROW_H: f32 = 30.0;
pub const HEAD_H: f32 = 30.0;

pub struct Col {
    pub title: &'static str,
    /// 0 = kalan genişliği paylaşır.
    pub w: f32,
    pub align: Align,
    pub sort: Option<Act>,
}

pub fn col(title: &'static str, w: f32, align: Align, sort: Option<Act>) -> Col {
    Col { title, w, align, sort }
}

/// Sayfa başlığı + sağında soluk alt bilgi.
pub fn title(p: &Painter, area: R, text: &str, sub: &str) {
    p.text(text, R::new(area.x, area.y, area.w, 34.0), 22.0, true, TEXT, 0.95, Align::Left);
    let tw = p.fonts.width(text, 22.0, true);
    p.text(sub, R::new(area.x + tw + 12.0, area.y + 4.0, area.w - tw - 12.0, 30.0), 13.0, false, TEXT, 0.55, Align::Left);
}

/// Bilgi kartı: küçük başlık + büyük değer + alt satır.
pub fn stat(p: &Painter, r: R, label: &str, value: &str, sub: &str, color: Option<u32>) {
    p.card(r);
    p.text(label, R::new(r.x + 14.0, r.y + 8.0, r.w - 28.0, 20.0), 12.0, false, TEXT, 0.6, Align::Left);
    p.text(value, R::new(r.x + 14.0, r.y + 28.0, r.w - 28.0, 30.0), 21.0, true, color.unwrap_or(TEXT), 0.95, Align::Left);
    if !sub.is_empty() {
        p.text(sub, R::new(r.x + 14.0, r.y + 58.0, r.w - 28.0, 18.0), 12.0, false, TEXT, 0.55, Align::Left);
    }
}

/// Kart dizisi: `n` kart yan yana, eşit genişlikte.
pub fn stat_row(area: R, y: f32, n: usize, h: f32) -> Vec<R> {
    let gap = 10.0;
    let w = (area.w - gap * (n as f32 - 1.0)) / n as f32;
    (0..n).map(|i| R::new(area.x + (w + gap) * i as f32, y, w, h)).collect()
}

pub struct Cell {
    pub text: String,
    pub color: u32,
    pub alpha: f32,
}

pub fn cell(text: impl Into<String>) -> Cell {
    Cell { text: text.into(), color: TEXT, alpha: 0.9 }
}

pub fn dim(text: impl Into<String>) -> Cell {
    Cell { text: text.into(), color: TEXT, alpha: 0.55 }
}

pub fn colored(text: impl Into<String>, color: u32) -> Cell {
    Cell { text: text.into(), color, alpha: 0.95 }
}

/// Kaydırılabilir tablo. `sorted`: (sütun, azalan mı). En fazla kaydırma döner.
#[allow(clippy::too_many_arguments)]
pub fn table(p: &mut Painter, area: R, cols: &[Col], sorted: Option<(usize, bool)>, rows: usize, scroll: f32, selected: Option<usize>, cell: impl Fn(usize, usize) -> Cell, row_act: impl Fn(usize) -> Act, scroll_act: Act) -> f32 {
    table_ex(p, area, cols, sorted, rows, scroll, selected, cell, row_act, scroll_act, |_| 1.0, |_, _, _| {})
}

/// `table` + satır başına yükseklik çarpanı (0..1; açılma animasyonunda alt
/// satırlar büyüyerek ve belirerek gelir) ve satırın üstüne çizim (`deco`:
/// ok, anahtar …; tıklama alanları satırınkinden sonra eklendiği için öncelikli).
#[allow(clippy::too_many_arguments)]
pub fn table_ex(
    p: &mut Painter,
    area: R,
    cols: &[Col],
    sorted: Option<(usize, bool)>,
    rows: usize,
    scroll: f32,
    selected: Option<usize>,
    cell: impl Fn(usize, usize) -> Cell,
    row_act: impl Fn(usize) -> Act,
    scroll_act: Act,
    height: impl Fn(usize) -> f32,
    mut deco: impl FnMut(&mut Painter, usize, R),
) -> f32 {
    let fixed: f32 = cols.iter().map(|c| c.w).sum();
    let flex = cols.iter().filter(|c| c.w == 0.0).count().max(1) as f32;
    let fw = ((area.w - fixed - 16.0) / flex).max(80.0);
    let widths: Vec<f32> = cols.iter().map(|c| if c.w == 0.0 { fw } else { c.w }).collect();
    p.card(area);
    // Başlık.
    let mut x = area.x + 8.0;
    for (i, c) in cols.iter().enumerate() {
        let r = R::new(x, area.y, widths[i], HEAD_H);
        if let Some(a) = &c.sort {
            if p.hover(r) {
                p.round(r.inset(3.0), 4.0, 0xFFFFFF, 0.06);
            }
            p.hit(r, a.clone());
        }
        let arrow = match sorted {
            Some((s, desc)) if s == i => if desc { " ↓" } else { " ↑" },
            _ => "",
        };
        p.text(&format!("{}{arrow}", c.title), r.inset(6.0).with_h(r.h, r.y), 12.0, true, TEXT, 0.65, c.align);
        x += widths[i];
    }
    p.fill(R::new(area.x + 1.0, area.y + HEAD_H, area.w - 2.0, 1.0), 0xFFFFFF, 0.08);
    let body = R::new(area.x, area.y + HEAD_H + 1.0, area.w, area.h - HEAD_H - 2.0);
    let hs: Vec<f32> = (0..rows).map(|r| height(r).clamp(0.0, 1.0) * ROW_H).collect();
    let total: f32 = hs.iter().sum();
    let max_scroll = (total - body.h).max(0.0);
    let scroll = scroll.clamp(0.0, max_scroll);
    p.hit(body, scroll_act);
    unsafe { p.rt.PushAxisAlignedClip(&body.d2d(), windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_ALIASED) };
    let mut y = body.y - scroll;
    for row in 0..rows {
        let h = hs[row];
        let top = y;
        y += h;
        if h < 0.5 || y < body.y || top > body.bottom() {
            continue;
        }
        let k = h / ROW_H;
        // Satır kendi yüksekliğine kırpılır: açılırken içerik yukarıdan kayarak gelir.
        let clip = R::new(area.x, top, area.w, h);
        unsafe { p.rt.PushAxisAlignedClip(&clip.d2d(), windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_ALIASED) };
        let ty = top - (ROW_H - h);
        let rr = R::new(area.x + 4.0, ty + 1.0, area.w - 8.0, ROW_H - 2.0);
        if selected == Some(row) {
            p.round(rr, 5.0, p.accent, 0.22 * k);
        } else if p.hover(clip) && body.contains(p.mouse) {
            p.round(rr, 5.0, 0xFFFFFF, 0.06 * k);
        }
        if k > 0.95 {
            p.hit(rr, row_act(row));
        }
        let mut x = area.x + 8.0;
        for (i, c) in cols.iter().enumerate() {
            let v = cell(row, i);
            p.text(&v.text, R::new(x + 6.0, ty, widths[i] - 12.0, ROW_H), 13.0, false, v.color, v.alpha * k, c.align);
            x += widths[i];
        }
        deco(p, row, R::new(area.x + 8.0, ty, widths.first().copied().unwrap_or(0.0), ROW_H));
        unsafe { p.rt.PopAxisAlignedClip() };
    }
    unsafe { p.rt.PopAxisAlignedClip() };
    // Kaydırma çubuğu.
    if max_scroll > 0.0 {
        let th = (body.h * body.h / total).max(30.0);
        let ty = body.y + (body.h - th) * (scroll / max_scroll);
        p.round(R::new(area.right() - 7.0, ty, 4.0, th), 2.0, 0xFFFFFF, 0.25);
    }
    max_scroll
}

trait WithH {
    fn with_h(self, h: f32, y: f32) -> R;
}

impl WithH for R {
    fn with_h(self, h: f32, y: f32) -> R {
        R::new(self.x, y, self.w, h)
    }
}
