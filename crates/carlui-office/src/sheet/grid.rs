//! Bu modül "tablonun ekrana çizilmesi: formül çubuğu, sütun/satır başlıkları,
//! hücreler (dolgu, yazı, taşma), seçim çerçevesi, hücre içi düzenleme ve sayfa
//! sekmeleri" sorununu çözüyor. Konum hesabı (`Geo`) her çizimde yenilenir ve
//! fare isabeti de aynı hesabı kullanır; böylece çizim ile tıklama hep uyuşur.

use super::{formula, view::SheetView, Val};
use crate::{
    app::{self, Act, Cmd},
    tl, ui,
};
use carlui_ui::paint::{mix, Align, Painter, R, TEXT};

pub const BAR_H: f32 = 36.0;
pub const TABS_H: f32 = 32.0;
pub const HEAD_H: f32 = 22.0;
/// Tablonun vurgu rengi (açık zeminde okunur yeşil).
pub const GREEN: u32 = 0x1E7B48;
const INK: u32 = 0x1F1F1F;
const GRID: u32 = 0xE0E0E0;
const HEAD_BG: u32 = 0xF4F4F4;

/// Son çizimdeki yerleşim.
#[derive(Clone, Default)]
pub struct Geo {
    pub name: R,
    pub bar: R,
    /// Başlıklar dahil tüm ızgara.
    pub head: R,
    /// Yalnızca hücre alanı.
    pub grid: R,
    pub head_w: f32,
    pub tabs: R,
    /// Görünen sütunlar: (sütun, x, genişlik).
    pub cols: Vec<(u32, f32, f32)>,
    pub rows: Vec<(u32, f32, f32)>,
}

pub enum Spot {
    Cell(u32, u32),
    Col(u32),
    /// Sütun kenarı: genişlik değiştirme.
    ColEdge(u32),
    Row(u32),
    Corner,
    Bar,
    Outside,
}

impl SheetView {
    pub(super) fn col_px(&self, c: u32) -> f32 {
        self.book.sheets[self.sh].col_w.get(&c).map(|w| w * 7.0 + 5.0).unwrap_or(64.0) * self.zoom
    }

    pub(super) fn row_px(&self, r: u32) -> f32 {
        self.book.sheets[self.sh].row_h.get(&r).map(|h| h * 4.0 / 3.0).unwrap_or(20.0).max(4.0) * self.zoom
    }

    pub(super) fn font(&self) -> f32 {
        13.0 * self.zoom
    }

    pub(super) fn layout(&mut self, area: R) {
        let mut g = Geo { bar: R::new(area.x + 8.0, area.y + 5.0, area.w - 16.0, BAR_H - 10.0), ..Default::default() };
        g.name = R::new(g.bar.x, g.bar.y, 96.0, g.bar.h);
        g.bar = R::new(g.bar.x + 96.0 + 34.0, g.bar.y, (g.bar.w - 130.0).max(40.0), g.bar.h);
        g.head = R::new(area.x, area.y + BAR_H, area.w, (area.h - BAR_H - TABS_H).max(0.0));
        // Satır numarası basamağı arttıkça başlık genişler.
        let digits = format!("{}", self.top + 60).len() as f32;
        g.head_w = (digits * 8.0 + 16.0).max(42.0);
        g.grid = R::new(g.head.x + g.head_w, g.head.y + HEAD_H, (g.head.w - g.head_w).max(0.0), (g.head.h - HEAD_H).max(0.0));
        let mut x = g.grid.x;
        let mut c = self.left;
        while x < g.grid.right() && c <= super::view::MAX_C {
            let w = self.col_px(c);
            g.cols.push((c, x, w));
            x += w;
            c += 1;
        }
        let mut y = g.grid.y;
        let mut r = self.top;
        while y < g.grid.bottom() && r <= super::view::MAX_R {
            let h = self.row_px(r);
            g.rows.push((r, y, h));
            y += h;
            r += 1;
        }
        g.tabs = R::new(area.x, g.head.bottom(), area.w, TABS_H);
        self.geo = g;
    }

