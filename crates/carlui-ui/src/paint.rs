//! Anlık modlu (immediate-mode) pencere çizimi için ortak yardımcılar
//! (Direct2D + DirectWrite): her karede sayfa baştan çizilir, tıklanabilir her
//! şey çizilirken bir dikdörtgen + eylem (`A`) olarak kaydedilir; tıklama bu
//! listeden bulunur. Görev yöneticisi ve ayarlar penceresi aynı parçaları
//! kullanır. Renkler koyu cam (akrilik) zemine göre seçilmiştir.
use crate::tween;
use std::{cell::RefCell, collections::HashMap};
use windows::{
    core::*,
    Win32::Graphics::{
        Direct2D::{Common::*, *},
        DirectWrite::*,
    },
};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct R {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl R {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn d2d(&self) -> D2D_RECT_F {
        D2D_RECT_F { left: self.x, top: self.y, right: self.x + self.w, bottom: self.y + self.h }
    }
    pub fn contains(&self, p: (f32, f32)) -> bool {
        p.0 >= self.x && p.0 < self.x + self.w && p.1 >= self.y && p.1 < self.y + self.h
    }
    pub fn inset(&self, d: f32) -> R {
        R::new(self.x + d, self.y + d, self.w - 2.0 * d, self.h - 2.0 * d)
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
}

pub const TEXT: u32 = 0xFFFFFF;
pub const RED: u32 = 0xFF6B6B;
pub const ORANGE: u32 = 0xFFB454;
pub const GREEN: u32 = 0x6CCB5F;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Align {
    Left,
    Center,
    Right,
}

pub fn rgb(c: u32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: ((c >> 16) & 0xFF) as f32 / 255.0, g: ((c >> 8) & 0xFF) as f32 / 255.0, b: (c & 0xFF) as f32 / 255.0, a }
}

pub struct Fonts {
    dw: IDWriteFactory,
    cache: RefCell<HashMap<(u32, bool, Align, bool), IDWriteTextFormat>>,
}

impl Fonts {
    pub fn new() -> Result<Self> {
        Ok(Self { dw: unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? }, cache: RefCell::default() })
    }

    fn get(&self, size: f32, bold: bool, align: Align, icon: bool) -> Option<IDWriteTextFormat> {
        let key = ((size * 10.0) as u32, bold, align, icon);
        if let Some(f) = self.cache.borrow().get(&key) {
            return Some(f.clone());
        }
        let weight = if bold { DWRITE_FONT_WEIGHT_SEMI_BOLD } else { DWRITE_FONT_WEIGHT_NORMAL };
        let f = unsafe {
            let fam = if icon { w!("Segoe Fluent Icons") } else { w!("Segoe UI Variable Text") };
            let f = self.dw.CreateTextFormat(fam, None, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("tr-tr")).or_else(|_| self.dw.CreateTextFormat(if icon { w!("Segoe MDL2 Assets") } else { w!("Segoe UI") }, None, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("tr-tr"))).ok()?;
            let _ = f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            let _ = f.SetTextAlignment(match align {
                Align::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
                Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
                Align::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
            });
            let trim = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
            let sign = self.dw.CreateEllipsisTrimmingSign(&f).ok();
            let _ = f.SetTrimming(&trim, sign.as_ref());
            f
        };
        self.cache.borrow_mut().insert(key, f.clone());
        Some(f)
    }

    pub fn width(&self, s: &str, size: f32, bold: bool) -> f32 {
        let Some(f) = self.get(size, bold, Align::Left, false) else { return 0.0 };
        let w: Vec<u16> = s.encode_utf16().collect();
        unsafe {
            self.dw
                .CreateTextLayout(&w, &f, 4000.0, 100.0)
                .and_then(|l| {
                    let mut m = DWRITE_TEXT_METRICS::default();
                    l.GetMetrics(&mut m).map(|_| m.widthIncludingTrailingWhitespace)
                })
                .unwrap_or(0.0)
        }
    }
}

