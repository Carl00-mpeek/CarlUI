//! Bu modül "PDF'yi okur gibi gezinelim" sorununu çözüyor: sayfalar alt alta
//! sürekli kayar, solda küçük resim şeridi, genişliğe sığdır ya da Ctrl+teker
//! ile yakınlaştırma. Yalnızca görünen sayfaların bitmap'i bellekte tutulur
//! (yüzlerce sayfalık dosyada da bellek az kalır); çizim yardımcı iş
//! parçacığında yapılır, pencere donmaz. Komutlar ve menüler `ops.rs`te.

use super::{Job, Renderer};
use crate::{
    app::{self, Act, Cmd, Keys, Mouse, View},
    img,
};
use carlui_core::t;
use carlui_ui::paint::{Align, Painter, R, TEXT};
use std::{collections::HashMap, path::Path, sync::Arc};
use windows::Win32::Graphics::Direct2D::*;

pub(super) const RAIL: f32 = 170.0;
pub(super) const THUMB_W: f32 = 112.0;
const GAP: f32 = 14.0;
const PAD: f32 = 18.0;
const MARGIN: f32 = 28.0;
pub(super) const ZOOMS: [f32; 15] = [0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0];

/// Yazılmakta olan metin (sayfaya henüz eklenmedi).
pub(super) struct Note {
    pub page: usize,
    /// Görünen sayfada sol üstten oran (0..1).
    pub u: f32,
    pub v: f32,
    pub text: String,
}

pub struct PdfView {
    pub(super) bytes: Arc<Vec<u8>>,
    pub(super) sizes: Vec<(f32, f32)>,
    pub(super) ren: Option<Renderer>,
    pub(super) pages: HashMap<usize, (u32, ID2D1Bitmap)>,
    pub(super) thumbs: HashMap<usize, (u32, ID2D1Bitmap)>,
    pub(super) failed: Vec<(usize, bool)>,
    pub(super) zoom: f32,
    pub(super) fit: bool,
    /// Etkin yakınlaştırma (DIP başına piksel).
    pub(super) z: f32,
    pub(super) sy: f32,
    pub(super) sx: f32,
    pub(super) rail_scroll: f32,
    pub(super) area: R,
    pub(super) scale: f32,
    pub(super) cur: usize,
    pub(super) goto: Option<usize>,
    pub(super) reveal_thumb: bool,
    pub(super) undo: Vec<Arc<Vec<u8>>>,
    pub(super) redo: Vec<Arc<Vec<u8>>>,
    pub(super) dirty: bool,
    pub(super) note_tool: bool,
    pub(super) note: Option<Note>,
    pub(super) note_size: f32,
    pub(super) note_color: u32,
    pub(super) drag: Option<((f32, f32), f32, f32)>,
    pub(super) mouse: (f32, f32),
}

impl PdfView {
    pub fn new(bytes: Vec<u8>) -> Result<Self, String> {
        let sizes = super::sizes(&bytes)?;
        Ok(PdfView {
            bytes: Arc::new(bytes),
            sizes,
            ren: None,
            pages: HashMap::new(),
            thumbs: HashMap::new(),
            failed: Vec::new(),
            zoom: 1.0,
            fit: true,
            z: 1.0,
            sy: 0.0,
            sx: 0.0,
            rail_scroll: 0.0,
            area: R::new(0.0, 0.0, 800.0, 600.0),
            scale: 1.0,
            cur: 0,
            goto: None,
            reveal_thumb: false,
            undo: Vec::new(),
            redo: Vec::new(),
            dirty: false,
            note_tool: false,
            note: None,
            note_size: 12.0,
            note_color: 0x000000,
            drag: None,
            mouse: (-1.0, -1.0),
        })
    }

    pub(super) fn canvas(&self) -> R {
        R::new(self.area.x + RAIL, self.area.y, (self.area.w - RAIL).max(50.0), self.area.h)
    }

    fn max_w(&self) -> f32 {
        self.sizes.iter().map(|s| s.0).fold(1.0, f32::max)
    }

    /// Her sayfanın belge içindeki üst kenarı (yakınlaştırılmış piksel) ve toplam yükseklik.
    pub(super) fn tops(&self) -> (Vec<f32>, f32) {
        let mut y = PAD;
        let mut out = Vec::with_capacity(self.sizes.len());
        for s in &self.sizes {
            out.push(y);
            y += s.1 * self.z + GAP;
        }
        (out, y - GAP + PAD)
    }