    /// Noktanın üstündeki öğe.
    pub(super) fn spot(&self, pt: (f32, f32)) -> Spot {
        let g = &self.geo;
        if g.bar.contains(pt) {
            return Spot::Bar;
        }
        if !g.head.contains(pt) {
            return Spot::Outside;
        }
        let in_head_row = pt.1 < g.grid.y;
        let in_head_col = pt.0 < g.grid.x;
        if in_head_row && in_head_col {
            return Spot::Corner;
        }
        let (r, c) = self.cell_near(pt);
        if in_head_row {
            if let Some((c, _, _)) = g.cols.iter().find(|(_, x, w)| (pt.0 - (x + w)).abs() <= 4.0) {
                return Spot::ColEdge(*c);
            }
            return Spot::Col(c);
        }
        if in_head_col {
            return Spot::Row(r);
        }
        Spot::Cell(r, c)
    }

    /// Noktaya en yakın görünen hücre (ızgara dışındaysa kenardaki).
    pub(super) fn cell_near(&self, pt: (f32, f32)) -> (u32, u32) {
        let g = &self.geo;
        let c = g.cols.iter().find(|(_, x, w)| pt.0 < x + w).or(g.cols.last()).map(|c| c.0).unwrap_or(self.left);
        let r = g.rows.iter().find(|(_, y, h)| pt.1 < y + h).or(g.rows.last()).map(|r| r.0).unwrap_or(self.top);
        let c = if pt.0 < g.grid.x { self.left } else { c };
        let r = if pt.1 < g.grid.y { self.top } else { r };
        (r, c)
    }

    pub(super) fn cell_rect(&self, r: u32, c: u32) -> Option<R> {
        let (_, x, w) = self.geo.cols.iter().find(|x| x.0 == c)?;
        let (_, y, h) = self.geo.rows.iter().find(|x| x.0 == r)?;
        Some(R::new(*x, *y, *w, *h))
    }

    pub(super) fn paint_sheet(&mut self, p: &mut Painter<Act>, area: R) {
        self.layout(area);
        self.paint_bar(p);
        let g = self.geo.clone();
        // Tablo her iki temada da beyaz kâğıttır; renkleri gerçek kalır.
        carlui_ui::tone::literal(|| {
            p.fill(g.head, 0xFFFFFF, 1.0);
            unsafe {
                p.rt.PushAxisAlignedClip(&g.grid.d2d(), windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_ALIASED);
            }
            self.paint_cells(p, &g);
            self.paint_selection(p, &g);
            unsafe {
                p.rt.PopAxisAlignedClip();
            }
            self.paint_headers(p, &g);
            self.paint_editor(p);
        });
        self.paint_tabs(p, &g);
    }

    fn paint_bar(&mut self, p: &mut Painter<Act>) {
        let g = self.geo.clone();
        p.round(g.name, 5.0, 0xFFFFFF, 0.07);
        let (a, b) = self.sel();
        let name = if a == b { formula::cell_name(a.0, a.1) } else { format!("{}:{}", formula::cell_name(a.0, a.1), formula::cell_name(b.0, b.1)) };
        p.text(&name, g.name.inset(0.0), 12.5, false, TEXT, 0.9, Align::Center);
        p.text("fx", R::new(g.name.right() + 4.0, g.bar.y, 26.0, g.bar.h), 13.0, true, TEXT, 0.55, Align::Center);
        p.round(g.bar, 5.0, 0xFFFFFF, 0.07);
        let caret = app::caret_on();
        match self.edit.as_mut() {
            Some(e) if e.in_bar => e.field.draw(p, g.bar, 13.0, TEXT, caret),
            Some(e) => {
                let mut f = e.field.clone();
                f.draw(p, g.bar, 13.0, TEXT, false);
            }
            None => {
                let t = self.book.edit_text(self.sh, self.act.0, self.act.1);
                let mut f = crate::field::Field::new(&t);
                f.caret = 0;
                f.anchor = 0;
                f.draw(p, g.bar, 13.0, TEXT, false);
            }
        }
    }

