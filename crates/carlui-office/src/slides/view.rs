//! Bu modül "sunuyu PowerPoint gibi gösterip üzerinde çalışalım" sorununu
//! çözüyor: solda slayt küçük resimleri, ortada etkin slayt; şekle tıklayınca
//! metni yazılır, kenarından sürükleyince taşınır, tutamaçlarından boyutlanır.
//! Slaytlar bitmap olarak önbelleğe çizilir; fare her kıpırdadığında yeniden
//! dizilmez (boşta işlemci harcamaz). Sunu kipi (F5) tam ekrandır.
//! Biçim komutları, geri al ve menüler `edit.rs`te.

use super::{
    render::{self, ShapeLay},
    save, Deck, Slide,
};
use crate::{
    app::{self, Act, Cmd, Keys, Mouse, View},
    img,
    rich::{self, Cursor, Pos},
    richedit::{self, Editor, Nav, Out},
};
use carlui_core::t;
use carlui_ui::paint::{Align, Painter, R, TEXT};
use std::path::Path;
use windows::{
    core::Interface,
    Win32::Graphics::Direct2D::{Common::*, *},
};

/// Sol küçük resim şeridinin genişliği.
pub(super) const RAIL: f32 = 184.0;
const THUMB_W: f32 = 140.0;
const THUMB_GAP: f32 = 14.0;

#[derive(Clone)]
pub(super) struct Snap {
    pub slides: Vec<Slide>,
    pub cur: usize,
    pub sel: Option<usize>,
    pub edit: bool,
    pub ed: Cursor,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Last {
    Other,
    Typing,
    Deleting,
}

pub(super) enum Drag {
    None,
    Text,
    /// Şekli taşı (`h` = None) ya da tutamaçla boyutla.
    Shape { h: Option<u8>, start: (f32, f32), orig: (f32, f32, f32, f32), snap: Option<Box<Snap>> },
}

pub struct SlideView {
    pub(super) d: Deck,
    pub(super) cur: usize,
    pub(super) sel: Option<usize>,
    pub(super) edit: bool,
    pub(super) ed: Editor,
    pub(super) zoom: f32,
    pub(super) area: R,
    /// Slaytın ekrandaki sol üstü ve punto başına piksel.
    pub(super) ox: f32,
    pub(super) oy: f32,
    pub(super) k: f32,
    pub(super) rail_scroll: f32,
    pub(super) reveal_thumb: bool,
    pub(super) mouse: (f32, f32),
    pub(super) scale: f32,
    pub(super) rt: Option<ID2D1RenderTarget>,
    pub(super) imgs: img::Cache,
    pub(super) thumbs: Vec<Option<ID2D1Bitmap>>,
    pub(super) main: Option<(MainKey, ID2D1Bitmap)>,
    pub(super) ver: u64,
    pub(super) undo: Vec<Snap>,
    pub(super) redo: Vec<Snap>,
    pub(super) last: Last,
    pub(super) dirty: bool,
    pub(super) drag: Drag,
    /// Sunu kipinde gösterilen slayt (slayt sayısı = "sunu bitti" ekranı).
    pub(super) present: Option<usize>,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct MainKey {
    pub(super) cur: usize,
    pub(super) ver: u64,
    pub(super) w: i32,
    pub(super) editing: Option<usize>,
}

/// Düzenlenen şeklin metin düzeni (ekran koordinatlarında).
pub(super) struct ShapeNav {
    pub lay: ShapeLay,
    pub bx: f32,
    pub by: f32,
}

impl Nav for ShapeNav {
    fn caret(&self, at: Pos) -> Option<(f32, f32, f32)> {
        let pl = self.lay.paras.get(at.p)?;
        let (cx, cy, h) = pl.laid.as_ref().map(|t| t.caret(at.c)).unwrap_or((0.0, 0.0, pl.h));
        Some((self.bx + pl.x + cx, self.by + pl.y + cy, h))
    }

    fn hit(&self, x: f32, y: f32) -> Option<Pos> {
        if self.lay.paras.is_empty() {
            return None;
        }
        let i = self.lay.paras.iter().rposition(|p| p.y <= y - self.by).unwrap_or(0);
        let pl = &self.lay.paras[i];
        let c = pl.laid.as_ref().map(|t| t.hit(x - self.bx - pl.x, y - self.by - pl.y)).unwrap_or(0);
        Some(Pos { p: i, c })
    }

