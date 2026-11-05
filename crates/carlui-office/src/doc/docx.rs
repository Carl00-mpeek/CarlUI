//! Bu modül "Word (.docx) dosyasını okuyup düzenlenebilir paragraflara
//! çevirelim, anlamadığımız her şeyi ham XML olarak saklayalım" sorununu
//! çözüyor. Yalnızca düz metin parçaları (w:t, sekme, satır sonu) düzenlenir;
//! köprü, alan kodu, resim, yer imi, izlenen değişiklik gibi öğeler "atom"
//! olarak kalır ve kaydederken birebir geri yazılır.

use super::{attr_in, Block, CellBlock, Document, Page, Style, Table};
use crate::{
    rich::{Fmt, PAlign, Para, Run},
    sheet::xlsx::rels,
    xml::{self, Node},
    zip::{self, Zip},
};
use std::collections::HashMap;

fn main_part(z: &Zip) -> String {
    if let Some(r) = z.read_str("_rels/.rels") {
        if let Ok(n) = xml::parse(&r) {
            for c in &n.children {
                if c.attr("Type").is_some_and(|t| t.ends_with("/officeDocument")) {
                    if let Some(t) = c.attr("Target") {
                        return zip::resolve("", t);
                    }
                }
            }
        }
    }
    "word/document.xml".into()
}

fn on(n: &Node) -> bool {
    !matches!(n.attr("val"), Some("0" | "false" | "off" | "none"))
}

fn twips(v: Option<&str>) -> Option<f32> {
    v.and_then(|s| s.parse::<f32>().ok()).map(|t| t / 20.0)
}

pub fn hex(v: &str) -> Option<u32> {
    if v.len() == 6 {
        u32::from_str_radix(v, 16).ok()
    } else {
        None
    }
}

/// `w:rPr` → yazı biçimi.
pub fn rpr_fmt(n: &Node) -> Fmt {
    let mut f = Fmt::default();
    for c in &n.children {
        match c.local() {
            "b" => f.b = Some(on(c)),
            "i" => f.i = Some(on(c)),
            "u" => f.u = Some(on(c)),
            "strike" => f.strike = Some(on(c)),
            "sz" => f.size = c.attr("val").and_then(|v| v.parse::<f32>().ok()).map(|h| h / 2.0),
            "color" => f.color = c.attr("val").and_then(hex),
            _ => {}
        }
    }
    f
}

fn jc(v: &str) -> Option<PAlign> {
    Some(match v {
        "center" => PAlign::Center,
        "right" | "end" => PAlign::Right,
        "both" | "distribute" => PAlign::Justify,
        "left" | "start" => PAlign::Left,
        _ => return None,
    })
}

/// `w:pPr` içinden boşluk / hizalama → biçem alanlarına.
fn ppr_into(n: &Node, s: &mut Style) {
    if let Some(sp) = n.child("spacing") {
        s.before = twips(sp.attr("before")).or(s.before);
        s.after = twips(sp.attr("after")).or(s.after);
        if let Some(l) = sp.attr("line").and_then(|v| v.parse::<f32>().ok()) {
            if sp.attr("lineRule").is_none_or(|r| r == "auto") {
                s.line = Some(l / 240.0);
            }
        }
    }
    if let Some(j) = n.child("jc").and_then(|j| j.attr("val")).and_then(jc) {
        s.align = Some(j);
    }
}