    fn paint_cells(&self, p: &mut Painter<Act>, g: &Geo) {
        let sheet = &self.book.sheets[self.sh];
        let size = self.font();
        // Dolgular.
        for (r, y, h) in &g.rows {
            for (c, x, w) in &g.cols {
                if let Some(cell) = sheet.get(*r, *c) {
                    if let Some(f) = self.book.look(cell.s).fill {
                        p.fill(R::new(*x, *y, *w, *h), f, 1.0);
                    }
                }
            }
        }
        // Izgara çizgileri.
        for (_, x, w) in &g.cols {
            p.fill(R::new(x + w - 1.0, g.grid.y, 1.0, g.grid.h), GRID, 1.0);
        }
        for (_, y, h) in &g.rows {
            p.fill(R::new(g.grid.x, y + h - 1.0, g.grid.w, 1.0), GRID, 1.0);
        }
        // Yazılar; sola yaslı uzun yazı boş komşu hücrelere taşar.
        for (r, y, h) in &g.rows {
            for (i, (c, x, w)) in g.cols.iter().enumerate() {
                let Some(cell) = sheet.get(*r, *c) else { continue };
                if cell.v == Val::Empty {
                    continue;
                }
                let look = self.book.look(cell.s);
                let mut s = self.book.display(self.sh, *r, *c);
                if s.is_empty() {
                    continue;
                }
                let align = match look.align {
                    1 => Align::Left,
                    2 => Align::Center,
                    3 => Align::Right,
                    _ => match cell.v {
                        Val::Num(_) => Align::Right,
                        Val::Bool(_) | Val::Err(_) => Align::Center,
                        _ => Align::Left,
                    },
                };
                let tw = p.fonts.width(&s, size, look.bold);
                let mut rr = R::new(x + 3.0, *y, w - 6.0, h - 1.0);
                if tw > rr.w {
                    if matches!(cell.v, Val::Num(_)) {
                        s = "#".repeat(((w - 6.0) / p.fonts.width("#", size, false).max(1.0)).floor().max(1.0) as usize);
                    } else if matches!(align, Align::Left) && !look.wrap {
                        let mut extra = 0.0;
                        for (c2, _, w2) in g.cols.iter().skip(i + 1) {
                            if tw <= rr.w + extra || sheet.get(*r, *c2).is_some_and(|x| x.v != Val::Empty) {
                                break;
                            }
                            extra += w2;
                        }
                        if extra > 0.0 {
                            p.fill(R::new(x + w - 1.0, *y, extra, h - 1.0), 0xFFFFFF, 1.0);
                            rr.w += extra;
                        }
                    }
                }
                let color = look.color.unwrap_or(INK);
                if look.italic {
                    let st = tl::Style { family: "Segoe UI", base: crate::rich::Fmt { size: Some(size), b: Some(look.bold), i: Some(true), ..Default::default() }, px_per_pt: 1.0, align: crate::rich::PAlign::Left, width: 100_000.0, line: 1.0 };
                    if let Some(l) = tl::plain(p.rt, &s, &st) {
                        let lx = match align {
                            Align::Left => rr.x,
                            Align::Center => rr.x + (rr.w - l.width) / 2.0,
                            Align::Right => rr.right() - l.width,
                        };
                        unsafe {
                            p.rt.PushAxisAlignedClip(&rr.d2d(), windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_ALIASED);
                        }
                        l.draw(p.rt, lx, rr.y + (rr.h - l.height) / 2.0, color, 1.0);
                        unsafe {
                            p.rt.PopAxisAlignedClip();
                        }
                    }
                } else {
                    p.text(&s, rr, size, look.bold, color, 1.0, align);
                }
            }
        }
    }

