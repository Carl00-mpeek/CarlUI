//! Bu modül "tek satırlık yazı alanı (hücre düzenleme, formül çubuğu, PDF
//! notu)" sorununu çözüyor: imleç, seçim, kelime atlama, pano ve çizim.
//! Konumlar karakter (char) sayısıdır.

use crate::{
    app::{Act, Keys},
    clip, tl,
};
use carlui_ui::paint::{Painter, R};
use windows::Win32::Graphics::Direct2D::{ID2D1RenderTarget, D2D1_ANTIALIAS_MODE_ALIASED};

#[derive(Clone, Debug, Default)]
pub struct Field {
    pub text: String,
    pub caret: usize,
    pub anchor: usize,
    /// Yatay kaydırma (piksel), imleç görünür kalsın.
    scroll: f32,
}

fn byte(s: &str, c: usize) -> usize {
    s.char_indices().nth(c).map(|(b, _)| b).unwrap_or(s.len())
}

impl Field {
    pub fn new(text: &str) -> Self {
        let n = text.chars().count();
        Field { text: text.to_string(), caret: n, anchor: n, scroll: 0.0 }
    }

    pub fn len(&self) -> usize {
        self.text.chars().count()
    }

    pub fn range(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    pub fn selected(&self) -> String {
        let (a, b) = self.range();
        self.text.chars().skip(a).take(b - a).collect()
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.caret = self.len();
    }

    fn cut_sel(&mut self) {
        let (a, b) = self.range();
        if a != b {
            let (ba, bb) = (byte(&self.text, a), byte(&self.text, b));
            self.text.replace_range(ba..bb, "");
            self.caret = a;
            self.anchor = a;
        }
    }

    pub fn insert(&mut self, s: &str) {
        self.cut_sel();
        let s: String = s.chars().filter(|c| !matches!(c, '\r' | '\n')).collect();
        let at = byte(&self.text, self.caret);
        self.text.insert_str(at, &s);
        self.caret += s.chars().count();
        self.anchor = self.caret;
    }

    fn go(&mut self, c: usize, extend: bool) {
        self.caret = c.min(self.len());
        if !extend {
            self.anchor = self.caret;
        }
    }

    /// Düzenleme/gezinme tuşları. İşlendiyse true.
    pub fn key(&mut self, vk: u16, k: &Keys) -> bool {
        let n = self.len();
        match vk {
            0x25 => {
                // Sol
                let c = if k.ctrl { crate::rich::word_left(&self.text, self.caret) } else if self.caret != self.anchor && !k.shift { self.range().0 } else { self.caret.saturating_sub(1) };
                self.go(c, k.shift);
            }
            0x27 => {
                let c = if k.ctrl { crate::rich::word_right(&self.text, self.caret) } else if self.caret != self.anchor && !k.shift { self.range().1 } else { self.caret + 1 };
                self.go(c, k.shift);
            }
            0x24 => self.go(0, k.shift),
            0x23 => self.go(n, k.shift),
            0x08 => {
                if self.caret == self.anchor {
                    let from = if k.ctrl { crate::rich::word_left(&self.text, self.caret) } else { self.caret.saturating_sub(1) };
                    self.anchor = from;
                }
                self.cut_sel();
            }
            0x2E => {
                if self.caret == self.anchor {
                    let to = if k.ctrl { crate::rich::word_right(&self.text, self.caret) } else { (self.caret + 1).min(n) };
                    self.anchor = to;
                }
                self.cut_sel();
            }
            0x41 if k.ctrl => self.select_all(),
            0x43 if k.ctrl => {
                let s = self.selected();
                if !s.is_empty() {
                    clip::copy(crate::app::hwnd(), &s);
                }
            }
            0x58 if k.ctrl => {
                let s = self.selected();
                if !s.is_empty() {
                    clip::copy(crate::app::hwnd(), &s);
                    self.cut_sel();
                }
            }
            0x56 if k.ctrl => {
                if let Some(s) = clip::paste(crate::app::hwnd()) {
                    // Çok satırlıysa ilk satır (hücreye sekmeli metin yapıştırmayı tablo yapar).
                    let line = s.lines().next().unwrap_or("").to_string();
                    self.insert(&line);
                }
            }
            _ => return false,
        }
        true
    }

    fn style(size: f32) -> tl::Style<'static> {
        tl::Style { family: "Segoe UI", base: crate::rich::Fmt { size: Some(size), ..Default::default() }, px_per_pt: 1.0, align: crate::rich::PAlign::Left, width: 100_000.0, line: 1.0 }
    }

    /// `x` (alan içi) noktasındaki karakter konumu.
    pub fn hit(&self, rt: &ID2D1RenderTarget, r: R, size: f32, x: f32) -> usize {
        tl::plain(rt, &self.text, &Self::style(size)).map(|l| l.hit(x - r.x - 4.0 + self.scroll, 2.0)).unwrap_or(0)
    }

    pub fn click(&mut self, rt: &ID2D1RenderTarget, r: R, size: f32, x: f32, extend: bool) {
        let c = self.hit(rt, r, size, x);
        self.go(c, extend);
    }

    pub fn draw(&mut self, p: &mut Painter<Act>, r: R, size: f32, color: u32, caret: bool) {
        let Some(l) = tl::plain(p.rt, &self.text, &Self::style(size)) else { return };
        let (cx, _, ch) = l.caret(self.caret);
        let inner = r.w - 8.0;
        if cx - self.scroll > inner {
            self.scroll = cx - inner;
        } else if cx < self.scroll {
            self.scroll = cx;
        }
        let y = r.y + (r.h - l.height) / 2.0;
        let x = r.x + 4.0 - self.scroll;
        unsafe {
            p.rt.PushAxisAlignedClip(&r.d2d(), D2D1_ANTIALIAS_MODE_ALIASED);
        }
        let (a, b) = self.range();
        for s in l.sel_rects(a, b) {
            p.fill(R::new(x + s.x, y + s.y, s.w, s.h), 0x3399FF, 0.35);
        }
        l.draw(p.rt, x, y, color, 1.0);
        if caret {
            p.fill(R::new(x + cx, y + (l.height - ch) / 2.0, 1.5, ch), color, 1.0);
        }
        unsafe {
            p.rt.PopAxisAlignedClip();
        }
    }
}