    /// Sayfanın ekrandaki dikdörtgeni.
    pub(super) fn page_rect(&self, tops: &[f32], i: usize) -> R {
        let c = self.canvas();
        let (w, h) = self.sizes[i];
        let (w, h) = (w * self.z, h * self.z);
        let x = c.x + ((c.w - w) / 2.0).max(MARGIN) - self.sx;
        R::new(x.round(), (c.y + tops[i] - self.sy).round(), w, h)
    }

    pub(super) fn fit_zoom(&self) -> f32 {
        ((self.canvas().w - 2.0 * MARGIN) / self.max_w()).clamp(0.1, 4.0)
    }

    /// Yakınlaştırmayı değiştirir; ekranın üstündeki nokta yerinde kalır.
    pub(super) fn set_zoom(&mut self, z: Option<f32>) {
        let old = self.z;
        self.fit = z.is_none();
        if let Some(z) = z {
            self.zoom = z.clamp(0.1, 4.0);
        }
        self.z = if self.fit { self.fit_zoom() } else { self.zoom };
        if (self.z - old).abs() > 1e-4 {
            self.sy = ((self.sy - PAD).max(0.0) * self.z / old + PAD).max(0.0);
            if self.sy <= PAD + 0.5 {
                self.sy = 0.0;
            }
        }
    }

    pub(super) fn clamp_scroll(&mut self, total: f32) {
        let c = self.canvas();
        self.sy = self.sy.clamp(0.0, (total - c.h).max(0.0));
        let wide = self.max_w() * self.z + 2.0 * MARGIN;
        self.sx = self.sx.clamp(0.0, (wide - c.w).max(0.0));
    }

    /// Ekrandaki noktanın altındaki sayfa ve sayfa içindeki oranı.
    pub(super) fn page_at(&self, pt: (f32, f32)) -> Option<(usize, f32, f32)> {
        let (tops, _) = self.tops();
        (0..self.sizes.len()).find_map(|i| {
            let r = self.page_rect(&tops, i);
            r.contains(pt).then(|| (i, (pt.0 - r.x) / r.w, (pt.1 - r.y) / r.h))
        })
    }

    pub(super) fn go(&mut self, i: usize) {
        self.goto = Some(i.min(self.sizes.len().saturating_sub(1)));
    }

    fn thumb_h(&self, i: usize) -> f32 {
        let (w, h) = self.sizes[i];
        (THUMB_W * h / w).min(THUMB_W * 1.6)
    }

    fn thumb_tops(&self) -> (Vec<f32>, f32) {
        let mut y = GAP;
        let mut out = Vec::with_capacity(self.sizes.len());
        for i in 0..self.sizes.len() {
            out.push(y);
            y += self.thumb_h(i) + GAP + 6.0;
        }
        (out, y)
    }

    /// Kanaldan gelen çizimleri bitmap'e çevirir.
    fn take_results(&mut self, rt: &ID2D1RenderTarget) {
        let Some(ren) = &self.ren else { return };
        for (job, out) in ren.results() {
            match out.and_then(|b| img::to_d2d(rt, &b)) {
                Some(bm) => {
                    let map = if job.thumb { &mut self.thumbs } else { &mut self.pages };
                    map.insert(job.page, (job.width, bm));
                }
                None => self.failed.push((job.page, job.thumb)),
            }
        }
    }

    fn needs(map: &HashMap<usize, (u32, ID2D1Bitmap)>, i: usize, w: u32) -> bool {
        map.get(&i).is_none_or(|(have, _)| (*have as f32 - w as f32).abs() > w as f32 * 0.02 + 1.0)
    }

