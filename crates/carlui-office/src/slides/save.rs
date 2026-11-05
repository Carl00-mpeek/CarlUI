//! Bu modül "düzenlenen sunuyu, dokunmadığımız her şeyi koruyarak .pptx
//! olarak geri yazalım" sorununu çözüyor. Slayt XML'inde yalnızca değişen
//! metin gövdesi ve taşınan şeklin konumu yeniden yazılır; gerisi bayt bayt
//! korunur. Slayt ekleme/çoğaltma/silme/sıralama presentation.xml'in
//! sldIdLst'i, ilişkileri ve [Content_Types].xml ile birlikte güncellenir.

use super::{pptx, Deck, Slide, EMU_PT};
use crate::{
    rich::{PAlign, Para, Run},
    xml::{self, escape},
    zip,
};
use std::collections::HashMap;

const SLIDE_CT: &str = "application/vnd.openxmlformats-officedocument.presentationml.slide+xml";
const REL_SLIDE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide";
const REL_LAYOUT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout";
const SECTIONS: &str = "{521415D9-36F7-43E2-AB2F-B90AF26B5E84}";

/// CT_TextCharacterProperties çocuk sırası.
const RPR_ORDER: &[&str] = &[
    "ln", "noFill", "solidFill", "gradFill", "blipFill", "pattFill", "grpFill", "effectLst", "effectDag", "highlight", "uLnTx", "uLn", "uFillTx", "uFill", "latin", "ea", "cs", "sym", "hlinkClick", "hlinkMouseOver", "rtl", "extLst",
];

fn emu(v: f32) -> i64 {
    (v * EMU_PT).round() as i64
}

/// İlk etiketteki özniteliği koyar (None: kaldırır).
pub fn set_attr(tag: &str, name: &str, val: Option<&str>) -> String {
    let Some(end) = tag.find('>') else { return tag.to_string() };
    let (head, rest) = tag.split_at(end);
    let self_close = head.ends_with('/');
    let mut head = head.trim_end_matches('/').to_string();
    let key = format!(" {name}=\"");
    if let Some(i) = head.find(&key) {
        let j = head[i + key.len()..].find('"').map(|j| i + key.len() + j + 1).unwrap_or(head.len());
        head.replace_range(i..j, "");
    }
    if let Some(v) = val {
        head.push_str(&format!(" {name}=\"{}\"", escape(v)));
    }
    format!("{head}{}{rest}", if self_close { "/" } else { "" })
}

fn algn(a: PAlign) -> &'static str {
    match a {
        PAlign::Left => "l",
        PAlign::Center => "ctr",
        PAlign::Right => "r",
        PAlign::Justify => "just",
    }
}

fn ppr_xml(p: &Para) -> String {
    let had_lvl = p.ppr.contains(" lvl=\"");
    if p.ppr.is_empty() && p.level == 0 && p.align == p.orig_align {
        return String::new();
    }
    let mut t = if p.ppr.is_empty() { "<a:pPr/>".to_string() } else { p.ppr.clone() };
    if p.level > 0 || had_lvl {
        let lv = p.level.to_string();
        t = set_attr(&t, "lvl", (p.level > 0).then_some(lv.as_str()));
    }
    if p.align != p.orig_align {
        t = set_attr(&t, "algn", p.align.map(algn));
    }
    if t == "<a:pPr/>" {
        return String::new();
    }
    t
}

fn rpr(r: &Run) -> String {
    let base = if r.rpr.is_empty() { "<a:rPr lang=\"tr-TR\" dirty=\"0\"/>".to_string() } else { r.rpr.clone() };
    if r.fmt == r.orig {
        return base;
    }
    let (f, o) = (&r.fmt, &r.orig);
    let flag = |v: Option<bool>| v.map(|b| if b { "1" } else { "0" });
    let mut t = base.clone();
    if f.b != o.b {
        t = set_attr(&t, "b", flag(f.b));
    }
    if f.i != o.i {
        t = set_attr(&t, "i", flag(f.i));
    }
    if f.u != o.u {
        t = set_attr(&t, "u", f.u.map(|b| if b { "sng" } else { "none" }));
    }
    if f.strike != o.strike {
        t = set_attr(&t, "strike", f.strike.map(|b| if b { "sngStrike" } else { "noStrike" }));
    }
    if f.size != o.size {
        t = set_attr(&t, "sz", f.size.map(|s| ((s * 100.0).round() as u32).clamp(100, 400_000).to_string()).as_deref());
    }
    if f.color == o.color {
        return t;
    }
    // Rengi değiştir: dolgu çocuklarını at, yenisini şema sırasıyla ekle.
    let Ok(n) = xml::parse(&t) else { return t };
    let open = n.open_tag(&t);
    let mut kids: Vec<(String, String)> = n.children.iter().filter(|c| !matches!(c.local(), "noFill" | "solidFill" | "gradFill" | "blipFill" | "pattFill" | "grpFill")).map(|c| (c.name.clone(), c.raw(&t).to_string())).collect();
    if let Some(c) = f.color {
        kids.push(("a:solidFill".into(), format!("<a:solidFill><a:srgbClr val=\"{c:06X}\"/></a:solidFill>")));
    }
    if kids.is_empty() {
        return format!("{}/>", open.trim_end_matches('>'));
    }
    xml::schema_sort(&mut kids, RPR_ORDER);
    format!("{open}{}</{}>", kids.into_iter().map(|x| x.1).collect::<String>(), n.name)
}

