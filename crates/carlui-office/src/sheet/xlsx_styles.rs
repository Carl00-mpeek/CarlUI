//! Bu modül "düzenlemede eklenen hücre stillerini (kalın, renk, hizalama,
//! sayı biçimi) Excel'in styles.xml dosyasına, var olan stilleri bozmadan
//! ekleyelim" sorununu çözüyor. Her yeni stil, kaynak stilin yazı tipi /
//! dolgu / kenarlığını kopyalar ve yalnızca değişeni yeniden yazar.

use super::{numfmt, Book, Look};
use crate::xml::{self, escape};

const FONT_ORDER: &[&str] = &["b", "i", "strike", "condense", "extend", "outline", "shadow", "u", "vertAlign", "sz", "color", "name", "family", "charset", "scheme"];

fn hex_argb(c: u32) -> String {
    format!("FF{c:06X}")
}

/// Aynı XML daha önce eklendiyse onun, değilse yeni eklenenin sırası.
fn intern(list: &mut Vec<String>, xml: String) -> usize {
    list.iter().position(|f| *f == xml).unwrap_or_else(|| {
        list.push(xml);
        list.len() - 1
    })
}

/// styles.xml'e `from..` arasındaki yeni stilleri ekler. Yeni stil, türediği
/// özgün stilin yazı tipini, dolgusunu, kenarlığını ve hizalamasını taşır;
/// yalnızca değişen özellik (kalın, renk, dolgu, hizalama, sayı biçimi) farklıdır.
pub(super) fn add_styles(src: &str, book: &Book, from: usize) -> Result<String, String> {
    let root = xml::parse(src)?;
    let mut edits = Vec::new();
    let mut next_id = 164u32;
    if let Some(nf) = root.child("numFmts") {
        for f in nf.children_named("numFmt") {
            if let Some(id) = f.attr("numFmtId").and_then(|x| x.parse::<u32>().ok()) {
                next_id = next_id.max(id + 1);
            }
        }
    }
    let fonts: Vec<&xml::Node> = root.child("fonts").map(|f| f.children_named("font").collect()).unwrap_or_default();
    let fills_n = root.child("fills").map(|f| f.children_named("fill").count()).unwrap_or(0);
    let Some(xfs) = root.child("cellXfs") else { return Err("cellXfs yok".into()) };
    let old_xfs: Vec<&xml::Node> = xfs.children_named("xf").collect();
    let mut new_fmts = String::new();
    let mut new_xfs = String::new();
    let mut new_fonts: Vec<String> = Vec::new();
    let mut new_fills: Vec<String> = Vec::new();
    let mut nf_count = 0;
    for i in from..book.numfmt.len() {
        let code = &book.numfmt[i];
        let id = (0..50).find(|id| numfmt::builtin(*id) == Some(code.as_str())).unwrap_or_else(|| {
            let id = next_id;
            next_id += 1;
            new_fmts.push_str(&format!("<numFmt numFmtId=\"{id}\" formatCode=\"{}\"/>", escape(code)));
            nf_count += 1;
            id
        });
        let look = book.look(i as u32);
        // Türediği stil dosyada yoksa (ör. xls'ten gelen) ilk stil ve varsayılan görünüm.
        let src_i = book.style_src.get(&(i as u32)).map(|x| *x as usize).unwrap_or(0);
        let (sx, base) = match old_xfs.get(src_i) {
            Some(x) => (Some(*x), book.look(src_i as u32)),
            None => (old_xfs.first().copied(), Look::default()),
        };
        let attr = |n: &str| sx.and_then(|x| x.attr(n)).unwrap_or("0").to_string();
        let mut font_id: usize = attr("fontId").parse().unwrap_or(0);
        if (look.bold, look.italic, look.color) != (base.bold, base.italic, base.color) {
            let mut kids: Vec<(String, String)> = fonts
                .get(font_id)
                .map(|f| f.children.iter().filter(|c| !matches!(c.local(), "b" | "i" | "color")).map(|c| (c.local().to_string(), c.raw(src).to_string())).collect())
                .unwrap_or_else(|| vec![("sz".into(), "<sz val=\"11\"/>".into()), ("name".into(), "<name val=\"Calibri\"/>".into())]);
            if look.bold {
                kids.push(("b".into(), "<b/>".into()));
            }
            if look.italic {
                kids.push(("i".into(), "<i/>".into()));
            }
            if let Some(c) = look.color {
                kids.push(("color".into(), format!("<color rgb=\"{}\"/>", hex_argb(c))));
            }
            xml::schema_sort(&mut kids, FONT_ORDER);
            let inner: String = kids.into_iter().map(|k| k.1).collect();
            font_id = fonts.len() + intern(&mut new_fonts, format!("<font>{inner}</font>"));
        }
        let mut fill_id: usize = attr("fillId").parse().unwrap_or(0);
        if look.fill != base.fill {
            fill_id = match look.fill {
                None => 0,
                Some(c) => fills_n + intern(&mut new_fills, format!("<fill><patternFill patternType=\"solid\"><fgColor rgb=\"{}\"/><bgColor indexed=\"64\"/></patternFill></fill>", hex_argb(c))),
            };
        }
        let old_al = sx.and_then(|x| x.child("alignment"));
        let align = if (look.align, look.wrap) != (base.align, base.wrap) {
            let mut a: Vec<(String, String)> = old_al.map(|n| n.attrs.iter().filter(|(k, _)| k != "horizontal" && k != "wrapText").cloned().collect()).unwrap_or_default();
            if let Some(h) = ["", "left", "center", "right"].get(look.align as usize).filter(|h| !h.is_empty()) {
                a.insert(0, ("horizontal".into(), h.to_string()));
            }
            if look.wrap {
                a.push(("wrapText".into(), "1".into()));
            }
            let at: String = a.iter().map(|(k, v)| format!(" {k}=\"{}\"", escape(v))).collect();
            if at.is_empty() { String::new() } else { format!("<alignment{at}/>") }
        } else {
            old_al.map(|n| n.raw(src).to_string()).unwrap_or_default()
        };
        let border = attr("borderId");
        let open = format!("<xf numFmtId=\"{id}\" fontId=\"{font_id}\" fillId=\"{fill_id}\" borderId=\"{border}\" xfId=\"0\" applyNumberFormat=\"1\" applyFont=\"1\" applyFill=\"1\" applyAlignment=\"1\"");
        if align.is_empty() {
            new_xfs.push_str(&format!("{open}/>"));
        } else {
            new_xfs.push_str(&format!("{open}>{align}</xf>"));
        }
    }
    let count = old_xfs.len() + (book.numfmt.len() - from);
    edits.push((xfs.start, xfs.end, format!("<cellXfs count=\"{count}\">{}{new_xfs}</cellXfs>", &src[xfs.inner.0..xfs.inner.1])));
    if let (false, Some(f)) = (new_fonts.is_empty(), root.child("fonts")) {
        let inner = &src[f.inner.0..f.inner.1];
        edits.push((f.start, f.end, format!("<fonts count=\"{}\">{inner}{}</fonts>", fonts.len() + new_fonts.len(), new_fonts.concat())));
    }
    if let (false, Some(f)) = (new_fills.is_empty(), root.child("fills")) {
        let inner = &src[f.inner.0..f.inner.1];
        edits.push((f.start, f.end, format!("<fills count=\"{}\">{inner}{}</fills>", fills_n + new_fills.len(), new_fills.concat())));
    }
    if !new_fmts.is_empty() {
        match root.child("numFmts") {
            Some(nf) => {
                let c = nf.children_named("numFmt").count() + nf_count;
                let inner = &src[nf.inner.0..nf.inner.1];
                edits.push((nf.start, nf.end, format!("<numFmts count=\"{c}\">{inner}{new_fmts}</numFmts>")));
            }
            None => {
                let at = root.children.first().map(|c| c.start).unwrap_or(root.inner.0);
                edits.push((at, at, format!("<numFmts count=\"{nf_count}\">{new_fmts}</numFmts>")));
            }
        }
    }
    Ok(xml::splice(src, edits))
}