    fn line_bounds(&self, at: Pos) -> (usize, usize) {
        match self.lay.paras.get(at.p).and_then(|p| p.laid.as_ref()) {
            Some(t) => t.line_bounds(at.c),
            None => (0, at.c),
        }
    }

    fn page(&self) -> f32 {
        200.0
    }
}

/// Ekran dışı bir bitmap'e çizer (`w`×`h` mantıksal piksel, `scale` DPI çarpanı).
pub(super) fn offscreen(rt: &ID2D1RenderTarget, w: f32, h: f32, scale: f32, f: impl FnOnce(&ID2D1RenderTarget)) -> Option<ID2D1Bitmap> {
    unsafe {
        let size = D2D_SIZE_F { width: w.max(1.0), height: h.max(1.0) };
        let px = D2D_SIZE_U { width: (w * scale).ceil().max(1.0) as u32, height: (h * scale).ceil().max(1.0) as u32 };
        let brt = rt.CreateCompatibleRenderTarget(Some(&size), Some(&px), None, D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE).ok()?;
        let base: ID2D1RenderTarget = brt.cast().ok()?;
        base.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        base.BeginDraw();
        base.Clear(None);
        f(&base);
        base.EndDraw(None, None).ok()?;
        brt.GetBitmap().ok()
    }
}

pub(super) fn blit(rt: &ID2D1RenderTarget, bm: &ID2D1Bitmap, r: R) {
    unsafe { rt.DrawBitmap(bm, Some(&r.d2d()), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None) };
}

impl SlideView {
    pub fn new(d: Deck) -> Self {
        SlideView {
            d,
            cur: 0,
            sel: None,
            edit: false,
            ed: Editor::default(),
            zoom: 1.0,
            area: R::new(0.0, 0.0, 800.0, 600.0),
            ox: 0.0,
            oy: 0.0,
            k: 1.0,
            rail_scroll: 0.0,
            reveal_thumb: false,
            mouse: (-1.0, -1.0),
            scale: 1.0,
            rt: None,
            imgs: img::Cache::default(),
            thumbs: Vec::new(),
            main: None,
            ver: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            last: Last::Other,
            dirty: false,
            drag: Drag::None,
            present: None,
        }
    }

    pub(super) fn snapshot(&self) -> Snap {
        Snap { slides: self.d.slides.clone(), cur: self.cur, sel: self.sel, edit: self.edit, ed: self.ed.cur.clone() }
    }

    /// Etkin slaytın içeriği değişti.
    pub(super) fn touched(&mut self) {
        self.dirty = true;
        self.ver += 1;
        if let Some(t) = self.thumbs.get_mut(self.cur) {
            *t = None;
        }
        self.redo.clear();
    }

    /// Slayt sırası / sayısı değişti.
    pub(super) fn restructured(&mut self) {
        self.dirty = true;
        self.ver += 1;
        self.thumbs.clear();
        self.cur = self.cur.min(self.d.slides.len().saturating_sub(1));
        self.reveal_thumb = true;
        self.redo.clear();
    }

    /// Metin değişti: madde numaraları ve kirli bayrağı.
    pub(super) fn text_changed(&mut self, si: usize) {
        if let Some(s) = self.d.slides.get_mut(self.cur).and_then(|s| s.shapes.get_mut(si)) {
            s.dirty = true;
            s.renumber();
        }
        self.touched();
    }

    pub(super) fn go(&mut self, i: usize) {
        let i = i.min(self.d.slides.len().saturating_sub(1));
        if i != self.cur {
            self.cur = i;
            self.sel = None;
            self.edit = false;
            self.last = Last::Other;
            self.reveal_thumb = true;
        }
    }

    pub(super) fn nav(&self, si: usize) -> Option<ShapeNav> {
        let rt = self.rt.as_ref()?;
        let slide = self.d.slides.get(self.cur)?;
        let s = slide.shapes.get(si)?;
        if !s.has_text() {
            return None;
        }
        let th = self.d.theme_for(&slide.layout);
        let lay = render::lay_shape(rt, &th, s, self.k, false);
        let r = render::shape_rect(s, self.ox, self.oy, self.k, Some(&lay));
        let (tx, ty) = render::text_origin(s, &lay, self.k);
        Some(ShapeNav { lay, bx: r.x + tx, by: r.y + ty })
    }

