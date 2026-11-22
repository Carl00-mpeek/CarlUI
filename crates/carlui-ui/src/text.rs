//! Metin: DirectWrite ile ölçülür, Direct2D ile bir kez yüzeye çizilir.
//! Etiketler ve önizleme başlıkları için. Uzun metin "…" ile kısaltılır.

use crate::gfx::Gfx;
use windows::{
    core::*,
    UI::Composition::CompositionDrawingSurface,
    Win32::Graphics::{
        Direct2D::{Common::*, *},
        DirectWrite::*,
    },
};

pub struct Text {
    factory: IDWriteFactory,
}

pub struct Rendered {
    pub surface: CompositionDrawingSurface,
    pub width: f32,
    pub height: f32,
}

/// Metnin mürekkep kutusu ve taban çizgisi, çizilen yüzeyin koordinatında.
#[derive(Clone, Copy, Debug)]
pub struct Ink {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub baseline: f32,
    /// Büyük harf yüksekliği; karışık metni (ör. indirme hapı) zıplatmadan ortalamak için.
    pub cap: f32,
}

impl Ink {
    pub fn mid_x(&self) -> f32 {
        (self.left + self.right) / 2.0
    }
    pub fn mid_y(&self) -> f32 {
        (self.top + self.bottom) / 2.0
    }
}

/// Çok satırlı yazı kutusu metni: yüzey, imlecin yüzeydeki konumu, satır
/// yüksekliği ve (tek satırda) metnin gerçek genişliği.
pub struct FieldText {
    pub text: Rendered,
    pub caret: (f32, f32),
    pub line_h: f32,
    pub text_w: f32,
}

