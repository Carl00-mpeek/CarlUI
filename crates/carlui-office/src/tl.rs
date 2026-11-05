//! Bu modül "biçimli paragrafı (kalın, italik, punto, renk karışık) doğru
//! satırlara bölüp çizelim, tıklanan yeri ve imlecin yerini bulalım" sorununu
//! çözüyor. DirectWrite metin düzeni (IDWriteTextLayout) kullanılır; konumlar
//! modelde karakter, DirectWrite'ta UTF-16 olduğu için burada çevrilir.

use crate::rich::{Fmt, PAlign, Para};
use carlui_ui::paint::{rgb, R};
use windows::{
    core::*,
    Win32::{
        Foundation::BOOL,
        Graphics::{
            Direct2D::{Common::*, *},
            DirectWrite::*,
        },
    },
};

thread_local! {
    static DW: Option<IDWriteFactory> = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok() };
}

pub fn factory() -> Option<IDWriteFactory> {
    DW.with(|d| d.clone())
}

pub struct Laid {
    pub layout: IDWriteTextLayout,
    pub width: f32,
    pub height: f32,
    /// karakter dizini → UTF-16 dizini (son eleman = toplam).
    map: Vec<u32>,
    /// Önek (madde işareti) uzunluğu, UTF-16.
    pre: u32,
}

fn wide(s: &str) -> (Vec<u16>, Vec<u32>) {
    let mut w = Vec::with_capacity(s.len());
    let mut map = Vec::with_capacity(s.len() + 1);
    for c in s.chars() {
        map.push(w.len() as u32);
        let mut b = [0u16; 2];
        w.extend_from_slice(c.encode_utf16(&mut b));
    }
    map.push(w.len() as u32);
    (w, map)
}

pub struct Style<'a> {
    pub family: &'a str,
    pub base: Fmt,
    /// Punto → piksel çarpanı (yakınlaştırma dahil).
    pub px_per_pt: f32,
    pub align: PAlign,
    pub width: f32,
    /// Satır aralığı çarpanı (1.0 = tek).
    pub line: f32,
}

/// Tek parça düz metin düzeni (tablo hücresi, slayt başlığı…).
pub fn plain(rt: &ID2D1RenderTarget, text: &str, st: &Style) -> Option<Laid> {
    let p = Para { runs: vec![crate::rich::Run::plain(text, Fmt::default())], ..Default::default() };
    para(rt, &p, st, None)
}

pub fn para(rt: &ID2D1RenderTarget, p: &Para, st: &Style, prefix: Option<&str>) -> Option<Laid> {
    let dw = factory()?;
    let pre_s = prefix.map(|s| format!("{s}\t")).unwrap_or_default();
    let body: String = p.runs.iter().map(|r| r.text.as_str()).collect();
    let (pw, _) = wide(&pre_s);
    let (bw, map) = wide(&body);
    let mut all = pw.clone();
    all.extend_from_slice(&bw);
    let pre = pw.len() as u32;
    let size0 = st.base.size.unwrap_or(11.0) * st.px_per_pt;
    let fam = HSTRING::from(st.family);
    unsafe {
        let tf = dw.CreateTextFormat(&fam, None, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size0.max(1.0), w!("tr-tr")).ok()?;
        let _ = tf.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP);
        let _ = tf.SetTextAlignment(match st.align {
            PAlign::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
            PAlign::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
            PAlign::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
            PAlign::Justify => DWRITE_TEXT_ALIGNMENT_JUSTIFIED,
        });
        if pre > 0 {
            let _ = tf.SetIncrementalTabStop(size0 * 1.6);
        }
        let layout = dw.CreateTextLayout(&all, &tf, st.width.max(1.0), 100_000.0).ok()?;
        if (st.line - 1.0).abs() > 0.01 {
            let _ = layout.SetLineSpacing(DWRITE_LINE_SPACING_METHOD_PROPORTIONAL, st.line, 0.8);
        }
        let mut c = 0usize;
        for r in &p.runs {
            let n = r.text.chars().count();
            if n == 0 {
                continue;
            }
            let f = r.fmt.over(&st.base);
            let range = DWRITE_TEXT_RANGE { startPosition: pre + map[c], length: map[c + n] - map[c] };
            c += n;
            if f.b == Some(true) {
                let _ = layout.SetFontWeight(DWRITE_FONT_WEIGHT_BOLD, range);
            }
            if f.i == Some(true) {
                let _ = layout.SetFontStyle(DWRITE_FONT_STYLE_ITALIC, range);
            }
            if f.u == Some(true) || r.atom.as_deref().is_some_and(|a| a.contains("hyperlink")) {
                let _ = layout.SetUnderline(true, range);
            }
            if f.strike == Some(true) {
                let _ = layout.SetStrikethrough(true, range);
            }
            if let Some(s) = f.size {
                let _ = layout.SetFontSize((s * st.px_per_pt).max(1.0), range);
            }
            let color = if r.atom.as_deref().is_some_and(|a| a.contains("hyperlink")) && r.fmt.color.is_none() { Some(0x0563C1) } else { f.color };
            if let Some(col) = color {
                if let Ok(b) = rt.CreateSolidColorBrush(&rgb(col, 1.0), None) {
                    if let Ok(u) = b.cast::<IUnknown>() {
                        let _ = layout.SetDrawingEffect(&u, range);
                    }
                }
            }
        }
        if pre > 0 {
            let range = DWRITE_TEXT_RANGE { startPosition: 0, length: pre };
            if let Some(s) = p.runs.first().and_then(|r| r.fmt.over(&st.base).size) {
                let _ = layout.SetFontSize((s * st.px_per_pt).max(1.0), range);
            }
        }
        let mut m = DWRITE_TEXT_METRICS::default();
        layout.GetMetrics(&mut m).ok()?;
        Some(Laid { layout, width: m.widthIncludingTrailingWhitespace, height: m.height, map, pre })
    }
}