fn run_xml(r: &Run, out: &mut String) {
    if let Some(a) = &r.atom {
        out.push_str(a);
        return;
    }
    for (i, part) in r.text.split('\n').enumerate() {
        if i > 0 {
            out.push_str("<a:br>");
            out.push_str(&rpr(r));
            out.push_str("</a:br>");
        }
        if !part.is_empty() {
            out.push_str(&format!("<a:r>{}<a:t>{}</a:t></a:r>", rpr(r), escape(part)));
        }
    }
}

pub fn para_xml(p: &Para) -> String {
    let mut out = if p.open.is_empty() { "<a:p>".to_string() } else { p.open.clone() };
    out.push_str(&ppr_xml(p));
    for r in &p.runs {
        run_xml(r, &mut out);
    }
    if p.end.is_empty() {
        let size = p.runs.last().and_then(|r| r.fmt.size).filter(|_| p.runs.last().is_some_and(|r| r.rpr.is_empty()));
        match size {
            Some(s) => out.push_str(&format!("<a:endParaRPr lang=\"tr-TR\" sz=\"{}\" dirty=\"0\"/>", (s * 100.0).round() as u32)),
            None => out.push_str("<a:endParaRPr lang=\"tr-TR\" dirty=\"0\"/>"),
        }
    } else {
        out.push_str(&p.end);
    }
    out.push_str("</a:p>");
    out
}

fn rels_from(xml_src: &str, part: &str) -> HashMap<String, (String, String)> {
    let mut m = HashMap::new();
    if let Ok(n) = xml::parse(xml_src) {
        for c in &n.children {
            if let (Some(id), Some(t)) = (c.attr("Id"), c.attr("Target")) {
                let path = if c.attr("TargetMode") == Some("External") { t.to_string() } else { zip::resolve(part, t) };
                m.insert(id.to_string(), (c.attr("Type").unwrap_or("").to_string(), path));
            }
        }
    }
    m
}

/// `from` parçasından `to`'ya göreli hedef.
fn rel_target(from: &str, to: &str) -> String {
    let fd: Vec<&str> = from.split('/').collect();
    let td: Vec<&str> = to.split('/').collect();
    let fd = &fd[..fd.len() - 1];
    let common = fd.iter().zip(&td).take_while(|(a, b)| a == b).count();
    let mut s = "../".repeat(fd.len() - common);
    s.push_str(&td[common..].join("/"));
    s
}

fn reparse(d: &mut Deck, i: usize, src: String) -> Result<(), String> {
    let old = &d.slides[i];
    let (part, rels, new_rels, id, rid) = (old.part.clone(), old.rels.clone(), old.new_rels.clone(), old.id, old.rid.clone());
    let mut s = pptx::parse_slide(d, &part, src, rels)?;
    s.new_rels = new_rels;
    s.id = id;
    s.rid = rid;
    d.slides[i] = s;
    Ok(())
}

