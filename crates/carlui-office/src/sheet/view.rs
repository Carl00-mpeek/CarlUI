//! Bu modül "tablonun düzenlenmesi: hücre seçimi ve gezinme, hücre içi /
//! formül çubuğunda düzenleme, pano, geri al / yinele, biçim komutları ve
//! kaydetme" sorununu çözüyor. Çizim ve konum hesabı `grid.rs`te.

use super::{csv, formula, general, grid, xlsx, Book, Cell, Kind, Look, Undo, Val};
use crate::{
    app::{self, Act, Cmd, Keys, Mouse, View},
    clip,
    field::Field,
    rich::PAlign,
    ui,
};
use carlui_core::t;
use carlui_ui::{
    paint::{Painter, R},
};
use std::{collections::HashMap, path::Path};
use windows::Win32::{Graphics::Direct2D::ID2D1RenderTarget, UI::WindowsAndMessaging::GetCursorPos};

pub const MAX_R: u32 = 1_048_575;
pub const MAX_C: u32 = 16_383;

pub(super) struct Edit {
    pub field: Field,
    pub in_bar: bool,
    pub at: (u32, u32),
    /// Yazarak başladı: ok tuşları düzenlemeyi bitirip hücre değiştirir.
    pub typed: bool,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Drag {
    None,
    Cells,
    Resize { col: u32, x0: f32, w0: f32 },
    Field,
}

pub struct SheetView {
    pub(super) book: Book,
    pub(super) sh: usize,
    /// Etkin hücre.
    pub(super) act: (u32, u32),
    /// Seçimin öbür köşesi.
    pub(super) end: (u32, u32),
    pub(super) top: u32,
    pub(super) left: u32,
    pub(super) zoom: f32,
    pub(super) edit: Option<Edit>,
    pub(super) geo: grid::Geo,
    pub(super) undo: Vec<Undo>,
    pub(super) redo: Vec<Undo>,
    pub(super) dirty: bool,
    pub(super) drag: Drag,
    pub(super) rt: Option<ID2D1RenderTarget>,
}

pub(super) fn cursor() -> (i32, i32) {
    let mut p = Default::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

impl SheetView {
    pub fn new(book: Book) -> Self {
        SheetView { book, sh: 0, act: (0, 0), end: (0, 0), top: 0, left: 0, zoom: 1.0, edit: None, geo: Default::default(), undo: Vec::new(), redo: Vec::new(), dirty: false, drag: Drag::None, rt: None }
    }

    /// Seçim (sol üst, sağ alt).
    pub(super) fn sel(&self) -> ((u32, u32), (u32, u32)) {
        ((self.act.0.min(self.end.0), self.act.1.min(self.end.1)), (self.act.0.max(self.end.0), self.act.1.max(self.end.1)))
    }

    /// Seçimin dolu alanla sınırlanmışı (tüm sütun seçilince milyon satır dolaşılmasın).
    pub(super) fn sel_used(&self) -> ((u32, u32), (u32, u32)) {
        let (a, b) = self.sel();
        let (er, ec) = self.book.sheets[self.sh].extent();
        let r1 = b.0.min(er.max(a.0 + 1) - 1).max(a.0);
        let c1 = b.1.min(ec.max(a.1 + 1) - 1).max(a.1);
        (a, (r1, c1))
    }

    pub(super) fn push(&mut self, u: Undo) {
        self.undo.push(u);
        if self.undo.len() > 300 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }

    pub(super) fn ensure_visible(&mut self, (r, c): (u32, u32)) {
        if r < self.top {
            self.top = r;
        } else {
            let h = self.geo.grid.h;
            let mut sum = 0.0;
            let mut i = r;
            loop {
                sum += self.row_px(i);
                if sum > h || i == self.top {
                    break;
                }
                i -= 1;
            }
            if sum > h && i >= self.top {
                self.top = (i + 1).min(r);
            }
        }
        if c < self.left {
            self.left = c;
        } else {
            let w = self.geo.grid.w;
            let mut sum = 0.0;
            let mut i = c;
            loop {
                sum += self.col_px(i);
                if sum > w || i == self.left {
                    break;
                }
                i -= 1;
            }
            if sum > w && i >= self.left {
                self.left = (i + 1).min(c);
            }
        }
    }

    pub(super) fn go(&mut self, to: (u32, u32), extend: bool) {
        let to = (to.0.min(MAX_R), to.1.min(MAX_C));
        if extend {
            self.end = to;
        } else {
            self.act = to;
            self.end = to;
        }
        self.ensure_visible(to);
    }

    pub(super) fn filled(&self, r: u32, c: u32) -> bool {
        self.book.sheets[self.sh].get(r, c).is_some_and(|x| x.v != Val::Empty || x.f.is_some())
    }

    /// Ctrl+ok: dolu bloğun kenarına ya da sonraki dolu hücreye atla.
    pub(super) fn jump(&self, from: (u32, u32), dr: i64, dc: i64) -> (u32, u32) {
        let (er, ec) = self.book.sheets[self.sh].extent();
        let lim_r = if dr > 0 { MAX_R } else { 0 };
        let lim_c = if dc > 0 { MAX_C } else { 0 };
        let step = |p: (u32, u32)| -> Option<(u32, u32)> {
            let r = p.0 as i64 + dr;
            let c = p.1 as i64 + dc;
            if r < 0 || c < 0 || r > MAX_R as i64 || c > MAX_C as i64 {
                None
            } else {
                Some((r as u32, c as u32))
            }
        };
        let Some(next) = step(from) else { return from };
        let beyond = |p: (u32, u32)| (dr > 0 && p.0 >= er) || (dc > 0 && p.1 >= ec);
        if self.filled(from.0, from.1) && self.filled(next.0, next.1) {
            let mut p = next;
            while let Some(n) = step(p) {
                if !self.filled(n.0, n.1) {
                    break;
                }
                p = n;
            }
            return p;
        }
        let mut p = next;
        loop {
            if self.filled(p.0, p.1) {
                return p;
            }
            if beyond(p) {
                return (if dr != 0 { lim_r } else { p.0 }, if dc != 0 { lim_c } else { p.1 });
            }
            match step(p) {
                Some(n) => p = n,
                None => return p,
            }
        }
    }

    pub(super) fn start_edit(&mut self, text: Option<&str>, in_bar: bool) {
        let at = self.act;
        let t = match text {
            Some(t) => t.to_string(),
            None => self.book.edit_text(self.sh, at.0, at.1),
        };
        self.edit = Some(Edit { field: Field::new(&t), in_bar, at, typed: text.is_some() });
        app::caret_reset();
    }

    /// Düzenlemeyi hücreye yazar.
    pub(super) fn commit(&mut self) {
        let Some(e) = self.edit.take() else { return };
        let (r, c) = e.at;
        if e.field.text == self.book.edit_text(self.sh, r, c) {
            return;
        }
        let u = self.book.snapshot(self.sh, [(r, c)]);
        self.book.set_input(self.sh, r, c, e.field.text.trim_end());
        self.push(u);
    }

    /// Seçili hücrelerin stilini değiştirir.
    pub(super) fn restyle(&mut self, code: Option<&str>, f: impl Fn(&mut Look) + Copy) {
        self.commit();
        let (a, b) = self.sel_used();
        let keys: Vec<(u32, u32)> = (a.0..=b.0).flat_map(|r| (a.1..=b.1).map(move |c| (r, c))).collect();
        let u = self.book.snapshot(self.sh, keys.iter().copied());
        let mut map: HashMap<u32, u32> = HashMap::new();
        let mut changed = false;
        for (r, c) in keys {
            let cell = self.book.sheets[self.sh].get(r, c).cloned().unwrap_or_default();
            let ns = match map.get(&cell.s) {
                Some(s) => *s,
                None => {
                    let s = self.book.restyle(cell.s, code, f);
                    map.insert(cell.s, s);
                    s
                }
            };
            if ns != cell.s {
                self.book.put(self.sh, r, c, Cell { s: ns, raw: None, ..cell });
                changed = true;
            }
        }
        if changed {
            self.push(u);
        }
    }

    pub(super) fn act_look(&self) -> Look {
        self.book.sheets[self.sh].get(self.act.0, self.act.1).map(|c| self.book.look(c.s)).unwrap_or_default()
    }

    pub(super) fn copy(&self) -> bool {
        let (a, b) = self.sel_used();
        let text = self.book.copy_tsv(self.sh, a, b);
        clip::copy(app::hwnd(), &text)
    }

    pub(super) fn clear(&mut self) {
        let (a, b) = self.sel_used();
        let u = self.book.clear(self.sh, a, b);
        if !u.cells.is_empty() {
            self.push(u);
        }
    }

    pub(super) fn paste(&mut self) {
        let Some(text) = clip::paste(app::hwnd()) else { return };
        if text.is_empty() {
            return;
        }
        let (a, _) = self.sel();
        let u = self.book.paste_tsv(self.sh, a.0, a.1, &text);
        let rows = text.trim_end_matches(['\r', '\n']).split('\n').count() as u32;
        let cols = text.lines().map(|l| l.split('\t').count()).max().unwrap_or(1) as u32;
        self.push(u);
        self.act = a;
        self.end = (a.0 + rows.max(1) - 1, a.1 + cols.max(1) - 1);
    }

    pub(super) fn history(&mut self, redo: bool) {
        self.commit();
        let item = if redo { self.redo.pop() } else { self.undo.pop() };
        if let Some(u) = item {
            let sheet = u.sheet;
            let first = u.cells.first().map(|x| x.0);
            let back = self.book.restore(u);
            if redo {
                self.undo.push(back);
            } else {
                self.redo.push(back);
            }
            self.dirty = true;
            if sheet < self.book.sheets.len() {
                self.sh = sheet;
            }
            if let Some(k) = first {
                self.go(k, false);
            }
        }
    }

    pub(super) fn pick(&mut self, i: usize) {
        if i < self.book.sheets.len() && i != self.sh {
            self.commit();
            self.sh = i;
            self.act = (0, 0);
            self.end = (0, 0);
            self.top = 0;
            self.left = 0;
        }
    }

    pub(super) fn page_rows(&self) -> u32 {
        (self.geo.rows.len() as u32).saturating_sub(1).max(1)
    }
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    app::write_atomic(path, bytes)
}

impl View for SheetView {
    fn toolbar(&mut self, p: &mut Painter<Act>, r: R) {
        let look = self.act_look();
        let y = r.y;
        let mut x = r.x;
        x += ui::tool(p, x, y, '\u{E8DD}', Act::View(Cmd::Bold), look.bold);
        x += ui::tool(p, x, y, '\u{E8DB}', Act::View(Cmd::Italic), look.italic);
        x += ui::sep(p, x, y);
        x += ui::tool(p, x, y, '\u{E8E4}', Act::View(Cmd::Align(PAlign::Left)), look.align == 1);
        x += ui::tool(p, x, y, '\u{E8E3}', Act::View(Cmd::Align(PAlign::Center)), look.align == 2);
        x += ui::tool(p, x, y, '\u{E8E2}', Act::View(Cmd::Align(PAlign::Right)), look.align == 3);
        x += ui::sep(p, x, y);
        x += ui::tool(p, x, y, '\u{E790}', Act::View(Cmd::ColorMenu), false);
        x += ui::sep(p, x, y);
        let label = self.fmt_label();
        x += ui::dropdown(p, x, y, 124.0, label, Act::View(Cmd::More));
        x += ui::tool_text(p, x, y, "%", Act::View(Cmd::NumFmt(3)), false, true);
        let _ = ui::tool_text(p, x, y, ",00", Act::View(Cmd::NumFmt(1)), false, false);
    }

    fn paint(&mut self, p: &mut Painter<Act>, area: R) {
        if self.rt.is_none() {
            self.rt = Some(p.rt.clone());
        }
        self.paint_sheet(p, area);
    }

    fn status(&self) -> String {
        let (a, b) = self.sel_used();
        let mut out = String::new();
        if self.edit.is_some() {
            out.push_str(t!("Düzenle", "Edit"));
        } else if a != b {
            let (mut sum, mut nums, mut count) = (0.0, 0u32, 0u32);
            for ((_, c), cell) in self.book.sheets[self.sh].cells.range((a.0, 0)..=(b.0, u32::MAX)) {
                if *c < a.1 || *c > b.1 || cell.v == Val::Empty {
                    continue;
                }
                count += 1;
                if let Val::Num(n) = cell.v {
                    sum += n;
                    nums += 1;
                }
            }
            if nums > 0 {
                out.push_str(&format!("{}: {}    ", t!("Ortalama", "Average"), general(sum / nums as f64)));
            }
            out.push_str(&format!("{}: {count}", t!("Sayı", "Count")));
            if nums > 0 {
                out.push_str(&format!("    {}: {}", t!("Toplam", "Sum"), general(sum)));
            }
        } else {
            out.push_str(t!("Hazır", "Ready"));
        }
        if (self.zoom - 1.0).abs() > 0.01 {
            out.push_str(&format!("    {}%", (self.zoom * 100.0).round()));
        }
        out
    }

    fn cmd(&mut self, c: Cmd) {
        match c {
            Cmd::Bold => {
                let on = !self.act_look().bold;
                self.restyle(None, move |l| l.bold = on);
            }
            Cmd::Italic => {
                let on = !self.act_look().italic;
                self.restyle(None, move |l| l.italic = on);
            }
            Cmd::Align(a) => {
                let v = match a {
                    PAlign::Left => 1,
                    PAlign::Center => 2,
                    PAlign::Right => 3,
                    PAlign::Justify => 0,
                };
                let v = if self.act_look().align == v { 0 } else { v };
                self.restyle(None, move |l| l.align = v);
            }
            Cmd::Color(c) => self.restyle(None, move |l| l.fill = Some(c)),
            Cmd::ColorMenu => self.color_menu(),
            Cmd::NumFmt(n) => self.num_fmt(n),
            Cmd::More => self.fmt_menu(),
            Cmd::Undo => self.history(false),
            Cmd::Redo => self.history(true),
            Cmd::Zoom(d) => self.zoom = (self.zoom + d as f32 * 0.1).clamp(0.5, 2.5),
            Cmd::Pick(i) => self.pick(i),
            Cmd::Add => {
                self.commit();
                let i = self.book.add_sheet();
                self.dirty = true;
                self.pick(i);
            }
            _ => {}
        }
    }

    fn key(&mut self, vk: u16, k: &Keys) -> bool {
        if let Some(e) = self.edit.as_ref() {
            let (typed, in_bar, formula) = (e.typed, e.in_bar, e.field.text.starts_with('='));
            match vk {
                0x0D => {
                    self.commit();
                    let a = self.act;
                    self.go(if k.shift { (a.0.saturating_sub(1), a.1) } else { (a.0 + 1, a.1) }, false);
                }
                0x09 => {
                    self.commit();
                    let a = self.act;
                    self.go(if k.shift { (a.0, a.1.saturating_sub(1)) } else { (a.0, a.1 + 1) }, false);
                }
                0x1B => self.edit = None,
                0x25..=0x28 if typed && !in_bar && !formula => {
                    self.commit();
                    return self.key(vk, k);
                }
                0x26 | 0x28 => {}
                0x5A | 0x59 if k.ctrl => {
                    self.edit = None;
                    self.history(vk == 0x59);
                }
                _ => return self.edit.as_mut().is_some_and(|e| e.field.key(vk, k)),
            }
            return true;
        }
        let a = self.act;
        let from = if k.shift { self.end } else { self.act };
        match vk {
            0x25 => self.go(if k.ctrl { self.jump(from, 0, -1) } else { (from.0, from.1.saturating_sub(1)) }, k.shift),
            0x27 => self.go(if k.ctrl { self.jump(from, 0, 1) } else { (from.0, from.1 + 1) }, k.shift),
            0x26 => self.go(if k.ctrl { self.jump(from, -1, 0) } else { (from.0.saturating_sub(1), from.1) }, k.shift),
            0x28 => self.go(if k.ctrl { self.jump(from, 1, 0) } else { (from.0 + 1, from.1) }, k.shift),
            0x24 => self.go(if k.ctrl { (0, 0) } else { (from.0, 0) }, k.shift),
            0x23 if k.ctrl => {
                let (r, c) = self.book.sheets[self.sh].extent();
                self.go((r.saturating_sub(1), c.saturating_sub(1)), k.shift);
            }
            0x21 => {
                let n = self.page_rows();
                self.top = self.top.saturating_sub(n);
                self.go((from.0.saturating_sub(n), from.1), k.shift);
            }
            0x22 => {
                let n = self.page_rows();
                self.top = (self.top + n).min(MAX_R);
                self.go((from.0 + n, from.1), k.shift);
            }
            0x0D => self.go(if k.shift { (a.0.saturating_sub(1), a.1) } else { (a.0 + 1, a.1) }, false),
            0x09 => self.go(if k.shift { (a.0, a.1.saturating_sub(1)) } else { (a.0, a.1 + 1) }, false),
            0x71 => self.start_edit(None, false),
            0x2E => self.clear(),
            0x08 => self.start_edit(Some(""), false),
            0x41 if k.ctrl => {
                let (r, c) = self.book.sheets[self.sh].extent();
                self.act = (0, 0);
                self.end = (r.max(1) - 1, c.max(1) - 1);
            }
            0x43 if k.ctrl => {
                if self.copy() {
                    app::toast(t!("Kopyalandı", "Copied"));
                }
            }
            0x58 if k.ctrl => {
                if self.copy() {
                    self.clear();
                }
            }
            0x56 if k.ctrl => self.paste(),
            0x5A if k.ctrl => self.history(false),
            0x59 if k.ctrl => self.history(true),
            0x42 if k.ctrl => self.cmd(Cmd::Bold),
            0x49 if k.ctrl => self.cmd(Cmd::Italic),
            0xBB | 0x6B if k.ctrl => self.cmd(Cmd::Zoom(1)),
            0xBD | 0x6D if k.ctrl => self.cmd(Cmd::Zoom(-1)),
            _ => return false,
        }
        true
    }

    fn char(&mut self, ch: char) -> bool {
        match self.edit.as_mut() {
            Some(e) => e.field.insert(&ch.to_string()),
            None => {
                self.start_edit(Some(""), false);
                if let Some(e) = self.edit.as_mut() {
                    e.field.insert(&ch.to_string());
                }
            }
        }
        true
    }

    fn mouse(&mut self, m: Mouse, pt: (f32, f32), k: &Keys) {
        match m {
            Mouse::Down | Mouse::Double => self.press(m, pt, k),
            Mouse::Move => match self.drag {
                Drag::Cells => {
                    let g = &self.geo;
                    if pt.1 > g.grid.bottom() {
                        self.top = (self.top + 1).min(MAX_R);
                    } else if pt.1 < g.grid.y {
                        self.top = self.top.saturating_sub(1);
                    }
                    if pt.0 > g.grid.right() {
                        self.left = (self.left + 1).min(MAX_C);
                    } else if pt.0 < g.grid.x {
                        self.left = self.left.saturating_sub(1);
                    }
                    let to = self.cell_near(pt);
                    self.end = to;
                }
                Drag::Resize { col, x0, w0 } => {
                    let px = (w0 + (pt.0 - x0) / self.zoom).max(8.0);
                    let s = &mut self.book.sheets[self.sh];
                    s.col_w.insert(col, ((px - 5.0) / 7.0 * 100.0).round() / 100.0);
                    s.cols_dirty = true;
                    self.dirty = true;
                }
                Drag::Field => {
                    if let (Some(e), Some(rt)) = (self.edit.as_mut(), self.rt.as_ref()) {
                        e.field.click(rt, self.geo.bar, 13.0, pt.0, true);
                    }
                }
                Drag::None => {}
            },
            Mouse::Up => self.drag = Drag::None,
            Mouse::Right => {
                if let grid::Spot::Cell(r, c) = self.spot(pt) {
                    let (a, b) = self.sel();
                    if r < a.0 || r > b.0 || c < a.1 || c > b.1 {
                        self.commit();
                        self.go((r, c), false);
                    }
                }
                self.context_menu();
            }
        }
    }

    fn wheel(&mut self, d: f32, k: &Keys) {
        if k.ctrl {
            self.cmd(Cmd::Zoom(if d > 0.0 { 1 } else { -1 }));
            return;
        }
        let n = (d.abs() * 3.0).round().max(1.0) as u32;
        if k.shift {
            self.left = if d > 0.0 { self.left.saturating_sub(n.div_ceil(2)) } else { (self.left + n.div_ceil(2)).min(MAX_C) };
        } else {
            self.top = if d > 0.0 { self.top.saturating_sub(n) } else { (self.top + n).min(MAX_R) };
        }
    }

    fn dirty(&self) -> bool {
        self.dirty || self.edit.as_ref().is_some_and(|e| e.typed)
    }

    fn ext(&self) -> &'static str {
        match self.book.kind {
            Some(Kind::Csv) => "csv",
            _ => "xlsx",
        }
    }

    fn can_save(&self, ext: &str) -> bool {
        matches!(ext, "xlsx" | "xlsm" | "csv")
    }

    fn save(&mut self, path: &Path) -> Result<(), String> {
        self.commit();
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let bytes = if ext == "csv" { csv::save(&mut self.book) } else { xlsx::save(&mut self.book)? };
        write_file(path, &bytes)?;
        if ext != "csv" {
            self.book.kind = Some(Kind::Xlsx);
        }
        self.sh = self.sh.min(self.book.sheets.len().saturating_sub(1));
        self.dirty = false;
        Ok(())
    }

    fn reset_device(&mut self) {
        self.rt = None;
    }

    fn editing(&self) -> bool {
        self.edit.is_some()
    }
}

impl SheetView {
    pub(super) fn press(&mut self, m: Mouse, pt: (f32, f32), k: &Keys) {
        let spot = self.spot(pt);
        // Formül yazarken başka hücreye tıklamak o hücrenin adını ekler.
        if let (Some(e), grid::Spot::Cell(r, c)) = (self.edit.as_mut(), &spot) {
            let t = &e.field.text;
            if t.starts_with('=') && (*r, *c) != e.at && t.ends_with(['=', '(', ';', ',', '+', '-', '*', '/', ':', '&', '<', '>', '^']) {
                e.field.insert(&formula::cell_name(*r, *c));
                return;
            }
        }
        match spot {
            grid::Spot::Bar => {
                if self.edit.is_none() {
                    self.start_edit(None, true);
                }
                if let (Some(e), Some(rt)) = (self.edit.as_mut(), self.rt.as_ref()) {
                    e.in_bar = true;
                    e.typed = false;
                    e.field.click(rt, self.geo.bar, 13.0, pt.0, k.shift);
                    if m == Mouse::Double {
                        e.field.select_all();
                    }
                }
                self.drag = Drag::Field;
            }
            grid::Spot::Cell(r, c) => {
                let cr = self.cell_rect(r, c).unwrap_or_default();
                if let Some(e) = self.edit.as_mut() {
                    if e.at == (r, c) && !e.in_bar {
                        if let Some(rt) = self.rt.as_ref() {
                            e.field.click(rt, cr, self.zoom * 13.0, pt.0, k.shift);
                        }
                        return;
                    }
                }
                self.commit();
                self.go((r, c), k.shift);
                if m == Mouse::Double {
                    self.start_edit(None, false);
                } else {
                    self.drag = Drag::Cells;
                }
            }
            grid::Spot::Col(c) => {
                self.commit();
                if k.shift {
                    self.end = (MAX_R, c);
                    self.act.0 = 0;
                } else {
                    self.act = (0, c);
                    self.end = (MAX_R, c);
                }
            }
            grid::Spot::Row(r) => {
                self.commit();
                if k.shift {
                    self.end = (r, MAX_C);
                    self.act.1 = 0;
                } else {
                    self.act = (r, 0);
                    self.end = (r, MAX_C);
                }
            }
            grid::Spot::ColEdge(c) => {
                if m == Mouse::Double {
                    self.autofit(c);
                } else {
                    let w0 = self.col_px(c) / self.zoom;
                    self.drag = Drag::Resize { col: c, x0: pt.0, w0 };
                }
            }
            grid::Spot::Corner => {
                self.commit();
                self.act = (0, 0);
                self.end = (MAX_R, MAX_C);
            }
            grid::Spot::Outside => {}
        }
    }

    /// Sütun kenarına çift tık: en uzun yazıya göre genişlik (yaklaşık ölçü).
    pub(super) fn autofit(&mut self, c: u32) {
        let s = &self.book.sheets[self.sh];
        let longest = s.cells.iter().filter(|(k, _)| k.1 == c).map(|(k, _)| self.book.display(self.sh, k.0, k.1).chars().count()).max().unwrap_or(0);
        let w = (longest as f32 * 1.1 + 1.0).clamp(4.0, 80.0);
        let s = &mut self.book.sheets[self.sh];
        s.col_w.insert(c, (w * 100.0).round() / 100.0);
        s.cols_dirty = true;
        self.dirty = true;
    }
}