impl Laid {
    pub fn draw(&self, rt: &ID2D1RenderTarget, x: f32, y: f32, color: u32, alpha: f32) {
        unsafe {
            if let Ok(b) = rt.CreateSolidColorBrush(&rgb(color, alpha), None) {
                rt.DrawTextLayout(D2D_POINT_2F { x, y }, &self.layout, &b, D2D1_DRAW_TEXT_OPTIONS_NONE);
            }
        }
    }

    fn to_char(&self, u: u32) -> usize {
        let u = u.saturating_sub(self.pre);
        match self.map.binary_search(&u) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
    }

    fn to_u16(&self, c: usize) -> u32 {
        self.pre + self.map[c.min(self.map.len() - 1)]
    }

    pub fn chars(&self) -> usize {
        self.map.len() - 1
    }

    /// Düzene göre (x, y) noktasındaki karakter konumu.
    pub fn hit(&self, x: f32, y: f32) -> usize {
        unsafe {
            let (mut trailing, mut inside) = (BOOL(0), BOOL(0));
            let mut m = DWRITE_HIT_TEST_METRICS::default();
            if self.layout.HitTestPoint(x, y, &mut trailing, &mut inside, &mut m).is_err() {
                return 0;
            }
            let pos = m.textPosition + if trailing.as_bool() { m.length } else { 0 };
            if pos < self.pre {
                return 0;
            }
            self.to_char(pos).min(self.chars())
        }
    }

    /// İmlecin düzen içindeki yeri: (x, y, yükseklik).
    pub fn caret(&self, c: usize) -> (f32, f32, f32) {
        unsafe {
            let (mut x, mut y) = (0.0f32, 0.0f32);
            let mut m = DWRITE_HIT_TEST_METRICS::default();
            if self.layout.HitTestTextPosition(self.to_u16(c), false, &mut x, &mut y, &mut m).is_err() {
                return (0.0, 0.0, 16.0);
            }
            (x, m.top, m.height)
        }
    }

    pub fn sel_rects(&self, a: usize, b: usize) -> Vec<R> {
        let (s, e) = (self.to_u16(a), self.to_u16(b));
        if e <= s {
            return Vec::new();
        }
        unsafe {
            let mut n = 0u32;
            let _ = self.layout.HitTestTextRange(s, e - s, 0.0, 0.0, None, &mut n);
            let mut v = vec![DWRITE_HIT_TEST_METRICS::default(); n as usize];
            if self.layout.HitTestTextRange(s, e - s, 0.0, 0.0, Some(&mut v), &mut n).is_err() {
                return Vec::new();
            }
            v.iter().map(|m| R::new(m.left, m.top, m.width.max(3.0), m.height)).collect()
        }
    }

    /// Satır başına/sonuna git: (satır başı, satır sonu) karakter konumları.
    pub fn line_bounds(&self, c: usize) -> (usize, usize) {
        unsafe {
            let mut n = 0u32;
            let _ = self.layout.GetLineMetrics(None, &mut n);
            let mut v = vec![DWRITE_LINE_METRICS::default(); n as usize];
            if self.layout.GetLineMetrics(Some(&mut v), &mut n).is_err() {
                return (0, self.chars());
            }
            let u = self.to_u16(c);
            let mut start = 0u32;
            for (i, l) in v.iter().enumerate() {
                let end = start + l.length;
                if u < end || i + 1 == v.len() {
                    let e = (end - l.newlineLength).max(start);
                    return (self.to_char(start.max(self.pre)), self.to_char(e).min(self.chars()));
                }
                start = end;
            }
            (0, self.chars())
        }
    }
}