/// Slaytın değişen şekillerini XML'e işler ve slaytı yeniden okur.
pub fn commit(d: &mut Deck, i: usize) -> Result<(), String> {
    let s = &d.slides[i];
    let hid = hidden_edit(s);
    if !s.dirty() && hid.is_none() {
        return Ok(());
    }
    let src = &s.src;
    let mut edits: Vec<_> = hid.into_iter().collect();
    for sh in &s.shapes {
        if sh.moved && !sh.grouped {
            let geo = format!("<a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/>", emu(sh.x), emu(sh.y), emu(sh.w).max(0), emu(sh.h).max(0));
            if let Some((a, b)) = sh.xf_span {
                let node = xml::parse(&src[a..b])?;
                let raw = &src[a..b];
                // off/ext dışındaki çocuklar (chOff…) korunur.
                let rest: String = node.children.iter().filter(|c| !matches!(c.local(), "off" | "ext")).map(|c| c.raw(raw)).collect();
                edits.push((a, b, format!("{}{geo}{rest}</{}>", node.open_tag(raw), node.name)));
            } else if let Some((a, b)) = sh.sppr_span {
                let node = xml::parse(&src[a..b])?;
                let raw = &src[a..b];
                let xf = format!("<a:xfrm>{geo}</a:xfrm>");
                if node.self_closing(raw) {
                    edits.push((a, b, format!("{}{xf}</{}>", node.open_tag(raw), node.name)));
                } else {
                    let at = a + (node.inner.0 - node.start);
                    edits.push((at, at, xf));
                }
            }
        }
        if sh.dirty {
            if let Some((a, b)) = sh.body {
                let body: String = sh.paras.iter().map(|p| match p.span {
                    Some((pa, pb)) if !p.dirty && p.align == p.orig_align => src[pa..pb].to_string(),
                    _ => para_xml(p),
                }).collect();
                edits.push((a, b, body));
            }
        }
    }
    let out = xml::splice(src, edits);
    reparse(d, i, out)
}

/// Gizli slayt: kök öğenin `show="0"` özniteliği değiştiyse düzeltmesi.
fn hidden_edit(s: &Slide) -> Option<(usize, usize, String)> {
    let a = s.src.find("<p:sld")?;
    let b = s.src[a..].find('>')? + a + 1;
    let tag = &s.src[a..b];
    (tag.contains(" show=\"0\"") != s.hidden).then(|| (a, b, set_attr(tag, "show", s.hidden.then_some("0"))))
}

fn max_shape_id(src: &str) -> u32 {
    let mut m = 1;
    let mut rest = src;
    while let Some(i) = rest.find("cNvPr id=\"") {
        rest = &rest[i + 10..];
        let e = rest.find('"').unwrap_or(0);
        m = m.max(rest[..e].parse().unwrap_or(0));
    }
    m
}

/// Slayta metin kutusu ekler; yeni şeklin dizinini döndürür.
pub fn insert_textbox(d: &mut Deck, i: usize, x: f32, y: f32, w: f32, h: f32) -> Result<usize, String> {
    commit(d, i)?;
    let s = &d.slides[i];
    let id = max_shape_id(&s.src) + 1;
    let sp = format!(
        "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"TextBox {id}\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap=\"square\" rtlCol=\"0\"><a:spAutoFit/></a:bodyPr><a:lstStyle/><a:p><a:endParaRPr lang=\"tr-TR\" dirty=\"0\"/></a:p></p:txBody></p:sp>",
        emu(x),
        emu(y),
        emu(w),
        emu(h)
    );
    let out = xml::splice(&s.src, vec![(s.tree_end, s.tree_end, sp)]);
    reparse(d, i, out)?;
    Ok(d.slides[i].shapes.len() - 1)
}

pub fn delete_shape(d: &mut Deck, i: usize, si: usize) -> Result<(), String> {
    commit(d, i)?;
    let s = &d.slides[i];
    let Some(sh) = s.shapes.get(si) else { return Ok(()) };
    let out = xml::splice(&s.src, vec![(sh.span.0, sh.span.1, String::new())]);
    reparse(d, i, out)
}

fn free_part(d: &Deck) -> String {
    let mut n = 1;
    loop {
        let p = format!("ppt/slides/slide{n}.xml");
        if !d.zip.has(&p) && !d.slides.iter().any(|s| s.part == p) {
            return p;
        }
        n += 1;
    }
}

fn rels_xml(s: &Slide, d: &Deck) -> String {
    s.new_rels.clone().or_else(|| d.zip.read_str(&zip::rels_path(&s.part))).unwrap_or_default()
}

fn push_slide(d: &mut Deck, at: usize, part: String, src: String, rels: String) -> Result<usize, String> {
    let map = rels_from(&rels, &part);
    if let Some((_, lp)) = map.values().find(|(t, _)| t.ends_with("/slideLayout")) {
        let lp = lp.clone();
        pptx::ensure_layout(d, &lp);
    }
    let mut s = pptx::parse_slide(d, &part, src, map)?;
    s.new_rels = Some(rels);
    let at = at.min(d.slides.len());
    d.slides.insert(at, s);
    Ok(at)
}