    fn paint_rail(&mut self, p: &mut Painter<Act>, r: R, jobs: &mut Vec<Job>) {
        p.fill(r, 0x000000, 0.14);
        let (tops, total) = self.thumb_tops();
        if self.reveal_thumb {
            let y = tops[self.cur];
            let h = self.thumb_h(self.cur) + GAP + 6.0;
            if y < self.rail_scroll {
                self.rail_scroll = y - GAP;
            } else if y + h > self.rail_scroll + r.h {
                self.rail_scroll = y + h - r.h;
            }
            self.reveal_thumb = false;
        }
        self.rail_scroll = self.rail_scroll.clamp(0.0, (total - r.h).max(0.0));
        let tw = (THUMB_W * self.scale).round() as u32;
        let mut keep = Vec::new();
        unsafe { p.rt.PushAxisAlignedClip(&r.d2d(), D2D1_ANTIALIAS_MODE_ALIASED) };
        for (i, &ty) in tops.iter().enumerate() {
            let y = r.y + ty - self.rail_scroll;
            let th = self.thumb_h(i);
            if y > r.bottom() + 400.0 {
                break;
            }
            if y + th < r.y - 400.0 {
                continue;
            }
            keep.push(i);
            if y > r.bottom() || y + th < r.y {
                continue;
            }
            let tr = R::new(r.x + 34.0, y, THUMB_W, th);
            carlui_ui::tone::literal(|| p.fill(tr, 0xFFFFFF, 1.0));
            match self.thumbs.get(&i) {
                Some((_, bm)) => blit(p.rt, bm, tr),
                None if !self.failed.contains(&(i, true)) => jobs.push(Job { page: i, width: tw, thumb: true }),
                None => {}
            }
            let on = i == self.cur;
            if on {
                p.outline(R::new(tr.x - 3.0, tr.y - 3.0, tr.w + 6.0, tr.h + 6.0), 4.0, p.accent, 1.0);
            } else if p.hover(tr) {
                p.outline(R::new(tr.x - 2.0, tr.y - 2.0, tr.w + 4.0, tr.h + 4.0), 3.0, 0xFFFFFF, 0.35);
            }
            p.text(&(i + 1).to_string(), R::new(r.x + 4.0, y, 24.0, 18.0), 11.5, on, TEXT, 0.75, Align::Right);
        }
        unsafe { p.rt.PopAxisAlignedClip() };
        self.thumbs.retain(|k, _| keep.contains(k));
    }

    fn paint_pages(&mut self, p: &mut Painter<Act>, c: R, jobs: &mut Vec<Job>) {
        let (tops, total) = self.tops();
        if let Some(i) = self.goto.take() {
            self.sy = tops[i] - PAD;
            self.reveal_thumb = true;
        }
        self.clamp_scroll(total);
        // Etkin sayfa: ekranın üst üçte birindeki sayfa.
        let probe = self.sy + c.h * 0.3;
        let cur = tops.iter().rposition(|&t| t <= probe).unwrap_or(0);
        if cur != self.cur {
            self.cur = cur;
            self.reveal_thumb = true;
        }
        let mut visible = Vec::new();
        unsafe { p.rt.PushAxisAlignedClip(&c.d2d(), D2D1_ANTIALIAS_MODE_ALIASED) };
        for i in 0..self.sizes.len() {
            let r = self.page_rect(&tops, i);
            if r.y > c.bottom() {
                break;
            }
            if r.bottom() < c.y {
                continue;
            }
            visible.push(i);
            p.paper(r);
            if let Some((_, bm)) = self.pages.get(&i) {
                blit(p.rt, bm, r);
            }
        }
        carlui_ui::tone::literal(|| self.paint_note(p, &tops));
        unsafe { p.rt.PopAxisAlignedClip() };
        // Görünenler önce (ekran ortasına yakın olan önce), sonra bir sonraki sayfa.
        let mid = c.y + c.h / 2.0;
        visible.sort_by(|&a, &b| {
            let da = (self.page_rect(&tops, a).y + self.page_rect(&tops, a).h / 2.0 - mid).abs();
            let db = (self.page_rect(&tops, b).y + self.page_rect(&tops, b).h / 2.0 - mid).abs();
            da.total_cmp(&db)
        });
        let mut want = visible.clone();
        if let Some(&last) = visible.iter().max() {
            if last + 1 < self.sizes.len() {
                want.push(last + 1);
            }
        }
        for &i in &want {
            let w = (self.sizes[i].0 * self.z * self.scale).round().max(8.0) as u32;
            if Self::needs(&self.pages, i, w) && !self.failed.contains(&(i, false)) {
                jobs.insert(jobs.iter().take_while(|j| !j.thumb).count(), Job { page: i, width: w, thumb: false });
            }
        }
        self.pages.retain(|k, _| want.contains(k) || visible.iter().any(|v| v.abs_diff(*k) <= 1));
    }