/// İki rengi `t` oranında karıştırır (0 = a, 1 = b).
pub fn mix(a: u32, b: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let ch = |s: u32| {
        let (x, y) = (((a >> s) & 0xFF) as f32, ((b >> s) & 0xFF) as f32);
        ((x + (y - x) * t).round() as u32) << s
    };
    ch(16) | ch(8) | ch(0)
}

pub struct Painter<'a, A> {
    pub rt: &'a ID2D1RenderTarget,
    pub fonts: &'a Fonts,
    pub mouse: (f32, f32),
    pub hits: &'a mut Vec<(R, A)>,
    pub accent: u32,
}

impl<A: Clone> Painter<'_, A> {
    fn brush(&self, c: u32, a: f32) -> Option<ID2D1SolidColorBrush> {
        unsafe { self.rt.CreateSolidColorBrush(&rgb(c, a), None).ok() }
    }

    pub fn hover(&self, r: R) -> bool {
        r.contains(self.mouse)
    }

    pub fn hit(&mut self, r: R, a: A) {
        self.hits.push((r, a));
    }

    pub fn fill(&self, r: R, c: u32, a: f32) {
        if let Some(b) = self.brush(c, a) {
            unsafe { self.rt.FillRectangle(&r.d2d(), &b) };
        }
    }

    pub fn round(&self, r: R, rad: f32, c: u32, a: f32) {
        if let Some(b) = self.brush(c, a) {
            unsafe { self.rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: rad, radiusY: rad }, &b) };
        }
    }

    pub fn outline(&self, r: R, rad: f32, c: u32, a: f32) {
        if let Some(b) = self.brush(c, a) {
            let r = r.inset(0.5);
            unsafe { self.rt.DrawRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: rad, radiusY: rad }, &b, 1.0, None) };
        }
    }

    /// Cam kart: hafif açık zemin + ince kenar.
    pub fn card(&self, r: R) {
        self.round(r, 8.0, 0xFFFFFF, 0.045);
        self.outline(r, 8.0, 0xFFFFFF, 0.07);
    }

    pub fn text(&self, s: &str, r: R, size: f32, bold: bool, c: u32, a: f32, align: Align) {
        let (Some(f), Some(b)) = (self.fonts.get(size, bold, align, false), self.brush(c, a)) else { return };
        let w: Vec<u16> = s.encode_utf16().collect();
        unsafe { self.rt.DrawText(&w, &f, &r.d2d(), &b, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL) };
    }

    pub fn glyph(&self, g: char, r: R, size: f32, c: u32, a: f32) {
        let (Some(f), Some(b)) = (self.fonts.get(size, false, Align::Center, true), self.brush(c, a)) else { return };
        let w: Vec<u16> = g.to_string().encode_utf16().collect();
        unsafe { self.rt.DrawText(&w, &f, &r.d2d(), &b, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL) };
    }

    /// Glifi merkezi etrafında `deg` derece döndürerek çizer (açılır ok).
    pub fn glyph_rot(&self, g: char, r: R, size: f32, c: u32, a: f32, deg: f32) {
        unsafe {
            let mut old = windows::Foundation::Numerics::Matrix3x2::default();
            self.rt.GetTransform(&mut old);
            let rot = windows::Foundation::Numerics::Matrix3x2::rotation(deg, r.x + r.w / 2.0, r.y + r.h / 2.0);
            self.rt.SetTransform(&(rot * old));
            self.glyph(g, r, size, c, a);
            self.rt.SetTransform(&old);
        }
    }

    /// Satır içi küçük anahtar (etiketsiz). Kapalıyken soluk. Topuz kayar,
    /// renk yumuşakça dolar; üstüne gelince topuz hafifçe büyür.
    pub fn switch(&mut self, x: f32, y: f32, on: bool, enabled: bool, act: A) {
        let sw = R::new(x, y, 34.0, 18.0);
        let k = if enabled { 1.0 } else { 0.35 };
        let hover = enabled && self.hover(sw.inset(-4.0));
        let id = tween::key(("sw", x as i32, y as i32));
        let t = tween::to(id, if on { 1.0 } else { 0.0 }, 16.0);
        let hv = tween::to(id ^ 1, if hover { 1.0 } else { 0.0 }, 20.0);
        // Kapalı zemin → vurgu rengi geçişi.
        self.round(sw, 9.0, 0xFFFFFF, (0.05 + 0.09 * hv) * (1.0 - t) * k);
        self.outline(sw, 9.0, 0xFFFFFF, 0.35 * (1.0 - t) * k);
        if t > 0.0 {
            self.round(sw, 9.0, self.accent, (1.0 - 0.15 * hv) * t * k);
        }
        let ks = 10.0 + 2.0 * hv;
        let kx = sw.x + 4.0 + 16.0 * t - (ks - 10.0) / 2.0;
        let kc = mix(TEXT, 0x000000, t);
        self.round(R::new(kx, sw.y + 9.0 - ks / 2.0, ks, ks), ks / 2.0, kc, (0.6 + 0.25 * t) * k.max(t));
        if enabled {
            self.hit(sw.inset(-4.0), act);
        }
    }

    /// 0..100 değerlerden alan grafiği (sağ uç = şimdi), ızgara çizgileriyle.
    /// `cap`: grafiğin tam genişliğinde gösterilecek örnek sayısı.
    pub fn graph(&self, cap: usize, r: R, values: &std::collections::VecDeque<f32>, color: u32) {
        self.round(r, 6.0, 0x000000, 0.18);
        for i in 1..4 {
            let y = r.y + r.h * i as f32 / 4.0;
            self.fill(R::new(r.x, y, r.w, 1.0), 0xFFFFFF, 0.05);
        }
        self.outline(r, 6.0, 0xFFFFFF, 0.07);
        if values.len() < 2 {
            return;
        }
        let n = cap.max(2) as f32 - 1.0;
        let pt = |i: usize, v: f32| D2D_POINT_2F { x: r.x + r.w - (values.len() - 1 - i) as f32 / n * r.w, y: r.y + r.h - v.clamp(0.0, 100.0) / 100.0 * (r.h - 2.0) - 1.0 };
        unsafe {
            let Ok(factory) = self.rt.GetFactory() else { return };
            let (Ok(area), Ok(line)) = (factory.CreatePathGeometry(), factory.CreatePathGeometry()) else { return };
            for (geo, closed) in [(&area, true), (&line, false)] {
                let Ok(sink) = geo.Open() else { continue };
                let first = pt(0, values[0]);
                sink.BeginFigure(if closed { D2D_POINT_2F { x: first.x, y: r.bottom() } } else { first }, if closed { D2D1_FIGURE_BEGIN_FILLED } else { D2D1_FIGURE_BEGIN_HOLLOW });
                if closed {
                    sink.AddLine(first);
                }
                for (i, v) in values.iter().enumerate().skip(1) {
                    sink.AddLine(pt(i, *v));
                }
                if closed {
                    sink.AddLine(D2D_POINT_2F { x: r.right(), y: r.bottom() });
                }
                sink.EndFigure(if closed { D2D1_FIGURE_END_CLOSED } else { D2D1_FIGURE_END_OPEN });
                let _ = sink.Close();
            }
            self.rt.PushAxisAlignedClip(&r.d2d(), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            if let Some(b) = self.brush(color, 0.22) {
                self.rt.FillGeometry(&area, &b, None);
            }
            if let Some(b) = self.brush(color, 0.95) {
                self.rt.DrawGeometry(&line, &b, 1.6, None);
            }
            self.rt.PopAxisAlignedClip();
        }
    }

    /// Yatay doluluk çubuğu (0..1).
    pub fn bar(&self, r: R, frac: f32, color: u32) {
        self.round(r, r.h / 2.0, 0xFFFFFF, 0.08);
        let w = (r.w * frac.clamp(0.0, 1.0)).max(if frac > 0.0 { r.h } else { 0.0 });
        if w > 0.0 {
            self.round(R::new(r.x, r.y, w, r.h), r.h / 2.0, color, 0.9);
        }
    }

    /// Düğme; `danger` kırmızı yazı.
    pub fn button(&mut self, r: R, glyph: Option<char>, label: &str, act: A, enabled: bool, danger: bool) {
        let h = tween::to(tween::key(("btn", r.x as i32, r.y as i32)), if enabled && self.hover(r) { 1.0 } else { 0.0 }, 18.0);
        self.round(r, 6.0, 0xFFFFFF, 0.06 + 0.06 * h);
        self.outline(r, 6.0, if h > 0.0 { mix(0xFFFFFF, self.accent, h) } else { 0xFFFFFF }, 0.08 + 0.25 * h);
        let a = if enabled { 0.92 } else { 0.35 };
        let c = if danger && enabled { RED } else { TEXT };
        let tw = self.fonts.width(label, 13.0, false);
        let gw = if glyph.is_some() { 22.0 } else { 0.0 };
        let x = r.x + (r.w - tw - gw) / 2.0;
        if let Some(g) = glyph {
            self.glyph(g, R::new(x, r.y, 18.0, r.h), 13.0, c, a);
        }
        self.text(label, R::new(x + gw, r.y, tw + 4.0, r.h), 13.0, false, c, a, Align::Left);
        if enabled {
            self.hit(r, act);
        }
    }

    /// Anahtar (switch) + etiket; tıklanabilir alan ikisini de kapsar.
    pub fn toggle(&mut self, x: f32, y: f32, label: &str, on: bool, act: A) -> f32 {
        let sw = R::new(x, y + 4.0, 36.0, 18.0);
        let tw = self.fonts.width(label, 13.0, false);
        let all = R::new(x, y, 36.0 + 10.0 + tw, 26.0);
        if on {
            self.round(sw, 9.0, self.accent, 1.0);
        } else {
            self.round(sw, 9.0, 0xFFFFFF, if self.hover(all) { 0.16 } else { 0.08 });
            self.outline(sw, 9.0, 0xFFFFFF, 0.45);
        }
        let k = if on { sw.x + 21.0 } else { sw.x + 4.0 };
        self.round(R::new(k, sw.y + 4.0, 10.0, 10.0), 5.0, if on { 0x000000 } else { TEXT }, if on { 0.85 } else { 0.8 });
        self.text(label, R::new(x + 46.0, y, tw + 4.0, 26.0), 13.0, false, TEXT, 0.9, Align::Left);
        self.hit(all, act);
        all.w
    }

    /// Bölümlü seçici; seçili olan vurgu renginde. Vurgu seçenekler arasında
    /// kayar, üstüne gelinen hücre yumuşakça aydınlanır.
    pub fn segments(&mut self, r: R, items: &[(&str, A)], selected: usize) {
        self.round(r, 7.0, 0xFFFFFF, 0.06);
        self.outline(r, 7.0, 0xFFFFFF, 0.08);
        let w = r.w / items.len() as f32;
        let id = tween::key(("seg", r.x as i32, r.y as i32));
        let has = selected < items.len();
        let pos = tween::to(id, if has { selected as f32 } else { -1.0 }, 15.0);
        let vis = tween::to(id ^ 1, if has { 1.0 } else { 0.0 }, 15.0);
        for (i, _) in items.iter().enumerate() {
            let cell = R::new(r.x + w * i as f32, r.y, w, r.h).inset(3.0);
            let hv = tween::to(id ^ (i as u64 + 7) << 8, if self.hover(cell) && i != selected { 1.0 } else { 0.0 }, 20.0);
            if hv > 0.0 {
                self.round(cell, 5.0, 0xFFFFFF, 0.08 * hv);
            }
        }
        if vis > 0.0 && pos >= 0.0 {
            let pill = R::new(r.x + w * pos, r.y, w, r.h).inset(3.0);
            self.round(pill, 5.0, self.accent, 0.9 * vis);
        }
        for (i, (label, act)) in items.iter().enumerate() {
            let cell = R::new(r.x + w * i as f32, r.y, w, r.h).inset(3.0);
            // Yazı rengi vurgu altına girdikçe koyulaşır.
            let under = (1.0 - (pos - i as f32).abs()).clamp(0.0, 1.0) * vis;
            self.text(label, cell, 13.0, i == selected, mix(TEXT, 0x000000, under), 0.9, Align::Center);
            self.hit(cell, act.clone());
        }
    }
}