fn load_styles(z: &Zip, d: &mut Document) {
    let Some(src) = z.read_str("word/styles.xml") else { return };
    let Ok(root) = xml::parse(&src) else { return };
    if let Some(dd) = root.child("docDefaults") {
        if let Some(r) = dd.path(&["rPrDefault", "rPr"]) {
            d.defaults.fmt = rpr_fmt(r);
            if let Some(f) = r.child("rFonts").and_then(|f| f.attr("ascii")) {
                d.font = f.to_string();
            }
        }
        if let Some(p) = dd.path(&["pPrDefault", "pPr"]) {
            ppr_into(p, &mut d.defaults);
        }
    }
    for s in root.children_named("style") {
        if s.attr("type") != Some("paragraph") {
            continue;
        }
        let Some(id) = s.attr("styleId") else { continue };
        let name = s.child("name").and_then(|n| n.attr("val")).unwrap_or(id).to_string();
        let mut st = Style { name: name.clone(), based: s.child("basedOn").and_then(|b| b.attr("val")).map(str::to_string), ..Default::default() };
        if let Some(r) = s.child("rPr") {
            st.fmt = rpr_fmt(r);
        }
        if let Some(p) = s.child("pPr") {
            ppr_into(p, &mut st);
            if let Some(l) = p.child("outlineLvl").and_then(|o| o.attr("val")).and_then(|v| v.parse::<u8>().ok()) {
                st.heading = l + 1;
            }
        }
        let lower = name.to_lowercase();
        if let Some(n) = lower.strip_prefix("heading ").and_then(|n| n.trim().parse::<u8>().ok()) {
            st.heading = n;
        } else if lower == "title" {
            st.heading = st.heading.max(1);
        }
        if s.attr("default").is_some_and(|v| v == "1" || v == "true") {
            d.normal = Some(id.to_string());
        }
        d.styles.insert(id.to_string(), st);
    }
}

fn load_numbering(z: &Zip, d: &mut Document) {
    let Some(src) = z.read_str("word/numbering.xml") else { return };
    let Ok(root) = xml::parse(&src) else { return };
    let mut abs: HashMap<String, Vec<(u8, String, String)>> = HashMap::new();
    for a in root.children_named("abstractNum") {
        let Some(id) = a.attr("abstractNumId") else { continue };
        let lvls = a
            .children_named("lvl")
            .filter_map(|l| {
                let i = l.attr("ilvl")?.parse().ok()?;
                let f = l.child("numFmt").and_then(|f| f.attr("val")).unwrap_or("decimal").to_string();
                let t = l.child("lvlText").and_then(|f| f.attr("val")).unwrap_or("").to_string();
                Some((i, f, t))
            })
            .collect();
        abs.insert(id.to_string(), lvls);
    }
    for n in root.children_named("num") {
        let (Some(id), Some(a)) = (n.attr("numId"), n.child("abstractNumId").and_then(|a| a.attr("val"))) else { continue };
        for (i, f, t) in abs.get(a).into_iter().flatten() {
            d.numbering.insert((id.to_string(), *i), (f.clone(), t.clone()));
        }
    }
}

fn theme_fonts(z: &Zip, d: &mut Document) {
    let Some(src) = z.read_str("word/theme/theme1.xml") else { return };
    let Ok(root) = xml::parse(&src) else { return };
    let face = |n: &str| root.find(n).and_then(|f| f.child("latin")).and_then(|l| l.attr("typeface")).filter(|s| !s.is_empty()).map(str::to_string);
    if let Some(f) = face("minorFont") {
        if d.font.is_empty() || d.font.starts_with('+') {
            d.font = f;
        }
    }
    if let Some(f) = face("majorFont") {
        d.heading_font = f;
    }
}

