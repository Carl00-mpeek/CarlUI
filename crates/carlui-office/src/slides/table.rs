//! Bu modül "slayttaki tabloyu PowerPoint'teki gibi gösterelim" sorununu
//! çözüyor: hücrenin kendi dolgusu, kenarlıkları, yazı rengi/kalınlığı/boyutu
//! ve hizası okunur. Tablo biçemi (tableStyleId) varsa başlık satırı ve
//! bantlar tema vurgu rengiyle boyanır; hücrenin kendi biçimi her zaman önce
//! gelir. Satır yükseklikleri dosyadaki oranlarla dağıtılır.

use super::{pptx, render, Theme};
use crate::{
    rich::{Fmt, PAlign},
    tl,
    xml::Node,
};
use carlui_ui::paint::R;
use windows::Win32::Graphics::Direct2D::*;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cell {
    pub text: String,
    pub fmt: Fmt,
    pub align: PAlign,
    /// None: biçeme bırak; Some(None): dolgu yok; Some(Some(c)): renk.
    pub fill: Option<Option<u32>>,
    /// Sol, sağ, üst, alt kenarlık: (renk, kalınlık pt).
    pub lines: [Option<(u32, f32)>; 4],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    pub rows: Vec<Vec<Cell>>,
    pub cols: Vec<f32>,
    pub heights: Vec<f32>,
    pub styled: bool,
    pub first_row: bool,
    pub band_row: bool,
}

fn line(n: Option<&Node>, th: &Theme) -> Option<(u32, f32)> {
    let n = n?;
    if n.child("noFill").is_some() {
        return None;
    }
    let c = n.child("solidFill").and_then(|f| super::fill_color(f, th))?;
    let w = super::emu(n.attr("w")).unwrap_or(1.0).max(0.5);
    Some((c, w))
}

fn cell(c: &Node, src: &str, th: &Theme) -> Cell {
    let mut ps = Vec::new();
    c.find_all("p", &mut ps);
    let text = ps.iter().map(|p| p.all_text(src, "t")).collect::<Vec<_>>().join("\n");
    let fmt = c.find("rPr").map(|r| pptx::rpr_fmt(r, th)).unwrap_or_default();
    let align = c.find("pPr").and_then(|p| p.attr("algn")).and_then(pptx::algn).unwrap_or(PAlign::Left);
    let mut out = Cell { text, fmt, align, ..Default::default() };
    if let Some(pr) = c.child("tcPr") {
        if pr.child("noFill").is_some() {
            out.fill = Some(None);
        } else if let Some(f) = pr.child("solidFill") {
            out.fill = Some(super::fill_color(f, th));
        }
        for (i, k) in ["lnL", "lnR", "lnT", "lnB"].iter().enumerate() {
            out.lines[i] = line(pr.child(k), th);
        }
    }
    out
}

pub fn parse(t: &Node, src: &str, th: &Theme) -> Table {
    let cols = t.child("tblGrid").map(|g| g.children_named("gridCol").filter_map(|c| super::emu(c.attr("w"))).collect()).unwrap_or_default();
    let mut rows = Vec::new();
    let mut heights = Vec::new();
    for r in t.children_named("tr") {
        heights.push(super::emu(r.attr("h")).unwrap_or(0.0));
        rows.push(r.children_named("tc").map(|c| cell(c, src, th)).collect());
    }
    let pr = t.child("tblPr");
    let on = |k: &str| pr.and_then(|p| p.attr(k)).is_some_and(|v| v == "1" || v == "true");
    let styled = pr.and_then(|p| p.child("tableStyleId")).is_some();
    Table { rows, cols, heights, styled, first_row: styled && on("firstRow"), band_row: styled && on("bandRow") }
}

/// Satır yüksekliklerini `total` içine oranlarıyla dağıtır.
fn row_heights(t: &Table, total: f32) -> Vec<f32> {
    let n = t.rows.len().max(1);
    let known: Vec<f32> = t.heights.iter().copied().filter(|h| *h > 0.0).collect();
    let avg = if known.is_empty() { 1.0 } else { known.iter().sum::<f32>() / known.len() as f32 };
    let hs: Vec<f32> = (0..n).map(|i| t.heights.get(i).copied().filter(|h| *h > 0.0).unwrap_or(avg)).collect();
    let sum: f32 = hs.iter().sum::<f32>().max(0.001);
    hs.iter().map(|h| h / sum * total).collect()
}