    /// Yazılmakta olan metni ve imleci sayfanın üstüne çizer.
    fn paint_note(&self, p: &mut Painter<Act>, tops: &[f32]) {
        let Some(n) = &self.note else { return };
        let r = self.page_rect(tops, n.page);
        let px = self.note_size * 96.0 / 72.0 * self.z;
        let lh = px * 1.2;
        let (x, y) = (r.x + n.u * r.w, r.y + n.v * r.h);
        let lines: Vec<&str> = n.text.split('\n').collect();
        let w = lines.iter().map(|l| p.fonts.width(l, px, false)).fold(px, f32::max);
        p.outline(R::new(x - 4.0, y - 3.0, w + 10.0, lh * lines.len() as f32 + 6.0), 3.0, p.accent, 0.8);
        for (i, l) in lines.iter().enumerate() {
            p.text(l, R::new(x, y + i as f32 * lh, w + px * 4.0, lh), px, false, self.note_color, 1.0, Align::Left);
        }
        if app::caret_on() {
            let last = lines.last().copied().unwrap_or("");
            let cx = x + p.fonts.width(last, px, false) + 1.0;
            p.fill(R::new(cx, y + (lines.len() - 1) as f32 * lh + lh * 0.1, 1.5, lh * 0.8), self.note_color, 1.0);
        }
    }

    pub(super) fn scroll_by(&mut self, dy: f32) {
        self.sy += dy;
    }
}

pub(super) fn blit(rt: &ID2D1RenderTarget, bm: &ID2D1Bitmap, r: R) {
    unsafe { rt.DrawBitmap(bm, Some(&r.d2d()), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None) };
}

impl View for PdfView {
    fn toolbar(&mut self, p: &mut Painter<Act>, r: R) {
        self.toolbar_ui(p, r);
    }

    fn paint(&mut self, p: &mut Painter<Act>, area: R) {
        unsafe {
            let mut m = Default::default();
            p.rt.GetTransform(&mut m);
            let s = m.M11;
            if (s - self.scale).abs() > 0.001 && s > 0.0 {
                self.scale = s;
            }
        }
        self.mouse = p.mouse;
        self.area = area;
        if self.fit {
            self.set_zoom(None);
        }
        if self.ren.is_none() {
            self.ren = Some(Renderer::start(self.bytes.clone(), app::hwnd().0 as isize));
        }
        self.take_results(p.rt);
        let mut jobs = Vec::new();
        self.paint_pages(p, self.canvas(), &mut jobs);
        self.paint_rail(p, R::new(area.x, area.y, RAIL, area.h), &mut jobs);
        if let Some(r) = &self.ren {
            r.want(jobs);
        }
    }

    fn status(&self) -> String {
        let n = self.sizes.len();
        let mut s = format!("{} {} / {n} · %{}", t!("Sayfa", "Page"), (self.cur + 1).min(n), (self.z * 100.0).round());
        if self.note_tool {
            s.push_str(" · ");
            s.push_str(t!("Metin eklemek için sayfaya tıklayın", "Click on a page to add text"));
        }
        s
    }

    fn cmd(&mut self, c: Cmd) {
        self.command(c);
    }

    fn key(&mut self, vk: u16, k: &Keys) -> bool {
        self.on_key(vk, k)
    }

    fn char(&mut self, ch: char) -> bool {
        let Some(n) = &mut self.note else { return false };
        if ch.is_control() {
            return false;
        }
        n.text.push(ch);
        app::caret_reset();
        true
    }

    fn mouse(&mut self, m: Mouse, pt: (f32, f32), k: &Keys) {
        self.on_mouse(m, pt, k);
    }

    fn wheel(&mut self, d: f32, k: &Keys) {
        if self.mouse.0 >= 0.0 && self.mouse.0 < self.area.x + RAIL {
            self.rail_scroll -= d * 90.0;
            return;
        }
        if k.ctrl {
            self.command(Cmd::Zoom(if d > 0.0 { 1 } else { -1 }));
        } else if k.shift {
            self.sx -= d * 90.0;
        } else {
            self.scroll_by(-d * 110.0);
        }
    }

    fn dirty(&self) -> bool {
        self.dirty
    }

    fn ext(&self) -> &'static str {
        "pdf"
    }

    fn save(&mut self, path: &Path) -> Result<(), String> {
        self.commit_note();
        app::write_atomic(path, &self.bytes)?;
        self.dirty = false;
        Ok(())
    }

    fn reset_device(&mut self) {
        self.pages.clear();
        self.thumbs.clear();
    }

    fn ticking(&self) -> bool {
        self.ren.as_ref().is_some_and(|r| r.working())
    }

    fn editing(&self) -> bool {
        self.note.is_some()
    }
}
