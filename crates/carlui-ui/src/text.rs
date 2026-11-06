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
        self.layout(s, "Segoe UI Variable Text", size, bold, 100_000.0)
            .and_then(|(l, _, _)| unsafe {
                let mut m = DWRITE_TEXT_METRICS::default();
                l.GetMetrics(&mut m)?;
                Ok(m.widthIncludingTrailingWhitespace)
            })
            .unwrap_or(0.0)
    }

    /// Metni beyaz (opaklık `alpha`) olarak kendi boyutundaki bir yüzeye çizer.
    pub fn render(&self, gfx: &Gfx, s: &str, size: f32, bold: bool, max_w: f32, alpha: f32) -> Result<Rendered> {
        self.render_font(gfx, s, "Segoe UI Variable Text", size, bold, max_w, alpha)
    }

    /// Simge fontundan bir karakter (ör. "Segoe Fluent Icons" içindeki Wi-Fi simgesi).
    pub fn glyph(&self, gfx: &Gfx, code: char, size: f32, alpha: f32) -> Result<Rendered> {
        self.render_font(gfx, &code.to_string(), "Segoe Fluent Icons", size, false, size * 2.0, alpha)
    }

    /// Çok satırlı metin bloğu (pano önizlemesi). `wrap`: bu genişlikte satır
    /// kaydır; None ise satırlar olduğu gibi kalır (yatay kaydırılarak okunur).
    /// Yüzey en fazla `max`×`max` piksel olur; taşan kısım kesilir.
    pub fn render_block(&self, gfx: &Gfx, s: &str, size: f32, wrap: Option<f32>, max: f32, alpha: f32) -> Result<Rendered> {
        unsafe {
            let format = self.factory.CreateTextFormat(
                w!("Segoe UI Variable Text"),
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
                w!("Segoe UI Variable Text"),
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