    /// Şeklin ekrandaki dikdörtgeni.
    pub(super) fn rect_of(&self, si: usize) -> Option<R> {
        let s = self.d.slides.get(self.cur)?.shapes.get(si)?;
        let lay = match (s.grow, self.rt.as_ref()) {
            (true, Some(rt)) => Some(render::lay_shape(rt, &self.d.theme_for(&self.d.slides[self.cur].layout), s, self.k, false)),
            _ => None,
        };
        Some(render::shape_rect(s, self.ox, self.oy, self.k, lay.as_ref()))
    }

    /// Noktanın altındaki en üstteki şekil.
    pub(super) fn shape_at(&self, pt: (f32, f32)) -> Option<usize> {
        let n = self.d.slides.get(self.cur)?.shapes.len();
        (0..n).rev().find(|&i| self.rect_of(i).is_some_and(|r| R::new(r.x - 3.0, r.y - 3.0, r.w + 6.0, r.h + 6.0).contains(pt)))
    }

    pub(super) fn handles(r: R) -> [(f32, f32); 8] {
        let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        [(r.x, r.y), (cx, r.y), (r.right(), r.y), (r.right(), cy), (r.right(), r.bottom()), (cx, r.bottom()), (r.x, r.bottom()), (r.x, cy)]
    }

    pub(super) fn handle_at(&self, pt: (f32, f32)) -> Option<u8> {
        let si = self.sel?;
        if self.d.slides[self.cur].shapes.get(si)?.grouped {
            return None;
        }
        let r = self.rect_of(si)?;
        Self::handles(r).iter().position(|(x, y)| (pt.0 - x).abs() <= 6.0 && (pt.1 - y).abs() <= 6.0).map(|i| i as u8)
    }

    /// Nokta şeklin kenar şeridinde mi (metin kutusunu oradan taşırız).
    pub(super) fn on_border(r: R, pt: (f32, f32)) -> bool {
        let inner = R::new(r.x + 6.0, r.y + 6.0, (r.w - 12.0).max(0.0), (r.h - 12.0).max(0.0));
        R::new(r.x - 4.0, r.y - 4.0, r.w + 8.0, r.h + 8.0).contains(pt) && !inner.contains(pt)
    }

    pub(super) fn start_edit(&mut self, si: usize, all: bool) {
        let Some(s) = self.d.slides.get_mut(self.cur).and_then(|s| s.shapes.get_mut(si)) else { return };
        if !s.has_text() {
            return;
        }
        if s.paras.is_empty() {
            s.paras.push(rich::Para::default());
        }
        let last = s.paras.len() - 1;
        let end = Pos { p: last, c: s.paras[last].len() };
        self.sel = Some(si);
        self.edit = true;
        self.last = Last::Other;
        self.ed = Editor::default();
        if all {
            self.ed.cur.set(Pos { p: 0, c: 0 }, false);
            self.ed.cur.set(end, true);
        } else {
            self.ed.cur.set(end, false);
        }
    }

    pub(super) fn edit_key(&mut self, vk: u16, k: &Keys) -> Out {
        let Some(si) = self.sel else { return Out::No };
        let Some(nav) = self.nav(si) else { return Out::No };
        let editing = matches!(vk, 0x08 | 0x2E | 0x0D) || (vk == 0x09 && !k.ctrl) || (k.ctrl && matches!(vk, 0x58 | 0x56));
        let kind = if matches!(vk, 0x08 | 0x2E) && !k.ctrl { Last::Deleting } else { Last::Other };
        let snap = editing.then(|| self.snapshot());
        let paras = &mut self.d.slides[self.cur].shapes[si].paras;
        let out = richedit::key(paras, &mut self.ed, vk, k, &nav);
        match out {
            Out::Changed => {
                if let Some(s) = snap {
                    if kind == Last::Other || self.last != kind {
                        self.push(s);
                    }
                }
                self.last = kind;
                self.text_changed(si);
            }
            Out::Moved => self.last = Last::Other,
            Out::No => {}
        }
        out
    }

    pub(super) fn nudge(&mut self, dx: f32, dy: f32) {
        let Some(si) = self.sel else { return };
        if self.d.slides[self.cur].shapes.get(si).is_none_or(|s| s.grouped) {
            return;
        }
        self.push(self.snapshot());
        let s = &mut self.d.slides[self.cur].shapes[si];
        s.x += dx;
        s.y += dy;
        s.moved = true;
        self.touched();
    }

