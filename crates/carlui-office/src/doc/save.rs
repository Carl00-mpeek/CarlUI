//! Bu modül "düzenlenen Word belgesini, dokunmadığımız her şeyi koruyarak
//! .docx olarak geri yazalım" sorununu çözüyor. Değişmeyen paragraflar ve
//! tablolar özgün XML'leriyle aynen kopyalanır; değişen paragrafın yalnızca
//! biçimi değişen parçalarının `rPr`'si yeniden kurulur. Yeni belge için en
//! küçük geçerli paket (iskelet) de burada üretilir.

use super::{Document, Page, Style};
use crate::{
    rich::{Fmt, PAlign, Para, Run},
    xml::{self, escape},
    zip::Zip,
};
use std::collections::HashMap;

const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// CT_RPr çocuk sırası.
const RPR_ORDER: &[&str] = &[
    "rStyle", "rFonts", "b", "bCs", "i", "iCs", "caps", "smallCaps", "strike", "dstrike", "outline", "shadow", "emboss", "imprint", "noProof", "snapToGrid", "vanish", "webHidden", "color", "spacing", "w", "kern", "position", "sz", "szCs", "highlight", "u", "effect", "bdr", "shd", "fitText", "vertAlign", "rtl", "cs", "em", "lang", "eastAsianLayout", "specVanish", "oMath",
];

/// CT_PPr çocuk sırası.
const PPR_ORDER: &[&str] = &[
    "pStyle", "keepNext", "keepLines", "pageBreakBefore", "framePr", "widowControl", "numPr", "suppressLineNumbers", "pBdr", "shd", "tabs", "suppressAutoHyphens", "kinsoku", "wordWrap", "overflowPunct", "topLinePunct", "autoSpaceDE", "autoSpaceDN", "bidi", "adjustRightInd", "snapToGrid", "spacing", "ind", "contextualSpacing", "mirrorIndents", "suppressOverlap", "jc", "textDirection", "textAlignment", "textboxTightWrap", "outlineLvl", "divId", "cnfStyle", "rPr", "sectPr", "pPrChange",
];

/// Öğenin çocuklarını (ad, ham XML) olarak verir; `drop` adları atlanır.
fn kids(raw: &str, drop: &[&str]) -> Vec<(String, String)> {
    let Ok(n) = xml::parse(raw) else { return Vec::new() };
    n.children.iter().filter(|c| !drop.contains(&c.local())).map(|c| (c.name.clone(), c.raw(raw).to_string())).collect()
}

fn toggle(v: &mut Vec<(String, String)>, tag: &str, val: Option<bool>) {
    match val {
        Some(true) => v.push((format!("w:{tag}"), format!("<w:{tag}/>"))),
        Some(false) => v.push((format!("w:{tag}"), format!("<w:{tag} w:val=\"0\"/>"))),
        None => {}
    }
}

fn rpr(r: &Run) -> String {
    if r.fmt == r.orig {
        return r.rpr.clone();
    }
    let f = &r.fmt;
    let mut v = kids(&r.rpr, &["b", "bCs", "i", "iCs", "u", "strike", "sz", "szCs", "color"]);
    toggle(&mut v, "b", f.b);
    toggle(&mut v, "i", f.i);
    toggle(&mut v, "strike", f.strike);
    match f.u {
        Some(true) => v.push(("w:u".into(), "<w:u w:val=\"single\"/>".into())),
        Some(false) => v.push(("w:u".into(), "<w:u w:val=\"none\"/>".into())),
        None => {}
    }
    if let Some(s) = f.size {
        let h = (s * 2.0).round() as u32;
        v.push(("w:sz".into(), format!("<w:sz w:val=\"{h}\"/>")));
        v.push(("w:szCs".into(), format!("<w:szCs w:val=\"{h}\"/>")));
    }
    if let Some(c) = f.color {
        v.push(("w:color".into(), format!("<w:color w:val=\"{c:06X}\"/>")));
    }
    if v.is_empty() {
        return String::new();
    }
    xml::schema_sort(&mut v, RPR_ORDER);
    format!("<w:rPr>{}</w:rPr>", v.into_iter().map(|x| x.1).collect::<String>())
}