/// Bir `w:r` öğesi: içi yalnızca metin/sekme/satır sonuysa düzenlenebilir run,
/// değilse atom.
fn run(src: &str, n: &Node, d: &Document, out: &mut Vec<Run>) {
    let rpr = n.child("rPr");
    let fmt = rpr.map(rpr_fmt).unwrap_or_default();
    let rpr_raw = rpr.map(|r| r.raw(src).to_string()).unwrap_or_default();
    let mut text = String::new();
    let mut plain = true;
    for c in &n.children {
        match c.local() {
            "rPr" | "lastRenderedPageBreak" => {}
            "t" => text.push_str(&c.text(src)),
            "tab" => text.push('\t'),
            "br" if c.attr("type").is_none_or(|t| t == "textWrapping") => text.push('\n'),
            "noBreakHyphen" => text.push('\u{2011}'),
            _ => plain = false,
        }
    }
    if plain {
        if !text.is_empty() {
            out.push(Run { text, fmt, orig: fmt, rpr: rpr_raw, ..Default::default() });
        } else if !n.children.iter().all(|c| c.local() == "rPr") {
            // Yalnızca sayfa kesme işareti vb.: dokunmadan sakla.
            out.push(Run { atom: Some(n.raw(src).to_string()), fmt, ..Default::default() });
        }
        return;
    }
    // Atom: göstermek için metin ve varsa resim.
    let mut shown = String::new();
    let mut image = None;
    for c in &n.children {
        match c.local() {
            "t" => shown.push_str(&c.text(src)),
            "tab" => shown.push('\t'),
            "br" => {}
            "drawing" | "pict" | "object" => {
                if let Some(b) = c.find("blip").or_else(|| c.find("imagedata")) {
                    let id = b.attr("r:embed").or_else(|| b.attr("r:id")).unwrap_or("");
                    if let Some((_, path)) = d.rels.get(id) {
                        let (cx, cy) = c.find("extent").map(|e| (e.attr("cx").and_then(|v| v.parse().ok()).unwrap_or(0), e.attr("cy").and_then(|v| v.parse().ok()).unwrap_or(0))).unwrap_or((914400, 914400));
                        image = Some((path.clone(), cx, cy));
                    }
                }
                shown.push('\u{FFFC}');
            }
            "sym" => shown.push(c.attr("char").and_then(|h| u32::from_str_radix(h, 16).ok()).and_then(char::from_u32).filter(|c| !('\u{F000}'..='\u{F0FF}').contains(c)).unwrap_or('•')),
            "footnoteReference" | "endnoteReference" => shown.push('*'),
            _ => {}
        }
    }
    out.push(Run { text: shown, fmt, orig: fmt, atom: Some(n.raw(src).to_string()), image, ..Default::default() });
}

pub fn para(src: &str, n: &Node, d: &Document) -> Para {
    let mut p = Para { open: n.open_tag(src), span: Some((n.start, n.end)), ..Default::default() };
    if n.self_closing(src) {
        p.open = "<w:p>".into();
    }
    for c in &n.children {
        match c.local() {
            "pPr" => {
                p.ppr = c.raw(src).to_string();
                p.style = c.child("pStyle").and_then(|s| s.attr("val")).map(str::to_string);
                p.align = c.child("jc").and_then(|j| j.attr("val")).and_then(jc);
                p.level = attr_in(&p.ppr, "w:ilvl").and_then(|v| v.parse().ok()).unwrap_or(0);
            }
            "r" => run(src, c, d, &mut p.runs),
            // Silinmiş (izlenen) metin gösterilmez ama saklanır.
            "del" | "moveFrom" => p.runs.push(Run { atom: Some(c.raw(src).to_string()), ..Default::default() }),
            _ => {
                let mut t = c.all_text(src, "t");
                if c.find("drawing").is_some() {
                    t.push('\u{FFFC}');
                }
                p.runs.push(Run { text: t, atom: Some(c.raw(src).to_string()), ..Default::default() });
            }
        }
    }
    p.orig_style = p.style.clone();
    p.orig_align = p.align;
    p
}

