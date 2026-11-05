//! Bu modül "Word belgesini sayfa sayfa gösterip üzerinde yazalım" sorununu
//! çözüyor: düzen önbelleği (yalnızca değişen paragraf yeniden dizilir),
//! sayfaların çizimi, imleç ve seçim, fare, kaydırma ve yakınlaştırma.
//! Biçim komutları ve geri al `edit.rs`te.

use super::{
    layout::{self, Geo, Lay, GAP},
    save, Document,
};
use crate::{
    app::{self, Act, Cmd, Keys, Mouse, View},
    img,
    rich::{self, Cursor, Para, Pos},
    richedit::{self, Editor, Nav, Out},
};
use carlui_core::t;
use carlui_ui::paint::{Painter, R};
use std::{cell::Cell, path::Path};
use windows::Win32::Graphics::Direct2D::{ID2D1RenderTarget, D2D1_ANTIALIAS_MODE_ALIASED};

pub(super) struct Snap {
    pub paras: Vec<Para>,
    pub cur: Cursor,
}

/// Geri al kayıtlarını birleştirmek için son düzenlemenin türü.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Last {
    Other,
    Typing,
    Deleting,
}

pub struct DocView {
    pub(super) d: Document,
    pub(super) ed: Editor,
    pub(super) lays: Vec<Option<Lay>>,
    /// Paragrafların mutlak (belge) üst y'si ve sayfası.
    pub(super) tops: Vec<f32>,
    pub(super) pages_of: Vec<usize>,
    pub(super) pages: usize,
    pub(super) zoom: f32,
    pub(super) scroll: f32,
    pub(super) area: R,
    /// Sayfanın ekrandaki sol kenarı.
    pub(super) px: f32,
    pub(super) rt: Option<ID2D1RenderTarget>,
    pub(super) imgs: img::Cache,
    pub(super) undo: Vec<Snap>,
    pub(super) redo: Vec<Snap>,
    pub(super) last: Last,
    pub(super) dirty: bool,
    pub(super) drag: bool,
    /// Bir sonraki çizimde imleci görünür alana getir.
    pub(super) reveal: bool,
    pub(super) words: Cell<Option<usize>>,
}

/// Düzen bilgisinden imleç hesabı (richedit'e verilir).
pub(super) struct DocNav<'a> {
    pub lays: &'a [Option<Lay>],
    pub tops: &'a [f32],
    pub g: Geo,
    pub page: f32,
}

impl DocNav<'_> {
    fn lay(&self, p: usize) -> Option<&Lay> {
        self.lays.get(p).and_then(Option::as_ref)
    }
}

impl Nav for DocNav<'_> {
    fn caret(&self, at: Pos) -> Option<(f32, f32, f32)> {
        let l = self.lay(at.p)?;
        let top = *self.tops.get(at.p)?;
        let x = self.g.left + l.indent;
        match &l.laid {
            Some(t) if l.images.is_empty() => {
                let (cx, cy, h) = t.caret(at.c);
                Some((x + cx, top + cy, h))
            }
            _ => {
                let w: f32 = if at.c > 0 { l.images.iter().map(|i| i.1).fold(0.0, f32::max) } else { 0.0 };
                Some((x + w, top, l.h.min(40.0 * self.g.s)))
            }
        }
    }

    fn hit(&self, x: f32, y: f32) -> Option<Pos> {
        if self.tops.is_empty() {
            return None;
        }
        let i = self.tops.partition_point(|t| *t <= y).saturating_sub(1);
        let l = self.lay(i)?;
        let lx = x - self.g.left - l.indent;
        let c = match &l.laid {
            Some(t) if l.images.is_empty() => t.hit(lx, y - self.tops[i]),
            _ => {
                if lx > l.images.first().map(|im| im.1 / 2.0).unwrap_or(0.0) {
                    l.images.len()
                } else {
                    0
                }
            }
        };
        Some(Pos { p: i, c })
    }

    fn line_bounds(&self, at: Pos) -> (usize, usize) {
        match self.lay(at.p).and_then(|l| l.laid.as_ref()) {
            Some(t) => t.line_bounds(at.c),
            None => (0, at.c),
        }
    }

    fn locked(&self, p: usize) -> bool {
        self.lay(p).is_none_or(|l| l.block)
    }

    fn page(&self) -> f32 {
        self.page
    }
}

