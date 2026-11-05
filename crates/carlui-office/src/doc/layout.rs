//! Bu modül "Word belgesini sayfalara dizip çizelim" sorununu çözüyor:
//! her paragraf DirectWrite ile bir kez dizilir (önbellekte tutulur, yalnızca
//! değişen paragraf yeniden dizilir), sonra yükseklikleri toplanarak sayfalara
//! yerleştirilir. Tablolar, yalnızca resimden oluşan paragraflar ve sayfa sonları
//! da burada. Koordinatlar "belge pikseli": x sayfanın sol kenarından, y ilk
//! sayfanın tepesinden.

use super::{Block, Document};
use crate::{
    app::Act,
    img,
    rich::{Fmt, Para},
    tl::{self, Laid},
};
use carlui_ui::paint::{Painter, R};
use windows::Win32::Graphics::Direct2D::ID2D1RenderTarget;

/// Sayfalar arası boşluk (px).
pub const GAP: f32 = 18.0;
const CELL_PAD: f32 = 5.0;

pub struct CellLay {
    pub x: f32,
    pub w: f32,
    pub fill: Option<u32>,
    pub lines: Vec<(Laid, f32)>,
}

pub struct RowLay {
    pub h: f32,
    pub cells: Vec<CellLay>,
}

#[derive(Default)]
pub struct Lay {
    pub laid: Option<Laid>,
    /// İçerik yüksekliği (boşluklar hariç).
    pub h: f32,
    pub before: f32,
    pub after: f32,
    /// Sol girinti (liste düzeyi / w:ind).
    pub indent: f32,
    pub images: Vec<(String, f32, f32)>,
    pub rows: Vec<RowLay>,
    pub break_after: bool,
    pub break_before: bool,
    /// İmleç girmeyen blok (tablo vb.).
    pub block: bool,
    /// Dizildiği andaki madde işareti (numara değişince yeniden dizilir).
    pub bullet: Option<String>,
}

/// Ham XML'de `<tag ... attr="…"` değeri.
fn tag_attr(xml: &str, tag: &str, attr: &str) -> Option<f32> {
    let i = xml.find(&format!("<{tag} "))?;
    let rest = &xml[i..];
    let open = &rest[..rest.find('>')?];
    let v = open.find(&format!(" {attr}=\""))? + attr.len() + 3;
    let e = open[v..].find('"')?;
    open[v..v + e].parse().ok()
}

pub struct Geo {
    /// pt → px.
    pub s: f32,
    pub page_w: f32,
    pub page_h: f32,
    pub left: f32,
    pub top: f32,
    pub text_w: f32,
    pub text_h: f32,
}

impl Geo {
    pub fn new(d: &Document, zoom: f32) -> Self {
        let s = 96.0 / 72.0 * zoom;
        let p = d.page;
        Geo { s, page_w: p.w * s, page_h: p.h * s, left: p.left * s, top: p.top * s, text_w: ((p.w - p.left - p.right) * s).max(40.0), text_h: ((p.h - p.top - p.bottom) * s).max(40.0) }
    }

    /// Sayfanın belge y'si.
    pub fn page_y(&self, page: usize) -> f32 {
        GAP + page as f32 * (self.page_h + GAP)
    }
}

fn text_style<'a>(d: &'a Document, p: &Para, s: f32, width: f32) -> (tl::Style<'a>, f32, f32) {
    let st = p.style.as_deref();
    let base = d.style_fmt(st);
    let family = if d.heading_of(st) > 0 { d.heading_font.as_str() } else { d.font.as_str() };
    let (mut before, mut after, mut line) = d.style_spacing(st);
    if let Some(b) = tag_attr(&p.ppr, "w:spacing", "w:before") {
        before = b / 20.0;
    }
    if let Some(a) = tag_attr(&p.ppr, "w:spacing", "w:after") {
        after = a / 20.0;
    }
    if let Some(l) = tag_attr(&p.ppr, "w:spacing", "w:line") {
        if !p.ppr.contains("w:lineRule=\"exact\"") && !p.ppr.contains("w:lineRule=\"atLeast\"") {
            line = l / 240.0;
        }
    }
    let align = p.align.unwrap_or_else(|| d.style_align(st));
    (tl::Style { family, base: Fmt { size: base.size.or(Some(11.0)), ..base }, px_per_pt: s, align, width, line: line.clamp(0.6, 4.0) }, before * s, after * s)
}