fn table(src: &str, n: &Node, d: &Document) -> Table {
    let mut t = Table { cols: n.child("tblGrid").map(|g| g.children_named("gridCol").filter_map(|c| twips(c.attr("w"))).collect()).unwrap_or_default(), rows: Vec::new() };
    for tr in n.children_named("tr") {
        let mut row = Vec::new();
        for tc in tr.children_named("tc") {
            let pr = tc.child("tcPr");
            let span = pr.and_then(|p| p.child("gridSpan")).and_then(|g| g.attr("val")).and_then(|v| v.parse().ok()).unwrap_or(1usize).clamp(1, 64);
            let fill = pr.and_then(|p| p.child("shd")).and_then(|s| s.attr("fill")).and_then(hex);
            let mut paras = Vec::new();
            for c in &tc.children {
                match c.local() {
                    "p" => paras.push(para(src, c, d)),
                    "tbl" => paras.push(crate::doc::plain_para(&c.all_text(src, "t"))),
                    _ => {}
                }
            }
            row.push(CellBlock { span: span.max(1), fill, paras });
        }
        t.rows.push(row);
    }
    if t.cols.is_empty() {
        let n = t.rows.iter().map(|r| r.iter().map(|c| c.span).sum::<usize>()).max().unwrap_or(1);
        t.cols = vec![450.0 / n as f32; n];
    }
    t
}

fn page(n: &Node) -> Page {
    let mut p = Page::default();
    if let Some(s) = n.child("pgSz") {
        p.w = twips(s.attr("w")).unwrap_or(p.w);
        p.h = twips(s.attr("h")).unwrap_or(p.h);
    }
    if let Some(m) = n.child("pgMar") {
        p.top = twips(m.attr("top")).map(f32::abs).unwrap_or(p.top);
        p.bottom = twips(m.attr("bottom")).map(f32::abs).unwrap_or(p.bottom);
        p.left = twips(m.attr("left")).unwrap_or(p.left);
        p.right = twips(m.attr("right")).unwrap_or(p.right);
    }
    p
}

pub fn load(bytes: &[u8]) -> Result<Document, String> {
    let z = Zip::parse(bytes).map_err(|e| e.0)?;
    let part = main_part(&z);
    let src = z.read_str(&part).ok_or("word/document.xml yok")?;
    let root = xml::parse(&src)?;
    let body = root.child("body").ok_or("w:body yok")?;
    let mut d = Document {
        rels: rels(&z, &part),
        zip: Zip::new(),
        part,
        src: String::new(),
        body: body.inner,
        sect: String::new(),
        paras: Vec::new(),
        blocks: Vec::new(),
        styles: HashMap::new(),
        normal: None,
        defaults: Style { fmt: Fmt { size: Some(10.0), ..Default::default() }, ..Default::default() },
        font: String::new(),
        heading_font: String::new(),
        page: Page::default(),
        numbering: HashMap::new(),
        add_styles: Vec::new(),
    };
    load_styles(&z, &mut d);
    load_numbering(&z, &mut d);
    theme_fonts(&z, &mut d);
    if d.font.is_empty() || d.font.starts_with('+') {
        d.font = "Calibri".into();
    }
    if d.heading_font.is_empty() {
        d.heading_font = d.font.clone();
    }
    for c in &body.children {
        match c.local() {
            "p" => d.paras.push(para(&src, c, &d)),
            "sectPr" => {
                d.sect = c.raw(&src).to_string();
                d.page = page(c);
            }
            "tbl" => {
                let t = table(&src, c, &d);
                d.blocks.push(Block::Table(t));
                d.paras.push(Para { open: c.raw(&src).to_string(), span: Some((c.start, c.end)), block: Some(d.blocks.len() - 1), ..Default::default() });
            }
            _ => {
                d.blocks.push(Block::Other(c.all_text(&src, "t")));
                d.paras.push(Para { open: c.raw(&src).to_string(), span: Some((c.start, c.end)), block: Some(d.blocks.len() - 1), ..Default::default() });
            }
        }
    }
    // Bölüm sonları paragraf içinde de olabilir; ilk bölümün sayfası yeterli.
    if d.sect.is_empty() {
        if let Some(s) = body.find("sectPr") {
            d.page = page(s);
        }
    }
    // Belge yalnızca bloklardan oluşuyorsa imlecin duracağı bir paragraf ekle.
    if d.paras.iter().all(|p| p.block.is_some()) {
        d.paras.push(Para { open: "<w:p>".into(), dirty: true, ..Default::default() });
    }
    d.src = src;
    d.zip = z;
    d.renumber();
    Ok(d)
}