    fn paint_selection(&self, p: &mut Painter<Act>, g: &Geo) {
        let (a, b) = self.sel();
        let vis_c: Vec<_> = g.cols.iter().filter(|c| c.0 >= a.1 && c.0 <= b.1).collect();
        let vis_r: Vec<_> = g.rows.iter().filter(|r| r.0 >= a.0 && r.0 <= b.0).collect();
        let x0 = if a.1 < self.left { g.grid.x - 3.0 } else { vis_c.first().map(|c| c.1).unwrap_or(f32::NAN) };
        let x1 = vis_c.last().map(|c| c.1 + c.2).unwrap_or(f32::NAN);
        let y0 = if a.0 < self.top { g.grid.y - 3.0 } else { vis_r.first().map(|r| r.1).unwrap_or(f32::NAN) };
        let y1 = vis_r.last().map(|r| r.1 + r.2).unwrap_or(f32::NAN);
        if x0.is_nan() || x1.is_nan() || y0.is_nan() || y1.is_nan() {
            return;
        }
        let sel = R::new(x0 - 1.0, y0 - 1.0, x1 - x0 + 1.0, y1 - y0 + 1.0);
        if a != b {
            p.fill(sel, GREEN, 0.12);
            if let Some(cr) = self.cell_rect(self.act.0, self.act.1) {
                p.fill(R::new(cr.x, cr.y, cr.w - 1.0, cr.h - 1.0), 0xFFFFFF, 1.0);
                self.redraw_one(p, cr);
            }
        }
        // 2 piksellik çerçeve ve sağ alttaki küçük kare.
        p.fill(R::new(sel.x - 1.0, sel.y - 1.0, sel.w + 2.0, 2.0), GREEN, 1.0);
        p.fill(R::new(sel.x - 1.0, sel.bottom() - 1.0, sel.w + 2.0, 2.0), GREEN, 1.0);
        p.fill(R::new(sel.x - 1.0, sel.y - 1.0, 2.0, sel.h + 2.0), GREEN, 1.0);
        p.fill(R::new(sel.right() - 1.0, sel.y - 1.0, 2.0, sel.h + 2.0), GREEN, 1.0);
        p.fill(R::new(sel.right() - 3.0, sel.bottom() - 3.0, 6.0, 6.0), GREEN, 1.0);
    }

    /// Seçimin içindeki etkin hücre beyaz görünür: dolgusu ve yazısı yeniden.
    fn redraw_one(&self, p: &mut Painter<Act>, cr: R) {
        let Some(cell) = self.book.sheets[self.sh].get(self.act.0, self.act.1) else { return };
        let look = self.book.look(cell.s);
        if let Some(f) = look.fill {
            p.fill(R::new(cr.x, cr.y, cr.w - 1.0, cr.h - 1.0), f, 1.0);
        }
        let s = self.book.display(self.sh, self.act.0, self.act.1);
        let align = match (look.align, &cell.v) {
            (2, _) => Align::Center,
            (3, _) | (0, Val::Num(_)) => Align::Right,
            _ => Align::Left,
        };
        p.text(&s, R::new(cr.x + 3.0, cr.y, cr.w - 6.0, cr.h - 1.0), self.font(), look.bold, look.color.unwrap_or(INK), 1.0, align);
    }