fn lay_cells(rt: &ID2D1RenderTarget, d: &Document, t: &super::Table, g: &Geo) -> Vec<RowLay> {
    let total: f32 = t.cols.iter().sum::<f32>() * g.s;
    let k = if total > g.text_w { g.text_w / total } else { 1.0 };
    let cols: Vec<f32> = t.cols.iter().map(|c| c * g.s * k).collect();
    let mut rows = Vec::new();
    for r in &t.rows {
        let mut x = 0.0;
        let mut ci = 0;
        let mut cells = Vec::new();
        let mut h: f32 = 18.0 * g.s / 1.333;
        for c in r {
            let w: f32 = (ci..ci + c.span).map(|i| cols.get(i).copied().unwrap_or(40.0)).sum();
            ci += c.span;
            let mut y = CELL_PAD;
            let mut lines = Vec::new();
            for p in &c.paras {
                let (st, before, after) = text_style(d, p, g.s, (w - CELL_PAD * 2.0).max(10.0));
                if let Some(l) = tl::para(rt, p, &st, p.bullet.as_deref()) {
                    y += before.min(6.0);
                    let lh = l.height;
                    lines.push((l, y));
                    y += lh + after.min(6.0);
                }
            }
            h = h.max(y + CELL_PAD);
            cells.push(CellLay { x, w, fill: c.fill, lines });
            x += w;
        }
        rows.push(RowLay { h, cells });
    }
    rows
}

pub fn lay_para(rt: &ID2D1RenderTarget, d: &Document, p: &Para, g: &Geo) -> Lay {
    let mut l = Lay { block: p.block.is_some(), bullet: p.bullet.clone(), ..Default::default() };
    match p.block.and_then(|b| d.blocks.get(b)) {
        Some(Block::Table(t)) => {
            l.rows = lay_cells(rt, d, t, g);
            l.h = l.rows.iter().map(|r| r.h).sum();
            l.after = 8.0 * g.s;
            return l;
        }
        Some(Block::Other(text)) => {
            let st = tl::Style { family: &d.font, base: Fmt { size: Some(10.0), i: Some(true), ..Default::default() }, px_per_pt: g.s, align: Default::default(), width: g.text_w, line: 1.0 };
            l.laid = tl::plain(rt, text, &st);
            l.h = l.laid.as_ref().map(|x| x.height).unwrap_or(0.0);
            l.after = 6.0 * g.s;
            return l;
        }
        None => {}
    }
    l.break_after = p.runs.iter().any(|r| r.atom.as_deref().is_some_and(|a| a.contains("w:type=\"page\"")));
    l.break_before = p.ppr.contains("<w:pageBreakBefore/>");
    let ind = tag_attr(&p.ppr, "w:ind", "w:left").or_else(|| tag_attr(&p.ppr, "w:ind", "w:start")).map(|t| t / 20.0 * g.s);
    l.indent = ind.unwrap_or(if p.bullet.is_some() { (p.level as f32 + 1.0) * 18.0 * g.s } else { 0.0 }).clamp(0.0, g.text_w * 0.6);
    let (st, before, after) = text_style(d, p, g.s, g.text_w - l.indent);
    l.before = before;
    l.after = after;
    let imgs = p.images_only();
    if !imgs.is_empty() {
        // Resim paragrafına imleç girmez (yazılırsa resim metne karışırdı).
        l.block = true;
        for (path, cx, cy) in imgs {
            let (mut w, mut h) = (cx as f32 / 914400.0 * 72.0 * g.s, cy as f32 / 914400.0 * 72.0 * g.s);
            if w > g.text_w {
                h *= g.text_w / w;
                w = g.text_w;
            }
            if h > g.text_h {
                w *= g.text_h / h;
                h = g.text_h;
            }
            l.h += h.max(4.0);
            l.images.push((path, w.max(4.0), h.max(4.0)));
        }
        return l;
    }
    // Madde işareti girintinin solunda durur.
    let prefix = p.bullet.as_deref().filter(|b| !b.is_empty());
    l.laid = tl::para(rt, p, &st, prefix);
    if prefix.is_some() {
        l.indent = (l.indent - 18.0 * g.s).max(0.0);
    }
    l.h = l.laid.as_ref().map(|x| x.height).unwrap_or(st.base.size.unwrap_or(11.0) * g.s * 1.2);
    l
}