/// `i` slaytının kopyasını hemen ardına ekler.
pub fn duplicate(d: &mut Deck, i: usize) -> Result<usize, String> {
    commit(d, i)?;
    let part = free_part(d);
    let s = &d.slides[i];
    let src = s.src.clone();
    let rels = rels_xml(s, d);
    // Konuşmacı notu ilişkisi kopyalanmaz (not tek slayta aittir).
    let rels = match xml::parse(&rels) {
        Ok(n) => {
            let cut: Vec<_> = n.children.iter().filter(|c| c.attr("Type").is_some_and(|t| t.ends_with("/notesSlide"))).map(|c| (c.start, c.end, String::new())).collect();
            xml::splice(&rels, cut)
        }
        Err(_) => rels,
    };
    push_slide(d, i + 1, part, src, rels)
}

/// `after` slaytının ardına boş (yer tutuculu) yeni slayt ekler.
pub fn new_slide(d: &mut Deck, after: usize) -> Result<usize, String> {
    let cur = d.slides.get(after).map(|s| s.layout.clone()).or_else(|| d.layers.iter().find(|(_, l)| !l.master.is_empty()).map(|(k, _)| k.clone())).ok_or("düzen yok")?;
    let master = d.layers.get(&cur).map(|l| l.master.clone()).unwrap_or_default();
    let cur_ty = d.layers.get(&cur).map(|l| l.ty.clone()).unwrap_or_default();
    let layout = if cur_ty == "title" || d.slides.is_empty() && cur_ty != "obj" {
        let mut v: Vec<&String> = d.layers.iter().filter(|(_, l)| l.master == master && l.ty == "obj").map(|(k, _)| k).collect();
        v.sort();
        v.first().map(|s| s.to_string()).unwrap_or(cur)
    } else {
        cur
    };
    let part = free_part(d);
    let mut sps = String::new();
    let mut id = 1;
    if let Some(l) = d.layers.get(&layout) {
        for sh in &l.shapes {
            let Some(ph) = &sh.ph else { continue };
            if matches!(ph.ty.as_str(), "dt" | "ftr" | "sldNum") {
                continue;
            }
            id += 1;
            let name = if ph.ty == "title" { "Title" } else { "Content Placeholder" };
            sps.push_str(&format!(
                "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{name} {id}\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr>{}</p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"tr-TR\" dirty=\"0\"/></a:p></p:txBody></p:sp>",
                ph.raw
            ));
        }
    }
    let src = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<p:sld xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>{sps}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"
    );
    let rels = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{REL_LAYOUT}\" Target=\"{}\"/></Relationships>",
        rel_target(&part, &layout)
    );
    push_slide(d, after + 1, part, src, rels)
}

fn main_part(d: &Deck) -> String {
    crate::sheet::xlsx::rels(&d.zip, "").into_values().find(|(t, _)| t.ends_with("/officeDocument")).map(|x| x.1.trim_start_matches('/').to_string()).unwrap_or_else(|| "ppt/presentation.xml".into())
}

fn remove_override(ct: &str, part: &str) -> String {
    let Ok(n) = xml::parse(ct) else { return ct.to_string() };
    let name = format!("/{part}");
    let cut: Vec<_> = n.children.iter().filter(|c| c.attr("PartName") == Some(name.as_str())).map(|c| (c.start, c.end, String::new())).collect();
    xml::splice(ct, cut)
}

fn insert_before_close(src: &str, close: &str, add: &str) -> String {
    match src.rfind(close) {
        Some(i) => format!("{}{add}{}", &src[..i], &src[i..]),
        None => src.to_string(),
    }
}