    // ---- Çizim ----

    pub(super) fn paint_rail(&mut self, p: &mut Painter<Act>, r: R) {
        p.fill(r, 0x000000, 0.14);
        let th_h = THUMB_W * self.d.h / self.d.w.max(1.0);
        let step = th_h + THUMB_GAP;
        let n = self.d.slides.len();
        let total = n as f32 * step + THUMB_GAP;
        if self.reveal_thumb {
            let y = self.cur as f32 * step;
            if y < self.rail_scroll {
                self.rail_scroll = y;
            } else if y + step + THUMB_GAP > self.rail_scroll + r.h {
                self.rail_scroll = y + step + THUMB_GAP - r.h;
            }
            self.reveal_thumb = false;
        }
        self.rail_scroll = self.rail_scroll.clamp(0.0, (total - r.h).max(0.0));
        self.thumbs.resize_with(n, || None);
        let Some(rt) = self.rt.clone() else { return };
        unsafe { p.rt.PushAxisAlignedClip(&r.d2d(), D2D1_ANTIALIAS_MODE_ALIASED) };
        let kt = THUMB_W / self.d.w.max(1.0);
        for i in 0..n {
            let y = r.y + THUMB_GAP + i as f32 * step - self.rail_scroll;
            if y > r.bottom() {
                break;
            }
            if y + th_h < r.y {
                continue;
            }
            let tr = R::new(r.x + 30.0, y, THUMB_W, th_h);
            if self.thumbs[i].is_none() {
                let (d, imgs) = (&self.d, &mut self.imgs);
                self.thumbs[i] = offscreen(&rt, THUMB_W, th_h, self.scale, |b| render::draw_slide(b, d, i, 0.0, 0.0, kt, imgs, false, None));
            }
            let hidden = self.d.slides[i].hidden;
            if let Some(bm) = &self.thumbs[i] {
                blit(p.rt, bm, tr);
            }
            if hidden {
                p.fill(tr, 0x000000, 0.45);
            }
            let on = i == self.cur;
            if on {
                p.outline(R::new(tr.x - 3.0, tr.y - 3.0, tr.w + 6.0, tr.h + 6.0), 4.0, p.accent, 1.0);
            } else if p.hover(tr) {
                p.outline(R::new(tr.x - 2.0, tr.y - 2.0, tr.w + 4.0, tr.h + 4.0), 3.0, 0xFFFFFF, 0.35);
            }
            p.text(&(i + 1).to_string(), R::new(r.x + 4.0, y, 22.0, 18.0), 11.5, on, TEXT, if hidden { 0.35 } else { 0.75 }, Align::Right);
        }
        unsafe { p.rt.PopAxisAlignedClip() };
    }

    pub(super) fn paint_canvas(&mut self, p: &mut Painter<Act>, c: R) {
        let fit = ((c.w - 48.0) / self.d.w.max(1.0)).min((c.h - 48.0) / self.d.h.max(1.0)).max(0.05);
        self.k = fit * self.zoom;
        let (sw, sh) = (self.d.w * self.k, self.d.h * self.k);
        self.ox = (c.x + (c.w - sw) / 2.0).round();
        self.oy = (c.y + (c.h - sh) / 2.0).round();
        let Some(rt) = self.rt.clone() else { return };
        let editing = if self.edit { self.sel } else { None };
        let key = MainKey { cur: self.cur, ver: self.ver, w: (sw * 4.0) as i32, editing };
        if self.main.as_ref().is_none_or(|(k, _)| *k != key) {
            let (d, imgs, cur, k) = (&self.d, &mut self.imgs, self.cur, self.k);
            self.main = offscreen(&rt, sw, sh, self.scale, |b| render::draw_slide(b, d, cur, 0.0, 0.0, k, imgs, true, editing)).map(|bm| (key, bm));
        }
        let sr = R::new(self.ox, self.oy, sw, sh);
        p.fill(R::new(sr.x + 3.0, sr.y + 4.0, sr.w, sr.h), 0x000000, 0.35);
        if let Some((_, bm)) = &self.main {
            blit(p.rt, bm, sr);
        }
        self.paint_overlay(p, sr);
    }