fn run_xml(r: &Run, out: &mut String) {
    if let Some(a) = &r.atom {
        out.push_str(a);
        return;
    }
    if r.text.is_empty() {
        return;
    }
    out.push_str("<w:r>");
    out.push_str(&rpr(r));
    let mut buf = String::new();
    let flush = |buf: &mut String, out: &mut String| {
        if !buf.is_empty() {
            out.push_str(&format!("<w:t xml:space=\"preserve\">{}</w:t>", escape(buf)));
            buf.clear();
        }
    };
    for ch in r.text.chars() {
        match ch {
            '\t' => {
                flush(&mut buf, out);
                out.push_str("<w:tab/>");
            }
            '\n' => {
                flush(&mut buf, out);
                out.push_str("<w:br/>");
            }
            '\u{2011}' => {
                flush(&mut buf, out);
                out.push_str("<w:noBreakHyphen/>");
            }
            c => buf.push(c),
        }
    }
    flush(&mut buf, out);
    out.push_str("</w:r>");
}

fn jc(a: PAlign) -> &'static str {
    match a {
        PAlign::Left => "left",
        PAlign::Center => "center",
        PAlign::Right => "right",
        PAlign::Justify => "both",
    }
}

pub fn para_xml(p: &Para) -> String {
    let mut out = if p.open.is_empty() { "<w:p>".to_string() } else { p.open.clone() };
    if p.style != p.orig_style || p.align != p.orig_align {
        let mut v = kids(&p.ppr, &["pStyle", "jc"]);
        if let Some(s) = &p.style {
            v.push(("w:pStyle".into(), format!("<w:pStyle w:val=\"{}\"/>", escape(s))));
        }
        if let Some(a) = p.align {
            v.push(("w:jc".into(), format!("<w:jc w:val=\"{}\"/>", jc(a))));
        }
        xml::schema_sort(&mut v, PPR_ORDER);
        if !v.is_empty() {
            out.push_str(&format!("<w:pPr>{}</w:pPr>", v.into_iter().map(|x| x.1).collect::<String>()));
        }
    } else {
        out.push_str(&p.ppr);
    }
    for r in &p.runs {
        run_xml(r, &mut out);
    }
    out.push_str("</w:p>");
    out
}

fn body_xml(d: &Document) -> String {
    let mut out = String::with_capacity(d.src.len().max(4096));
    for p in &d.paras {
        if p.block.is_some() {
            out.push_str(&p.open);
        } else if let (false, Some((s, e))) = (p.dirty, p.span) {
            out.push_str(&d.src[s..e]);
        } else {
            out.push_str(&para_xml(p));
        }
    }
    out.push_str(&d.sect);
    out
}

pub fn save(d: &mut Document) -> Result<Vec<u8>, String> {
    let body = body_xml(d);
    let src = xml::splice(&d.src, vec![(d.body.0, d.body.1, body)]);
    let mut z = d.zip.clone();
    z.set(&d.part, src.into_bytes());
    if !d.add_styles.is_empty() {
        let styles = z.read_str("word/styles.xml").ok_or("word/styles.xml yok")?;
        let at = styles.rfind("</w:styles>").ok_or("styles.xml bozuk")?;
        let mut s = styles[..at].to_string();
        // Aynı kimlik zaten varsa ekleme (iki kez kaydetme).
        for st in &d.add_styles {
            let id = st.split("w:styleId=\"").nth(1).and_then(|r| r.split('"').next()).unwrap_or("");
            if !styles.contains(&format!("w:styleId=\"{id}\"")) {
                s.push_str(st);
            }
        }
        s.push_str(&styles[at..]);
        z.set("word/styles.xml", s.into_bytes());
    }
    Ok(z.to_bytes())
}

