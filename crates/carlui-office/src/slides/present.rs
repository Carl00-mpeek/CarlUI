//! Bu modül "sunuyu tam ekranda slayt slayt oynatalım" sorununu çözüyor
//! (F5): gizli slaytlar atlanır, ok/boşluk/Enter ilerletir, Esc ya da son
//! ekrandan sonra tıklamak çıkar. Slayt ekran boyutunda bitmap'e bir kez
//! çizilir; sonraki karelerde yalnızca kopyalanır.

use super::{
    render,
    view::{blit, offscreen, MainKey, SlideView},
};
use crate::app::Act;
use carlui_core::t;
use carlui_ui::paint::{Align, Painter, R};

impl SlideView {
    pub(super) fn next_visible(&self, from: usize) -> usize {
        (from..self.d.slides.len()).find(|&i| !self.d.slides[i].hidden).unwrap_or(self.d.slides.len())
    }

    pub(super) fn prev_visible(&self, from: usize) -> usize {
        (0..from).rev().find(|&i| !self.d.slides[i].hidden).unwrap_or(from)
    }

    pub(super) fn present_from(&mut self, i: usize) {
        self.edit = false;
        self.sel = None;
        let n = self.d.slides.len();
        let first = self.next_visible(i.min(n));
        self.present = Some(if first == n { self.next_visible(0) } else { first });
        self.main = None;
    }

    pub(super) fn present_step(&mut self, fwd: bool) {
        let Some(i) = self.present else { return };
        let n = self.d.slides.len();
        if fwd {
            if i >= n {
                self.end_present();
            } else {
                self.present = Some(self.next_visible(i + 1));
            }
        } else {
            self.present = Some(self.prev_visible(i));
        }
    }

    pub(super) fn end_present(&mut self) {
        if let Some(i) = self.present.take() {
            self.go(i.min(self.d.slides.len().saturating_sub(1)));
        }
        self.main = None;
    }

    pub(super) fn present_key(&mut self, vk: u16) -> bool {
        match vk {
            0x1B => self.end_present(),
            0x27 | 0x28 | 0x22 | 0x20 | 0x0D | 0x4E => self.present_step(true),
            0x25 | 0x26 | 0x21 | 0x08 | 0x50 => self.present_step(false),
            0x24 => self.present = Some(self.next_visible(0)),
            0x23 => self.present = Some(self.prev_visible(self.d.slides.len())),
            _ => return false,
        }
        true
    }

    pub(super) fn paint_present(&mut self, p: &mut Painter<Act>, area: R, i: usize) {
        p.fill(area, 0x000000, 1.0);
        if i >= self.d.slides.len() {
            p.text(t!("Sunu sonu. Çıkmak için tıklayın.", "End of slide show. Click to exit."), R::new(0.0, 24.0, area.w, 30.0), 15.0, false, 0xFFFFFF, 0.8, Align::Center);
            return;
        }
        let k = (area.w / self.d.w.max(1.0)).min(area.h / self.d.h.max(1.0));
        let (sw, sh) = (self.d.w * k, self.d.h * k);
        let r = R::new(((area.w - sw) / 2.0).round(), ((area.h - sh) / 2.0).round(), sw, sh);
        let Some(rt) = self.rt.clone() else { return };
        let key = MainKey { cur: i, ver: self.ver, w: -((sw * 4.0) as i32), editing: None };
        if self.main.as_ref().is_none_or(|(k, _)| *k != key) {
            let (d, imgs) = (&self.d, &mut self.imgs);
            self.main = offscreen(&rt, sw, sh, self.scale, |b| render::draw_slide(b, d, i, 0.0, 0.0, k, imgs, false, None)).map(|bm| (key, bm));
        }
        if let Some((_, bm)) = &self.main {
            blit(p.rt, bm, r);
        }
    }
}