pub fn draw(rt: &ID2D1RenderTarget, th: &Theme, t: &Table, r: R, k: f32) {
    if t.rows.is_empty() {
        return;
    }
    let total: f32 = t.cols.iter().sum::<f32>().max(1.0);
    let n = t.rows.iter().map(|x| x.len()).max().unwrap_or(1).max(1);
    let widths: Vec<f32> = (0..n).map(|i| t.cols.get(i).copied().unwrap_or(total / n as f32) / total * r.w).collect();
    let hs = row_heights(t, r.h);
    let accent = th.colors.get("accent1").copied().unwrap_or(0x4472C4);
    let dark = th.colors.get("tx1").or_else(|| th.colors.get("dk1")).copied().unwrap_or(0);
    let family = if th.minor.is_empty() { "Calibri" } else { &th.minor };
    let mut y = r.y;
    for (ri, row) in t.rows.iter().enumerate() {
        let rh = hs[ri];
        let head = t.first_row && ri == 0;
        let band = t.band_row && (ri + usize::from(t.first_row)) % 2 == 1;
        let style_bg = if !t.styled {
            None
        } else if head {
            Some(accent)
        } else if band {
            Some(render::mix(accent, 0xFFFFFF, 0.6))
        } else {
            Some(render::mix(accent, 0xFFFFFF, 0.8))
        };
        let mut x = r.x;
        for (ci, w) in widths.iter().enumerate() {
            let cr = R::new(x, y, *w, rh);
            let c = row.get(ci);
            let bg = match c.and_then(|c| c.fill) {
                Some(f) => f,
                None => style_bg,
            };
            if let Some(bg) = bg {
                render::fill_rect(rt, cr, bg, 1.0);
            }
            if t.styled && c.is_none_or(|c| c.lines.iter().all(Option::is_none)) {
                render::fill_rect(rt, R::new(cr.x, cr.bottom() - k, cr.w, k), 0xFFFFFF, 1.0);
                render::fill_rect(rt, R::new(cr.right() - k, cr.y, k, cr.h), 0xFFFFFF, 1.0);
            }
            if let Some(c) = c {
                for (i, l) in c.lines.iter().enumerate() {
                    let Some((col, lw)) = *l else { continue };
                    let lw = (lw * k).max(1.0);
                    let lr = match i {
                        0 => R::new(cr.x, cr.y, lw, cr.h),
                        1 => R::new(cr.right() - lw, cr.y, lw, cr.h),
                        2 => R::new(cr.x, cr.y, cr.w, lw),
                        _ => R::new(cr.x, cr.bottom() - lw, cr.w, lw),
                    };
                    render::fill_rect(rt, lr, col, 1.0);
                }
                if !c.text.is_empty() {
                    let fg = c.fmt.color.unwrap_or(if head { 0xFFFFFF } else { dark });
                    let base = Fmt { size: Some(c.fmt.size.unwrap_or(18.0)), b: Some(c.fmt.b.unwrap_or(head)), i: c.fmt.i, u: c.fmt.u, strike: c.fmt.strike, color: None };
                    let st = tl::Style { family, base, px_per_pt: k, align: c.align, width: (cr.w - 14.4 * k).max(4.0), line: 1.0 };
                    if let Some(l) = tl::plain(rt, &c.text, &st) {
                        unsafe { rt.PushAxisAlignedClip(&cr.d2d(), D2D1_ANTIALIAS_MODE_ALIASED) };
                        l.draw(rt, cr.x + 7.2 * k, cr.y + (rh - l.height).max(0.0) / 2.0, fg, 1.0);
                        unsafe { rt.PopAxisAlignedClip() };
                    }
                }
            }
            x += w;
        }
        y += rh;
    }
}