impl DocView {
    pub fn new(d: Document) -> Self {
        let n = d.paras.len();
        let mut v = DocView {
            d,
            ed: Editor::default(),
            lays: Vec::new(),
            tops: Vec::new(),
            pages_of: Vec::new(),
            pages: 1,
            zoom: 1.0,
            scroll: 0.0,
            area: R::new(0.0, 0.0, 800.0, 600.0),
            px: 0.0,
            rt: None,
            imgs: img::Cache::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            last: Last::Other,
            dirty: false,
            drag: false,
            reveal: false,
            words: Cell::new(None),
        };
        v.lays.resize_with(n, || None);
        // İmleç ilk yazılabilir paragrafta başlar (belge tabloyla başlayabilir).
        let first = v.d.paras.iter().position(|p| p.block.is_none()).unwrap_or(0);
        v.ed.cur.set(Pos { p: first, c: 0 }, false);
        v
    }

    pub(super) fn geo(&self) -> Geo {
        Geo::new(&self.d, self.zoom)
    }

    /// Eksik düzenleri kurar, sayfalara yerleştirir.
    pub(super) fn relayout(&mut self) {
        let Some(rt) = self.rt.clone() else { return };
        let g = self.geo();
        self.lays.resize_with(self.d.paras.len(), || None);
        for (i, p) in self.d.paras.iter().enumerate() {
            let stale = self.lays[i].as_ref().is_none_or(|l| l.bullet != p.bullet);
            if stale {
                self.lays[i] = Some(layout::lay_para(&rt, &self.d, p, &g));
            }
        }
        let (places, pages) = layout::paginate(self.lays.iter().flatten(), &g);
        self.pages = pages;
        self.tops = places.iter().map(|(pg, y)| g.page_y(*pg) + g.top + y).collect();
        self.pages_of = places.into_iter().map(|(pg, _)| pg).collect();
    }