impl Text {
    pub fn new() -> Result<Self> {
        Ok(Self { factory: unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? } })
    }

    fn layout(&self, s: &str, family: &str, size: f32, bold: bool, max_w: f32) -> Result<(IDWriteTextLayout, f32, f32)> {
        unsafe {
            let weight = if bold { DWRITE_FONT_WEIGHT_SEMI_BOLD } else { DWRITE_FONT_WEIGHT_NORMAL };
            let format = self.factory.CreateTextFormat(
                &HSTRING::from(family),
                None,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("tr-tr"),
            )?;
            format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            let sign = self.factory.CreateEllipsisTrimmingSign(&format)?;
            let trimming = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
            format.SetTrimming(&trimming, &sign)?;
            let wide: Vec<u16> = s.encode_utf16().collect();
            let layout = self.factory.CreateTextLayout(&wide, &format, max_w, size * 2.0)?;
            let mut m = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut m)?;
            Ok((layout, m.widthIncludingTrailingWhitespace.min(max_w).ceil(), m.height.ceil()))
        }
    }

    /// Metnin genişliği (sondaki boşluklar dahil); imleç konumu için.
    pub fn measure(&self, s: &str, size: f32, bold: bool) -> f32 {
        if s.is_empty() {
            return 0.0;
        }
        self.layout(s, carlui_core::os::ui_font(), size, bold, 100_000.0)
            .and_then(|(l, _, _)| unsafe {
                let mut m = DWRITE_TEXT_METRICS::default();
                l.GetMetrics(&mut m)?;
                Ok(m.widthIncludingTrailingWhitespace)
            })
            .unwrap_or(0.0)
    }

    /// `render` ile çizilen metnin mürekkep kutusu (sol, üst, sağ, alt), yüzey
    /// koordinatında. Yazı kutusu satır aralığını da içerdiğinden, rozet gibi
    /// küçük zeminlerde rakamı gözle ortalamak için bu kutu kullanılır.
    /// Glifin tasarım ölçülerinden hesaplanır (satır yüksekliğinden değil).
    pub fn ink(&self, s: &str, size: f32, bold: bool, max_w: f32) -> Option<Ink> {
        if s.is_empty() {
            return None;
        }
        let family = carlui_core::os::ui_font();
        let (layout, _, _) = self.layout(s, family, size, bold, max_w).ok()?;
        unsafe {
            let mut line = [DWRITE_LINE_METRICS::default()];
            let mut count = 0;
            layout.GetLineMetrics(Some(&mut line), &mut count).ok()?;
            let baseline = line[0].baseline;

            let mut coll = None;
            self.factory.GetSystemFontCollection(&mut coll, false).ok()?;
            let coll = coll?;
            let (mut idx, mut exists) = (0u32, windows::Win32::Foundation::BOOL(0));
            coll.FindFamilyName(&HSTRING::from(family), &mut idx, &mut exists).ok()?;
            if !exists.as_bool() {
                return None;
            }
            let weight = if bold { DWRITE_FONT_WEIGHT_SEMI_BOLD } else { DWRITE_FONT_WEIGHT_NORMAL };
            let face = coll.GetFontFamily(idx).ok()?.GetFirstMatchingFont(weight, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL).ok()?.CreateFontFace().ok()?;
            let mut fm = DWRITE_FONT_METRICS::default();
            face.GetMetrics(&mut fm);
            let k = size / fm.designUnitsPerEm.max(1) as f32;

            let cps: Vec<u32> = s.chars().map(u32::from).collect();
            let mut glyphs = vec![0u16; cps.len()];
            face.GetGlyphIndices(cps.as_ptr(), cps.len() as u32, glyphs.as_mut_ptr()).ok()?;
            if glyphs.contains(&0) {
                return None;
            }
            let mut gm = vec![DWRITE_GLYPH_METRICS::default(); glyphs.len()];
            face.GetDesignGlyphMetrics(glyphs.as_ptr(), glyphs.len() as u32, gm.as_mut_ptr(), false).ok()?;

            let (mut pen, mut l, mut r, mut top, mut bottom) = (0.0f32, f32::MAX, f32::MIN, f32::MAX, f32::MIN);
            for (c, g) in s.chars().zip(&gm) {
                let adv = g.advanceWidth as f32;
                if !c.is_whitespace() {
                    l = l.min(pen + g.leftSideBearing as f32 * k);
                    r = r.max(pen + (adv - g.rightSideBearing as f32) * k);
                    // Taban çizgisinden yukarı: yMax = vOrigin - tsb, yMin = vOrigin - advH + bsb.
                    let y_max = (g.verticalOriginY - g.topSideBearing) as f32 * k;
                    let y_min = (g.verticalOriginY - g.advanceHeight as i32 + g.bottomSideBearing) as f32 * k;
                    top = top.min(baseline - y_max);
                    bottom = bottom.max(baseline - y_min);
                }
                pen += adv * k;
            }
            (r > l && bottom > top).then_some(Ink { left: l, top, right: r, bottom, baseline, cap: fm.capHeight as f32 * k })
        }
    }

    /// Metni beyaz (opaklık `alpha`) olarak kendi boyutundaki bir yüzeye çizer.
    pub fn render(&self, gfx: &Gfx, s: &str, size: f32, bold: bool, max_w: f32, alpha: f32) -> Result<Rendered> {
        self.render_font(gfx, s, carlui_core::os::ui_font(), size, bold, max_w, alpha)
    }

    /// Simge fontundan bir karakter (ör. "Segoe Fluent Icons" içindeki Wi-Fi simgesi).
    pub fn glyph(&self, gfx: &Gfx, code: char, size: f32, alpha: f32) -> Result<Rendered> {
        self.render_font(gfx, &carlui_core::os::glyph(code).to_string(), carlui_core::os::icon_font(), size, false, size * 2.0, alpha)
    }

    /// Çok satırlı metin bloğu (pano önizlemesi). `wrap`: bu genişlikte satır
    /// kaydır; None ise satırlar olduğu gibi kalır (yatay kaydırılarak okunur).
    /// Yüzey en fazla `max`×`max` piksel olur; taşan kısım kesilir.
    pub fn render_block(&self, gfx: &Gfx, s: &str, size: f32, wrap: Option<f32>, max: f32, alpha: f32) -> Result<Rendered> {
        unsafe {
            let format = self.factory.CreateTextFormat(
                carlui_core::os::ui_font_w(),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("tr-tr"),
            )?;
            format.SetWordWrapping(if wrap.is_some() { DWRITE_WORD_WRAPPING_EMERGENCY_BREAK } else { DWRITE_WORD_WRAPPING_NO_WRAP })?;
            let wide: Vec<u16> = s.encode_utf16().collect();
            let layout = self.factory.CreateTextLayout(&wide, &format, wrap.unwrap_or(max), max)?;
            let mut m = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut m)?;
            let w = m.widthIncludingTrailingWhitespace.ceil().clamp(1.0, max);
            let h = m.height.ceil().clamp(1.0, max);
            let surface = gfx.surface(w, h, |dc| {
                dc.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
                let (r, g, b) = crate::tone::unit(1.0, 1.0, 1.0);
                let brush = dc.CreateSolidColorBrush(&D2D1_COLOR_F { r, g, b, a: crate::tone::ink_alpha(alpha) }, None)?;
                dc.DrawTextLayout(D2D_POINT_2F { x: 0.0, y: 0.0 }, &layout, &brush, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT);
                Ok(())
            })?;
            Ok(Rendered { surface, width: w, height: h })
        }
    }

    /// Yazı kutusu metni: `max_w` genişlikte kelimeden satır kaydırır, en fazla
    /// `max_lines` satır gösterir (fazlasında imlecin olduğu satırlar görünür).
    pub fn render_field(&self, gfx: &Gfx, s: &str, size: f32, max_w: f32, max_lines: u32, caret: usize, alpha: f32) -> Result<FieldText> {
        unsafe {
            let format = self.factory.CreateTextFormat(
                carlui_core::os::ui_font_w(),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("tr-tr"),
            )?;
            format.SetWordWrapping(DWRITE_WORD_WRAPPING_EMERGENCY_BREAK)?;
            let wide: Vec<u16> = s.encode_utf16().collect();
            let layout = self.factory.CreateTextLayout(&wide, &format, max_w, 100_000.0)?;
            let mut m = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut m)?;
            let lines = m.lineCount.max(1);
            let line_h = (m.height / lines as f32).ceil();
            let pos = s.chars().take(caret).map(char::len_utf16).sum::<usize>() as u32;
            let (mut cx, mut cy) = (0.0f32, 0.0f32);
            let mut hm = DWRITE_HIT_TEST_METRICS::default();
            layout.HitTestTextPosition(pos, false, &mut cx, &mut cy, &mut hm)?;
            let shown = lines.min(max_lines);
            let caret_line = ((cy + line_h / 2.0) / line_h).floor() as u32;
            let first = caret_line.saturating_sub(shown - 1).min(lines - shown);
            let dy = first as f32 * line_h;
            let w = max_w.ceil().max(1.0);
            let h = (shown as f32 * line_h).max(1.0);
            let surface = gfx.surface(w, h, |dc| {
                dc.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
                let (r, g, b) = crate::tone::unit(1.0, 1.0, 1.0);
                let brush = dc.CreateSolidColorBrush(&D2D1_COLOR_F { r, g, b, a: crate::tone::ink_alpha(alpha) }, None)?;
                dc.DrawTextLayout(D2D_POINT_2F { x: 0.0, y: -dy }, &layout, &brush, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT);
                Ok(())
            })?;
            let text_w = if lines == 1 { m.widthIncludingTrailingWhitespace.ceil().min(w) } else { w };
            Ok(FieldText { text: Rendered { surface, width: w, height: h }, caret: (cx, cy - dy), line_h, text_w })
        }
    }

    pub fn render_font(&self, gfx: &Gfx, s: &str, family: &str, size: f32, bold: bool, max_w: f32, alpha: f32) -> Result<Rendered> {
        let (layout, w, h) = self.layout(s, family, size, bold, max_w)?;
        let (sw, sh) = (w.max(1.0), h.max(1.0));
        let surface = gfx.surface(sw, sh, |dc| unsafe {
            dc.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            let (r, g, b) = crate::tone::unit(1.0, 1.0, 1.0);
                let brush = dc.CreateSolidColorBrush(&D2D1_COLOR_F { r, g, b, a: crate::tone::ink_alpha(alpha) }, None)?;
            dc.DrawTextLayout(D2D_POINT_2F { x: 0.0, y: 0.0 }, &layout, &brush, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT);
            Ok(())
        })?;
        Ok(Rendered { surface, width: sw, height: sh })
    }
}

#[cfg(test)]
mod ink_tests {
    use super::*;

    #[test]
    fn digit_ink_sits_inside_the_box() {
        let t = Text::new().unwrap();
        for s in ["1", "9+", "4:10 PM"] {
            let (_, w, h) = t.layout(s, carlui_core::os::ui_font(), 20.0, true, 200.0).unwrap();
            let i = t.ink(s, 20.0, true, 200.0).expect("ink");
            println!("{s}: box {w}x{h} ink {i:?}");
            assert!(i.left >= -1.0 && i.right <= w + 1.0 && i.top >= 0.0 && i.bottom <= h);
            assert!((i.baseline - i.cap / 2.0 - i.mid_y()).abs() < 1.5);
        }
    }
}