/// Boş belge: A4, Calibri 11, Normal ve üç başlık biçemi.
pub fn blank() -> Document {
    let mut z = Zip::new();
    z.set(
        "[Content_Types].xml",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Default Extension="jpeg" ContentType="image/jpeg"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>"#
            .as_bytes()
            .to_vec(),
    );
    z.set(
        "_rels/.rels",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#
            .as_bytes()
            .to_vec(),
    );
    z.set(
        "word/_rels/document.xml.rels",
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#
            .as_bytes()
            .to_vec(),
    );
    let lang = if carlui_core::lang::is_tr() { "tr-TR" } else { "en-US" };
    z.set(
        "word/styles.xml",
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="{W_NS}"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="Calibri" w:cs="Calibri"/><w:sz w:val="22"/><w:szCs w:val="22"/><w:lang w:val="{lang}"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style></w:styles>"#
        )
        .into_bytes(),
    );
    let src = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{W_NS}" xmlns:r="{R_NS}"><w:body><w:p/><w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1417" w:right="1417" w:bottom="1417" w:left="1417" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr></w:body></w:document>"#
    );
    z.set("word/document.xml", src.clone().into_bytes());
    let bytes = z.to_bytes();
    let mut d = super::docx::load(&bytes).unwrap_or_else(|_| Document {
        zip: Zip::new(),
        part: "word/document.xml".into(),
        src: String::new(),
        body: (0, 0),
        sect: String::new(),
        paras: Vec::new(),
        blocks: Vec::new(),
        styles: HashMap::new(),
        normal: None,
        defaults: Style::default(),
        font: "Calibri".into(),
        heading_font: "Calibri".into(),
        page: Page::default(),
        rels: HashMap::new(),
        numbering: HashMap::new(),
        add_styles: Vec::new(),
    });
    if d.paras.is_empty() {
        d.paras.push(super::plain_para(""));
    }
    d.defaults.fmt = Fmt { size: Some(11.0), ..d.defaults.fmt };
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rich::{Cursor, Pos};

    #[test]
    fn blank_roundtrip_with_heading_and_bold() {
        let mut d = blank();
        let mut c = Cursor::default();
        crate::rich::type_text(&mut d.paras, &mut c, "Başlık\nGövde <metin> & sekme\tson");
        let h = d.heading_style(1);
        d.paras[0].style = h;
        c.set(Pos { p: 1, c: 0 }, false);
        c.set(Pos { p: 1, c: 5 }, true);
        crate::rich::apply_fmt(&mut d.paras, &c, |f| f.b = Some(true));
        let bytes = save(&mut d).unwrap();
        let d2 = super::super::docx::load(&bytes).unwrap();
        assert_eq!(d2.paras.len(), 2);
        assert_eq!(d2.paras[0].text(), "Başlık");
        assert_eq!(d2.heading_of(d2.paras[0].style.as_deref()), 1);
        assert_eq!(d2.paras[1].text(), "Gövde <metin> & sekme\tson");
        assert_eq!(d2.paras[1].runs[0].fmt.b, Some(true));
        assert_eq!(d2.paras[1].runs[0].text, "Gövde");
        assert!((d2.page.w - 595.3).abs() < 1.0);
    }

    #[test]
    fn untouched_paragraph_is_verbatim() {
        let mut d = blank();
        d.src = d.src.replace("<w:p/>", "<w:p w:rsidR=\"00AB\"><w:r><w:t>x</w:t></w:r><w:proofErr w:type=\"spellStart\"/></w:p>");
        let bytes = {
            let mut z = d.zip.clone();
            z.set("word/document.xml", d.src.clone().into_bytes());
            z.to_bytes()
        };
        let mut d = super::super::docx::load(&bytes).unwrap();
        let z = Zip::parse(&save(&mut d).unwrap()).unwrap();
        assert!(z.read_str("word/document.xml").unwrap().contains("<w:p w:rsidR=\"00AB\"><w:r><w:t>x</w:t></w:r><w:proofErr w:type=\"spellStart\"/></w:p>"));
    }
}