/// Slayt ekleme / silme / sıralamayı pakete işler.
fn write_structure(d: &mut Deck) -> Result<(), String> {
    let main = main_part(d);
    let mut pres = d.zip.read_str(&main).ok_or("presentation.xml yok")?;
    let prels_path = zip::rels_path(&main);
    let mut prels = d.zip.read_str(&prels_path).unwrap_or_default();
    let mut ct = d.zip.read_str("[Content_Types].xml").unwrap_or_default();
    // Silinen slaytlar.
    let keep: Vec<String> = d.slides.iter().map(|s| s.part.clone()).collect();
    for (part, _, rid) in d.orig.clone() {
        if keep.contains(&part) {
            continue;
        }
        let r = crate::sheet::xlsx::rels(&d.zip, &part);
        for (t, p) in r.values() {
            if t.ends_with("/notesSlide") {
                d.zip.remove(p);
                d.zip.remove(&zip::rels_path(p));
                ct = remove_override(&ct, p);
            }
        }
        d.zip.remove(&part);
        d.zip.remove(&zip::rels_path(&part));
        ct = remove_override(&ct, &part);
        if let Ok(n) = xml::parse(&prels) {
            let cut: Vec<_> = n.children.iter().filter(|c| c.attr("Id") == Some(rid.as_str())).map(|c| (c.start, c.end, String::new())).collect();
            prels = xml::splice(&prels, cut);
        }
    }
    // Yeni slaytlar.
    let mut next_id = d.slides.iter().map(|s| s.id).chain(d.orig.iter().map(|o| o.1)).max().unwrap_or(255).max(255) + 1;
    for i in 0..d.slides.len() {
        if d.slides[i].id != 0 {
            continue;
        }
        let part = d.slides[i].part.clone();
        let mut n = 1;
        while prels.contains(&format!("Id=\"rId{n}\"")) {
            n += 1;
        }
        let rid = format!("rId{n}");
        prels = insert_before_close(&prels, "</Relationships>", &format!("<Relationship Id=\"{rid}\" Type=\"{REL_SLIDE}\" Target=\"{}\"/>", rel_target(&main, &part)));
        if !ct.contains(&format!("PartName=\"/{part}\"")) {
            ct = insert_before_close(&ct, "</Types>", &format!("<Override PartName=\"/{part}\" ContentType=\"{SLIDE_CT}\"/>"));
        }
        d.slides[i].id = next_id;
        d.slides[i].rid = rid;
        next_id += 1;
    }
    // sldIdLst'i yeniden kur, bölümleri (artık tutarsız) kaldır.
    let list = format!("<p:sldIdLst>{}</p:sldIdLst>", d.slides.iter().map(|s| format!("<p:sldId id=\"{}\" r:id=\"{}\"/>", s.id, s.rid)).collect::<String>());
    let root = xml::parse(&pres)?;
    let mut edits = Vec::new();
    match root.child("sldIdLst") {
        Some(l) => edits.push((l.start, l.end, list)),
        None => {
            if let Some(sz) = root.child("sldSz") {
                edits.push((sz.start, sz.start, list));
            }
        }
    }
    let mut exts = Vec::new();
    root.find_all("ext", &mut exts);
    for e in exts {
        if e.attr("uri") == Some(SECTIONS) {
            edits.push((e.start, e.end, String::new()));
        }
    }
    pres = xml::splice(&pres, edits);
    d.zip.set(&main, pres.into_bytes());
    d.zip.set(&prels_path, prels.into_bytes());
    d.zip.set("[Content_Types].xml", ct.into_bytes());
    Ok(())
}

/// Bütün değişiklikleri işleyip paketin baytlarını verir.
pub fn save(d: &mut Deck) -> Result<Vec<u8>, String> {
    for i in 0..d.slides.len() {
        commit(d, i)?;
    }
    for s in &mut d.slides {
        if d.zip.read_str(&s.part).as_deref() != Some(s.src.as_str()) {
            d.zip.set(&s.part, s.src.clone().into_bytes());
        }
        if let Some(r) = s.new_rels.take() {
            d.zip.set(&zip::rels_path(&s.part), r.into_bytes());
        }
    }
    if d.structure_changed() || d.slides.iter().any(|s| s.id == 0) {
        write_structure(d)?;
    }
    d.orig = d.slides.iter().map(|s| (s.part.clone(), s.id, s.rid.clone())).collect();
    Ok(d.zip.to_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attrs() {
        assert_eq!(set_attr("<a:pPr lvl=\"1\"/>", "algn", Some("ctr")), "<a:pPr lvl=\"1\" algn=\"ctr\"/>");
        assert_eq!(set_attr("<a:rPr b=\"1\" sz=\"1800\"><a:latin/></a:rPr>", "b", None), "<a:rPr sz=\"1800\"><a:latin/></a:rPr>");
        assert_eq!(rel_target("ppt/slides/slide3.xml", "ppt/slideLayouts/slideLayout2.xml"), "../slideLayouts/slideLayout2.xml");
        assert_eq!(rel_target("ppt/presentation.xml", "ppt/slides/slide3.xml"), "slides/slide3.xml");
    }

    #[test]
    fn color_change() {
        let mut r = Run { text: "x".into(), rpr: "<a:rPr lang=\"tr-TR\" b=\"1\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"Arial\"/></a:rPr>".into(), ..Default::default() };
        r.fmt.color = Some(0xFF0000);
        let x = rpr(&r);
        assert_eq!(x, "<a:rPr lang=\"tr-TR\" b=\"1\"><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill><a:latin typeface=\"Arial\"/></a:rPr>");
    }
}