    fn paint_headers(&self, p: &mut Painter<Act>, g: &Geo) {
        let (a, b) = self.sel();
        let hs = (11.5 * self.zoom).clamp(10.0, 15.0);
        let top = R::new(g.grid.x, g.head.y, g.grid.w, HEAD_H);
        p.fill(top, HEAD_BG, 1.0);
        unsafe {
            p.rt.PushAxisAlignedClip(&top.d2d(), windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_ALIASED);
        }
        for (c, x, w) in &g.cols {
            let on = *c >= a.1 && *c <= b.1;
            let r = R::new(*x, g.head.y, *w, HEAD_H);
            if on {
                p.fill(r, mix(HEAD_BG, GREEN, 0.18), 1.0);
                p.fill(R::new(*x, r.bottom() - 2.0, *w, 2.0), GREEN, 1.0);
            }
            p.fill(R::new(x + w - 1.0, r.y + 3.0, 1.0, r.h - 6.0), 0xC8C8C8, 1.0);
            p.text(&formula::col_name(*c), r, hs, on, if on { GREEN } else { 0x404040 }, 1.0, Align::Center);
        }
        unsafe {
            p.rt.PopAxisAlignedClip();
        }
        let left = R::new(g.head.x, g.grid.y, g.head_w, g.grid.h);
        p.fill(left, HEAD_BG, 1.0);
        unsafe {
            p.rt.PushAxisAlignedClip(&left.d2d(), windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_ALIASED);
        }
        for (r, y, h) in &g.rows {
            let on = *r >= a.0 && *r <= b.0;
            let rr = R::new(g.head.x, *y, g.head_w, *h);
            if on {
                p.fill(rr, mix(HEAD_BG, GREEN, 0.18), 1.0);
                p.fill(R::new(rr.right() - 2.0, *y, 2.0, *h), GREEN, 1.0);
            }
            p.fill(R::new(rr.x + 3.0, y + h - 1.0, rr.w - 6.0, 1.0), 0xC8C8C8, 1.0);
            p.text(&format!("{}", r + 1), rr, hs, on, if on { GREEN } else { 0x404040 }, 1.0, Align::Center);
        }
        unsafe {
            p.rt.PopAxisAlignedClip();
        }
        let corner = R::new(g.head.x, g.head.y, g.head_w, HEAD_H);
        p.fill(corner, mix(HEAD_BG, 0x000000, 0.04), 1.0);
        p.fill(R::new(g.head.x, g.grid.y - 1.0, g.head.w, 1.0), 0xC8C8C8, 1.0);
        p.fill(R::new(g.grid.x - 1.0, g.head.y, 1.0, g.head.h), 0xC8C8C8, 1.0);
    }

    fn paint_editor(&mut self, p: &mut Painter<Act>) {
        let size = self.font();
        let Some(e) = self.edit.as_ref() else { return };
        if e.in_bar {
            return;
        }
        let Some(cr) = self.cell_rect(e.at.0, e.at.1) else { return };
        let tw = p.fonts.width(&e.field.text, size, false) + 14.0;
        let w = cr.w.max(tw).min(self.geo.grid.right() - cr.x);
        let r = R::new(cr.x, cr.y, w, cr.h);
        p.fill(r, 0xFFFFFF, 1.0);
        p.fill(R::new(r.x - 1.0, r.y - 1.0, r.w + 1.0, 2.0), GREEN, 1.0);
        p.fill(R::new(r.x - 1.0, r.bottom() - 1.0, r.w + 1.0, 2.0), GREEN, 1.0);
        p.fill(R::new(r.x - 1.0, r.y - 1.0, 2.0, r.h + 1.0), GREEN, 1.0);
        p.fill(R::new(r.right() - 1.0, r.y - 1.0, 2.0, r.h + 1.0), GREEN, 1.0);
        let caret = app::caret_on();
        if let Some(e) = self.edit.as_mut() {
            e.field.draw(p, R::new(r.x, r.y, r.w - 2.0, r.h), size, INK, caret);
        }
    }

    fn paint_tabs(&self, p: &mut Painter<Act>, g: &Geo) {
        let mut x = g.tabs.x + 8.0;
        let y = g.tabs.y + 3.0;
        for (i, s) in self.book.sheets.iter().enumerate() {
            let w = p.fonts.width(&s.name, 12.5, i == self.sh) + 26.0;
            let r = R::new(x, y, w, TABS_H - 6.0);
            if r.right() > g.tabs.right() - 44.0 {
                break;
            }
            if i == self.sh {
                p.round(r, 5.0, 0xFFFFFF, 0.13);
                p.fill(R::new(r.x + 8.0, r.bottom() - 2.0, r.w - 16.0, 2.0), mix(GREEN, 0xFFFFFF, 0.35), 1.0);
            } else if p.hover(r) {
                p.round(r, 5.0, 0xFFFFFF, 0.07);
            }
            p.text(&s.name, r, 12.5, i == self.sh, TEXT, if i == self.sh { 0.95 } else { 0.7 }, Align::Center);
            p.hit(r, Act::View(Cmd::Pick(i)));
            x += w + 4.0;
        }
        ui::tool(p, x, g.tabs.y + 1.0, '\u{E710}', Act::View(Cmd::Add), false);
    }
}