    pub(super) fn nav(&self) -> DocNav<'_> {
        DocNav { lays: &self.lays, tops: &self.tops, g: self.geo(), page: (self.area.h - 40.0).max(100.0) }
    }

    /// Paragraf düzenlerini (lo..=hi, eski dizinler) sıfırlar; paragraf sayısı `n0`dan bugüne değişti.
    pub(super) fn invalidate(&mut self, lo: usize, hi: usize, n0: usize) {
        let n1 = self.d.paras.len();
        let hi = hi.min(self.lays.len().saturating_sub(1));
        if lo >= self.lays.len() || lo > hi {
            self.lays.clear();
        } else {
            let removed = hi - lo + 1;
            let inserted = (removed + n1).saturating_sub(n0);
            self.lays.splice(lo..=hi, std::iter::repeat_with(|| None).take(inserted));
        }
        self.lays.resize_with(n1, || None);
    }

    pub(super) fn invalidate_all(&mut self) {
        self.lays.clear();
        self.lays.resize_with(self.d.paras.len(), || None);
    }

    /// Metin değişti: düzen, numara, kirli bayrağı.
    pub(super) fn changed(&mut self, lo: usize, hi: usize, n0: usize) {
        self.invalidate(lo, hi, n0);
        self.d.renumber();
        self.dirty = true;
        self.reveal = true;
        self.words.set(None);
        self.redo.clear();
    }

    /// Seçili paragrafların, bitişikleri dahil, aralığı (birleşen paragraflar için).
    pub(super) fn span(&self) -> (usize, usize, usize) {
        let r = rich::sel_paras(&self.ed.cur);
        let n = self.d.paras.len();
        (r.start().saturating_sub(1), (*r.end() + 1).min(n.saturating_sub(1)), n)
    }

    /// Düzenleme tuşuyla metni değiştir; undo kaydını birleştirir.
    fn edit_key(&mut self, vk: u16, k: &Keys) -> Out {
        self.relayout();
        let editing = matches!(vk, 0x08 | 0x2E | 0x0D) || (vk == 0x09 && !k.ctrl) || (k.ctrl && matches!(vk, 0x58 | 0x56));
        let kind = if matches!(vk, 0x08 | 0x2E) && !k.ctrl { Last::Deleting } else { Last::Other };
        let (lo, hi, n0) = self.span();
        let snap = editing.then(|| Snap { paras: self.d.paras.clone(), cur: self.ed.cur.clone() });
        let nav = DocNav { lays: &self.lays, tops: &self.tops, g: Geo::new(&self.d, self.zoom), page: (self.area.h - 40.0).max(100.0) };
        let out = richedit::key(&mut self.d.paras, &mut self.ed, vk, k, &nav);
        match out {
            Out::Changed => {
                if let Some(s) = snap {
                    if kind == Last::Other || self.last != kind {
                        self.push(s);
                    }
                }
                self.last = kind;
                self.changed(lo, hi, n0);
            }
            Out::Moved => {
                self.last = Last::Other;
                self.reveal = true;
            }
            Out::No => {}
        }
        out
    }

    /// Ekrandaki noktanın belge konumu (tabloya düşerse en yakın paragraf).
    fn pos_at(&self, pt: (f32, f32)) -> Option<Pos> {
        let nav = self.nav();
        let (x, y) = (pt.0 - self.px, pt.1 - self.area.y + self.scroll);
        let mut at = nav.hit(x, y)?;
        if nav.locked(at.p) {
            let down = (at.p + 1..self.d.paras.len()).find(|&i| !nav.locked(i));
            let up = (0..at.p).rev().find(|&i| !nav.locked(i));
            at = match (down, up) {
                (Some(d), _) => Pos { p: d, c: 0 },
                (None, Some(u)) => Pos { p: u, c: self.d.paras[u].len() },
                _ => return None,
            };
        }
        Some(at)
    }

    fn total_h(&self) -> f32 {
        let g = self.geo();
        g.page_y(self.pages) + GAP
    }

    fn clamp_scroll(&mut self) {
        let max = (self.total_h() - self.area.h).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max);
    }

    fn reveal_caret(&mut self) {
        let Some((_, y, h)) = self.nav().caret(self.ed.cur.at) else { return };
        let pad = 24.0;
        if y - pad < self.scroll {
            self.scroll = y - pad;
        } else if y + h + pad > self.scroll + self.area.h {
            self.scroll = y + h + pad - self.area.h;
        }
    }

    fn word_count(&self) -> usize {
        if let Some(n) = self.words.get() {
            return n;
        }
        let n = self.d.paras.iter().map(|p| p.runs.iter().map(|r| r.text.as_str()).collect::<String>().split_whitespace().count()).sum();
        self.words.set(Some(n));
        n
    }

    fn paint_pages(&mut self, p: &mut Painter<Act>, area: R) {
        let g = self.geo();
        self.px = area.x + ((area.w - g.page_w) / 2.0).max(16.0);
        let top = area.y - self.scroll;
        unsafe {
            p.rt.PushAxisAlignedClip(&area.d2d(), D2D1_ANTIALIAS_MODE_ALIASED);
        }
        for pg in 0..self.pages {
            let r = R::new(self.px, top + g.page_y(pg), g.page_w, g.page_h);
            if r.bottom() < area.y || r.y > area.bottom() {
                continue;
            }
            p.fill(R::new(r.x + 2.0, r.y + 3.0, r.w, r.h), 0x000000, 0.35);
            p.fill(r, 0xFFFFFF, 1.0);
        }
        let (a, b) = self.ed.cur.range();
        let sel = a != b;
        for (i, para) in self.d.paras.iter().enumerate() {
            let (Some(Some(l)), Some(&y)) = (self.lays.get(i), self.tops.get(i)) else { continue };
            let sy = top + y;
            if sy > area.bottom() {
                break;
            }
            if sy + l.h < area.y {
                continue;
            }
            let range = (sel && i >= a.p && i <= b.p).then(|| (if i == a.p { a.c } else { 0 }, if i == b.p { b.c } else { para.len() + 1 }));
            layout::draw(p, &self.d, para, l, &g, (self.px + g.left, sy), range, &mut self.imgs);
        }
        if app::caret_on() && !sel {
            if let Some((x, y, h)) = self.nav().caret(self.ed.cur.at) {
                p.fill(R::new(self.px + x, top + y, (1.5 * self.zoom).max(1.0), h), 0x000000, 1.0);
            }
        }
        unsafe {
            p.rt.PopAxisAlignedClip();
        }
        // Kaydırma çubuğu.
        let total = self.total_h();
        if total > area.h {
            let h = (area.h * area.h / total).max(30.0);
            let y = area.y + (area.h - h) * (self.scroll / (total - area.h));
            p.round(R::new(area.right() - 7.0, y, 4.0, h), 2.0, 0xFFFFFF, 0.28);
        }
    }
}