    /// Seçili şeklin çerçevesi, tutamaçlar, metin seçimi ve imleç.
    pub(super) fn paint_overlay(&mut self, p: &mut Painter<Act>, sr: R) {
        let Some(si) = self.sel else { return };
        let Some(r) = self.rect_of(si) else { return };
        let grouped = self.d.slides[self.cur].shapes[si].grouped;
        unsafe { p.rt.PushAxisAlignedClip(&R::new(sr.x - 8.0, sr.y - 8.0, sr.w + 16.0, sr.h + 16.0).d2d(), D2D1_ANTIALIAS_MODE_ALIASED) };
        p.outline(r, 0.0, p.accent, if self.edit { 0.55 } else { 0.9 });
        if !grouped && !self.edit {
            for (x, y) in Self::handles(r) {
                let h = R::new(x - 4.0, y - 4.0, 8.0, 8.0);
                p.fill(h, 0xFFFFFF, 1.0);
                p.outline(h, 0.0, p.accent, 1.0);
            }
        }
        if self.edit {
            if let Some(nav) = self.nav(si) {
                let (a, b) = self.ed.cur.range();
                if a != b {
                    for (i, pl) in nav.lay.paras.iter().enumerate() {
                        if i < a.p || i > b.p {
                            continue;
                        }
                        let Some(t) = &pl.laid else { continue };
                        let s = if i == a.p { a.c } else { 0 };
                        let e = if i == b.p { b.c } else { t.chars() };
                        for q in t.sel_rects(s, e) {
                            p.fill(R::new(nav.bx + pl.x + q.x, nav.by + pl.y + q.y, q.w, q.h), p.accent, 0.35);
                        }
                    }
                } else if app::caret_on() {
                    if let Some((x, y, h)) = nav.caret(self.ed.cur.at) {
                        let bg = self.slide_bg();
                        let lum = ((bg >> 16) & 0xFF) * 3 + ((bg >> 8) & 0xFF) * 6 + (bg & 0xFF);
                        p.fill(R::new(x, y, (1.5 * self.k / 1.33).max(1.0), h), if lum > 1270 { 0x000000 } else { 0xFFFFFF }, 1.0);
                    }
                }
            }
        }
        unsafe { p.rt.PopAxisAlignedClip() };
    }

    pub(super) fn slide_bg(&self) -> u32 {
        let s = &self.d.slides[self.cur];
        match self.d.bg(s) {
            super::Bg::Solid(c) => c,
            _ => self.d.theme_for(&s.layout).colors.get("bg1").copied().unwrap_or(0xFFFFFF),
        }
    }

    pub(super) fn mouse_rail(&mut self, m: Mouse, pt: (f32, f32)) {
        let th_h = THUMB_W * self.d.h / self.d.w.max(1.0);
        let y = pt.1 - self.area.y - THUMB_GAP + self.rail_scroll;
        let i = (y / (th_h + THUMB_GAP)).floor();
        if i >= 0.0 && (i as usize) < self.d.slides.len() {
            self.go(i as usize);
        }
        if m == Mouse::Right {
            self.slide_menu();
        }
    }
}

impl View for SlideView {
    fn toolbar(&mut self, p: &mut Painter<Act>, r: R) {
        self.toolbar_ui(p, r);
    }

    fn paint(&mut self, p: &mut Painter<Act>, area: R) {
        if self.rt.is_none() {
            self.rt = Some(p.rt.clone());
        }
        unsafe {
            let mut m = Default::default();
            p.rt.GetTransform(&mut m);
            let s = m.M11;
            if (s - self.scale).abs() > 0.001 && s > 0.0 {
                self.scale = s;
                self.thumbs.clear();
                self.main = None;
            }
        }
        self.mouse = p.mouse;
        if let Some(i) = self.present {
            self.paint_present(p, area, i);
            return;
        }
        self.area = area;
        if self.d.slides.is_empty() {
            p.text(t!("Sunuda slayt yok.", "The presentation has no slides."), area, 14.0, false, TEXT, 0.6, Align::Center);
            return;
        }
        self.paint_rail(p, R::new(area.x, area.y, RAIL, area.h));
        self.paint_canvas(p, R::new(area.x + RAIL, area.y, area.w - RAIL, area.h));
    }

    fn status(&self) -> String {
        let n = self.d.slides.len();
        let pct = (self.k / (96.0 / 72.0) * 100.0).round();
        format!("{} {} / {n} · %{pct}", t!("Slayt", "Slide"), (self.cur + 1).min(n))
    }