/// Paragrafların yeri: (sayfa, sayfa içi y). Döndürülen ikinci değer sayfa sayısı.
pub fn paginate<'a>(lays: impl Iterator<Item = &'a Lay>, g: &Geo) -> (Vec<(usize, f32)>, usize) {
    let mut out = Vec::new();
    let (mut page, mut y) = (0usize, 0.0f32);
    for l in lays {
        if l.break_before && y > 0.0 {
            page += 1;
            y = 0.0;
        }
        let before = if y > 0.0 { l.before } else { 0.0 };
        if y > 0.0 && y + before + l.h > g.text_h {
            page += 1;
            y = 0.0;
        }
        let before = if y > 0.0 { l.before } else { 0.0 };
        out.push((page, y + before));
        y += before + l.h + l.after;
        if l.break_after {
            page += 1;
            y = 0.0;
        }
    }
    (out, page + 1)
}

/// Bir paragrafı (x0, y0) ekran noktasına çizer; `sel` imleç seçim aralığı.
#[allow(clippy::too_many_arguments)]
pub fn draw(p: &mut Painter<Act>, d: &Document, para: &Para, l: &Lay, g: &Geo, (x0, y0): (f32, f32), sel: Option<(usize, usize)>, imgs: &mut img::Cache) {
    let x = x0 + l.indent;
    if !l.rows.is_empty() {
        let mut y = y0;
        for r in &l.rows {
            for c in &r.cells {
                let cr = R::new(x0 + c.x, y, c.w, r.h);
                if let Some(f) = c.fill {
                    p.fill(cr, f, 1.0);
                }
                p.fill(R::new(cr.x, cr.y, cr.w, 1.0), 0x000000, 0.5);
                p.fill(R::new(cr.x, cr.y, 1.0, cr.h), 0x000000, 0.5);
                p.fill(R::new(cr.right() - 1.0, cr.y, 1.0, cr.h), 0x000000, 0.5);
                p.fill(R::new(cr.x, cr.bottom() - 1.0, cr.w, 1.0), 0x000000, 0.5);
                for (lt, ly) in &c.lines {
                    lt.draw(p.rt, cr.x + CELL_PAD, y + ly, 0x000000, 1.0);
                }
            }
            y += r.h;
        }
        return;
    }
    if !l.images.is_empty() {
        let mut y = y0;
        let align = para.align.unwrap_or_else(|| d.style_align(para.style.as_deref()));
        for (path, w, h) in &l.images {
            let free = (g.text_w - l.indent - w).max(0.0);
            let ix = x + match align {
                crate::rich::PAlign::Center => free / 2.0,
                crate::rich::PAlign::Right => free,
                _ => 0.0,
            };
            let r = R::new(ix, y, *w, *h);
            let zip = &d.zip;
            match imgs.get(p.rt, path, || zip.read(path)) {
                Some(bm) => unsafe {
                    p.rt.DrawBitmap(&bm, Some(&r.d2d()), 1.0, windows::Win32::Graphics::Direct2D::D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
                },
                None => {
                    p.fill(r, 0xE0E0E0, 1.0);
                    p.glyph('\u{EB9F}', r, 20.0, 0x808080, 1.0);
                }
            }
            if let Some((a, b)) = sel {
                if b > a {
                    p.fill(r, 0x3399FF, 0.3);
                }
            }
            y += h;
        }
        return;
    }
    let Some(t) = &l.laid else { return };
    if let Some((a, b)) = sel {
        for s in t.sel_rects(a, b) {
            p.fill(R::new(x + s.x, y0 + s.y, s.w, s.h), 0x3399FF, 0.35);
        }
    }
    let color = d.style_fmt(para.style.as_deref()).color.unwrap_or(0x000000);
    t.draw(p.rt, x, y0, color, 1.0);
}