impl View for DocView {
    fn toolbar(&mut self, p: &mut Painter<Act>, r: R) {
        self.toolbar_ui(p, r);
    }

    fn paint(&mut self, p: &mut Painter<Act>, area: R) {
        if self.rt.is_none() {
            self.rt = Some(p.rt.clone());
        }
        self.area = area;
        self.relayout();
        if self.reveal {
            self.reveal_caret();
            self.reveal = false;
        }
        self.clamp_scroll();
        self.paint_pages(p, area);
    }

    fn status(&self) -> String {
        let page = self.pages_of.get(self.ed.cur.at.p).copied().unwrap_or(0) + 1;
        format!("{} {page} / {} · {} {} · %{}", t!("Sayfa", "Page"), self.pages, self.word_count(), t!("sözcük", "words"), (self.zoom * 100.0).round())
    }

    fn cmd(&mut self, c: Cmd) {
        self.command(c);
    }

    fn key(&mut self, vk: u16, k: &Keys) -> bool {
        if self.shortcut(vk, k) {
            return true;
        }
        self.edit_key(vk, k) != Out::No
    }

    fn char(&mut self, ch: char) -> bool {
        if ch.is_control() {
            return false;
        }
        let (lo, hi, n0) = self.span();
        if self.last != Last::Typing || self.ed.cur.has_sel() {
            let s = Snap { paras: self.d.paras.clone(), cur: self.ed.cur.clone() };
            self.push(s);
        }
        if richedit::typed(&mut self.d.paras, &mut self.ed, ch) == Out::Changed {
            self.last = Last::Typing;
            self.changed(lo, hi, n0);
            return true;
        }
        false
    }

    fn mouse(&mut self, m: Mouse, pt: (f32, f32), k: &Keys) {
        self.last = Last::Other;
        match m {
            Mouse::Down => {
                if let Some(at) = self.pos_at(pt) {
                    self.ed.cur.set(at, k.shift);
                    self.ed.goal = None;
                    self.drag = true;
                }
            }
            Mouse::Double => {
                if let Some(at) = self.pos_at(pt) {
                    let text = self.d.paras[at.p].text();
                    let a = rich::word_left(&text, (at.c + 1).min(text.chars().count()));
                    let b = rich::word_right(&text, at.c);
                    self.ed.cur.set(Pos { p: at.p, c: a.min(at.c) }, false);
                    self.ed.cur.set(Pos { p: at.p, c: b }, true);
                }
            }
            Mouse::Move if self.drag => {
                if pt.1 > self.area.bottom() {
                    self.scroll += 20.0;
                } else if pt.1 < self.area.y {
                    self.scroll -= 20.0;
                }
                self.clamp_scroll();
                if let Some(at) = self.pos_at(pt) {
                    self.ed.cur.set(at, true);
                }
            }
            Mouse::Move => {}
            Mouse::Up => self.drag = false,
            Mouse::Right => self.context_menu(),
        }
    }

    fn wheel(&mut self, d: f32, k: &Keys) {
        if k.ctrl {
            self.command(Cmd::Zoom(if d > 0.0 { 1 } else { -1 }));
            return;
        }
        self.scroll -= d * 90.0;
        self.clamp_scroll();
    }

    fn dirty(&self) -> bool {
        self.dirty
    }

    fn ext(&self) -> &'static str {
        "docx"
    }

    fn save(&mut self, path: &Path) -> Result<(), String> {
        let bytes = save::save(&mut self.d)?;
        app::write_atomic(path, &bytes)?;
        self.dirty = false;
        Ok(())
    }

    fn reset_device(&mut self) {
        self.rt = None;
        self.imgs.clear();
        self.invalidate_all();
    }

    fn editing(&self) -> bool {
        true
    }
}