    fn cmd(&mut self, c: Cmd) {
        self.command(c);
    }

    fn key(&mut self, vk: u16, k: &Keys) -> bool {
        if self.present.is_some() {
            return self.present_key(vk);
        }
        if vk == 0x74 {
            // F5 baştan, Shift+F5 bu slayttan.
            self.present_from(if k.shift { self.cur } else { 0 });
            return true;
        }
        if self.shortcut(vk, k) {
            return true;
        }
        if self.edit {
            match vk {
                0x1B => {
                    self.edit = false;
                    return true;
                }
                0x09 if !k.ctrl && self.indent_at_start(k.shift) => return true,
                _ => {}
            }
            return self.edit_key(vk, k) != Out::No;
        }
        let step = if k.ctrl { 0.5 } else if k.shift { 10.0 } else { 2.0 };
        match (vk, self.sel) {
            (0x1B, Some(_)) => self.sel = None,
            (0x2E | 0x08, Some(_)) => self.command(Cmd::Remove),
            (0x2E, None) => self.command(Cmd::Remove),
            (0x25, Some(_)) => self.nudge(-step, 0.0),
            (0x27, Some(_)) => self.nudge(step, 0.0),
            (0x26, Some(_)) => self.nudge(0.0, -step),
            (0x28, Some(_)) => self.nudge(0.0, step),
            (0x0D, Some(si)) => self.start_edit(si, true),
            (0x71, Some(si)) => self.start_edit(si, false),
            (0x25 | 0x26 | 0x21, _) => self.go(self.cur.saturating_sub(1)),
            (0x27 | 0x28 | 0x22, _) => self.go(self.cur + 1),
            (0x24, _) => self.go(0),
            (0x23, _) => self.go(usize::MAX),
            (0x09, _) => {
                // Tab: şekiller arasında gez.
                let n = self.d.slides[self.cur].shapes.len();
                if n > 0 {
                    self.sel = Some(match self.sel {
                        Some(i) if k.shift => (i + n - 1) % n,
                        Some(i) => (i + 1) % n,
                        None => 0,
                    });
                }
            }
            _ => return false,
        }
        true
    }

    fn char(&mut self, ch: char) -> bool {
        if ch.is_control() || self.present.is_some() {
            return false;
        }
        if !self.edit {
            // Seçili şekle yazmaya başlamak metnini değiştirir (PowerPoint gibi).
            let Some(si) = self.sel else { return false };
            if !self.d.slides[self.cur].shapes.get(si).is_some_and(|s| s.has_text()) {
                return false;
            }
            self.start_edit(si, true);
        }
        let Some(si) = self.sel else { return false };
        if self.last != Last::Typing || self.ed.cur.has_sel() {
            self.push(self.snapshot());
        }
        let paras = &mut self.d.slides[self.cur].shapes[si].paras;
        if richedit::typed(paras, &mut self.ed, ch) == Out::Changed {
            self.last = Last::Typing;
            self.text_changed(si);
            return true;
        }
        false
    }

    fn mouse(&mut self, m: Mouse, pt: (f32, f32), k: &Keys) {
        self.on_mouse(m, pt, k);
    }

    fn wheel(&mut self, d: f32, k: &Keys) {
        if self.present.is_some() {
            self.present_step(d < 0.0);
            return;
        }
        if self.mouse.0 >= 0.0 && self.mouse.0 < self.area.x + RAIL {
            self.rail_scroll -= d * 90.0;
            return;
        }
        if k.ctrl {
            self.command(Cmd::Zoom(if d > 0.0 { 1 } else { -1 }));
        } else if d < 0.0 {
            self.go(self.cur + 1);
        } else {
            self.go(self.cur.saturating_sub(1));
        }
    }

    fn dirty(&self) -> bool {
        self.dirty
    }

    fn ext(&self) -> &'static str {
        "pptx"
    }

    fn save(&mut self, path: &Path) -> Result<(), String> {
        let bytes = save::save(&mut self.d)?;
        app::write_atomic(path, &bytes)?;
        self.dirty = false;
        self.ver += 1;
        Ok(())
    }

    fn reset_device(&mut self) {
        self.rt = None;
        self.imgs.clear();
        self.thumbs.clear();
        self.main = None;
    }

    fn editing(&self) -> bool {
        self.edit && self.present.is_none()
    }

    fn fullscreen(&self) -> bool {
        self.present.is_some()
    }
}
