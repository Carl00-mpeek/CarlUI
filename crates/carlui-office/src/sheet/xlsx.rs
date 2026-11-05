//! Bu modül "Excel dosyasını açıp düzenledikten sonra, bizim anlamadığımız
//! her şeyi (grafik, koşullu biçim, birleştirilmiş hücre, makro, sayfa düzeni…)
//! bozmadan geri yazalım" sorununu çözüyor. Zip içindeki parçalar olduğu gibi
//! kalır; yalnızca değişen sayfanın `sheetData` bölümü yeniden yazılır, eklenen
//! stiller `styles.xml` sonuna eklenir, eskiyen `calcChain` silinir ve Excel'e
//! açılışta yeniden hesaplaması söylenir.

use super::{formula, numfmt, xlsx_styles::add_styles, Book, Cell, Kind, Look, Sheet, Val};
use crate::xml::{self, escape};
use crate::zip::{self, Zip};
use std::collections::{BTreeMap, HashMap};

const MAIN_NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const WS_TYPE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet";
const WS_CT: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";

fn workbook_part(z: &Zip) -> String {
    // Kök ilişkilerden ana parçayı bul (çoğunlukla xl/workbook.xml).
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
    "xl/workbook.xml".into()
}

/// Bir parçanın ilişkileri: kimlik → (tür, hedef yol).
pub fn rels(z: &Zip, part: &str) -> HashMap<String, (String, String)> {
    let mut m = HashMap::new();
    if let Some(r) = z.read_str(&zip::rels_path(part)) {
        if let Ok(n) = xml::parse(&r) {
            for c in &n.children {
                if let (Some(id), Some(t)) = (c.attr("Id"), c.attr("Target")) {
                    let path = if c.attr("TargetMode") == Some("External") { t.to_string() } else { zip::resolve(part, t) };
                    m.insert(id.to_string(), (c.attr("Type").unwrap_or("").to_string(), path));
                }
            }
        }
    }
    m
}

fn argb(v: &str) -> Option<u32> {
    let v = v.trim();
    let v = if v.len() == 8 { &v[2..] } else { v };
    u32::from_str_radix(v, 16).ok()
}

fn load_styles(src: &str, book: &mut Book) {
    let Ok(root) = xml::parse(src) else { return };
    let mut custom: HashMap<u32, String> = HashMap::new();
    if let Some(nf) = root.child("numFmts") {
        for f in nf.children_named("numFmt") {
            if let (Some(id), Some(code)) = (f.attr("numFmtId").and_then(|x| x.parse().ok()), f.attr("formatCode")) {
                custom.insert(id, code.to_string());
            }
        }
    }
    let fonts: Vec<(bool, bool, Option<u32>)> = root
        .child("fonts")
        .map(|f| {
            f.children_named("font")
                .map(|x| {
                    let on = |n: &str| x.child(n).is_some_and(|b| b.attr("val").is_none_or(|v| v != "0" && v != "false"));
                    (on("b"), on("i"), x.child("color").and_then(|c| c.attr("rgb")).and_then(argb))
                })
                .collect()
        })
        .unwrap_or_default();
    let fills: Vec<Option<u32>> = root
        .child("fills")
        .map(|f| {
            f.children_named("fill")
                .map(|x| {
                    let p = x.child("patternFill")?;
                    if p.attr("patternType") != Some("solid") {
                        return None;
                    }
                    p.child("fgColor").and_then(|c| c.attr("rgb")).and_then(argb)
                })
                .collect()
        })
        .unwrap_or_default();
    book.numfmt.clear();
    book.looks.clear();
    if let Some(xfs) = root.child("cellXfs") {
        for xf in xfs.children_named("xf") {
            let id: u32 = xf.attr("numFmtId").and_then(|x| x.parse().ok()).unwrap_or(0);
            let code = custom.get(&id).cloned().or_else(|| numfmt::builtin(id).map(String::from)).unwrap_or_else(|| "General".into());
            book.numfmt.push(code);
            let font = xf.attr("fontId").and_then(|x| x.parse::<usize>().ok()).and_then(|i| fonts.get(i).copied()).unwrap_or_default();
            let fill = xf.attr("fillId").and_then(|x| x.parse::<usize>().ok()).and_then(|i| fills.get(i).copied()).flatten();
            let al = xf.child("alignment");
            let align = match al.and_then(|a| a.attr("horizontal")) {
                Some("left") => 1,
                Some("center") | Some("centerContinuous") => 2,
                Some("right") => 3,
                _ => 0,
            };
            book.looks.push(Look { bold: font.0, italic: font.1, color: font.2, fill, align, wrap: al.and_then(|a| a.attr("wrapText")).is_some_and(|v| v == "1" || v == "true") });
        }
    }
    if book.numfmt.is_empty() {
        book.numfmt.push("General".into());
        book.looks.push(Look::default());
    }
    book.xf_base = book.numfmt.len();
}

fn shared_strings(src: &str) -> Vec<String> {
    let Ok(root) = xml::parse(src) else { return Vec::new() };
    root.children_named("si")
        .map(|si| {
            // Fonetik (rPh) metni hariç: doğrudan t ve r/t.
            let mut s = String::new();
            for c in &si.children {
                match c.local() {
                    "t" => s.push_str(&c.text(src)),
                    "r" => {
                        if let Some(t) = c.child("t") {
                            s.push_str(&t.text(src));
                        }
                    }
                    _ => {}
                }
            }
            s
        })
        .collect()
}

fn load_sheet(src: &str, sst: &[String], sh: &mut Sheet) {
    let Ok(root) = xml::parse(src) else { return };
    if let Some(cols) = root.child("cols") {
        for c in cols.children_named("col") {
            let (Some(a), Some(b)) = (c.attr("min").and_then(|x| x.parse::<u32>().ok()), c.attr("max").and_then(|x| x.parse::<u32>().ok())) else { continue };
            if let Some(w) = c.attr("width").and_then(|x| x.parse::<f32>().ok()) {
                for i in a.saturating_sub(1)..b.min(16384) {
                    sh.col_w.insert(i, w);
                }
            }
        }
    }
    let Some(data) = root.child("sheetData") else { return };
    // Paylaşılan formül ana hücreleri: si → (satır, sütun, formül).
    let mut masters: HashMap<u32, (u32, u32, String)> = HashMap::new();
    let mut followers: Vec<((u32, u32), u32)> = Vec::new();
    let mut next_row = 0u32;
    for row in data.children_named("row") {
        let r = row.attr("r").and_then(|x| x.parse::<u32>().ok()).map(|x| x - 1).unwrap_or(next_row);
        next_row = r + 1;
        let open = strip_spans(&row.open_tag(src));
        sh.rows.insert(r, open);
        if let Some(h) = row.attr("ht").and_then(|x| x.parse::<f32>().ok()) {
            if row.attr("customHeight").is_some_and(|v| v == "1" || v == "true") {
                sh.row_h.insert(r, h);
            }
        }
        let mut next_col = 0u32;
        for c in row.children_named("c") {
            let (col, rr) = match c.attr("r").and_then(formula::parse_ref) {
                Some((col, rr, _, _)) => (col, rr),
                None => (next_col, r),
            };
            next_col = col + 1;
            let t = c.attr("t").unwrap_or("n");
            let v = c.child("v").map(|v| v.text(src));
            let val = match (t, v) {
                ("s", Some(i)) => i.trim().parse::<usize>().ok().and_then(|i| sst.get(i)).map(|s| Val::Str(s.clone())).unwrap_or_default(),
                ("str", Some(s)) => Val::Str(s),
                ("inlineStr", _) => Val::Str(c.child("is").map(|is| is.all_text(src, "t")).unwrap_or_default()),
                ("b", Some(b)) => Val::Bool(b.trim() == "1"),
                ("e", Some(e)) => Val::Err(e),
                ("d", Some(d)) => iso_date(&d).map(Val::Num).unwrap_or(Val::Str(d)),
                (_, Some(n)) => n.trim().parse::<f64>().map(Val::Num).unwrap_or(Val::Str(n)),
                _ => Val::Empty,
            };
            let mut cell = Cell { v: val, s: c.attr("s").and_then(|x| x.parse().ok()).unwrap_or(0), raw: Some(c.raw(src).to_string()), ..Default::default() };
            if let Some(f) = c.child("f") {
                let text = f.text(src);
                if f.attr("t") == Some("shared") {
                    let si: u32 = f.attr("si").and_then(|x| x.parse().ok()).unwrap_or(0);
                    cell.si = Some(si);
                    if !text.trim().is_empty() {
                        masters.insert(si, (rr, col, text.clone()));
                        cell.f = Some(text);
                    } else {
                        followers.push(((rr, col), si));
                    }
                } else if (f.attr("t") != Some("array") || !text.is_empty()) && !text.trim().is_empty() {
                    cell.f = Some(text);
                }
            }
            sh.cells.insert((rr, col), cell);
        }
    }
    for ((r, c), si) in followers {
        if let Some((mr, mc, f)) = masters.get(&si) {
            let shifted = formula::shift(f, r as i64 - *mr as i64, c as i64 - *mc as i64);
            if let Some(cell) = sh.cells.get_mut(&(r, c)) {
                cell.f = Some(shifted);
            }
        }
    }
}

fn iso_date(s: &str) -> Option<f64> {
    let d = s.get(0..10)?;
    let p: Vec<&str> = d.split('-').collect();
    if p.len() != 3 {
        return None;
    }
    let mut v = numfmt::serial(p[0].parse().ok()?, p[1].parse().ok()?, p[2].parse().ok()?);
    if let Some(t) = s.get(11..19) {
        let q: Vec<f64> = t.split(':').filter_map(|x| x.parse().ok()).collect();
        if q.len() == 3 {
            v += (q[0] * 3600.0 + q[1] * 60.0 + q[2]) / 86400.0;
        }
    }
    Some(v)
}

fn strip_spans(open: &str) -> String {
    // spans yalnızca bir ipucu; satırdaki hücreler değişebileceği için atılır.
    if let Some(i) = open.find(" spans=\"") {
        if let Some(j) = open[i + 8..].find('"') {
            return format!("{}{}", &open[..i], &open[i + 9 + j..]);
        }
    }
    open.to_string()
}

pub fn load(bytes: &[u8]) -> Result<Book, String> {
    let z = Zip::parse(bytes).map_err(|e| e.to_string())?;
    let wb_part = workbook_part(&z);
    let wb = z.read_str(&wb_part).ok_or("workbook.xml yok")?;
    let root = xml::parse(&wb)?;
    let mut book = Book { kind: Some(Kind::Xlsx), ..Default::default() };
    book.date1904 = root.child("workbookPr").and_then(|p| p.attr("date1904")).is_some_and(|v| v == "1" || v == "true");
    let rel = rels(&z, &wb_part);
    let find_rel = |suffix: &str| rel.values().find(|(t, _)| t.ends_with(suffix)).map(|(_, p)| p.clone());
    if let Some(s) = find_rel("/styles").and_then(|p| z.read_str(&p)) {
        load_styles(&s, &mut book);
    } else {
        book.numfmt.push("General".into());
        book.looks.push(Look::default());
        book.xf_base = 1;
    }
    let sst = find_rel("/sharedStrings").and_then(|p| z.read_str(&p)).map(|s| shared_strings(&s)).unwrap_or_default();
    if let Some(sheets) = root.child("sheets") {
        for s in sheets.children_named("sheet") {
            let name = s.attr("name").unwrap_or("?").to_string();
            let Some((ty, part)) = s.attr("r:id").or_else(|| s.attr("id")).and_then(|id| rel.get(id)) else { continue };
            if !ty.ends_with("/worksheet") {
                // Grafik sayfası vb.: gösterilmez ama dosyada kalır.
                continue;
            }
            let mut sh = Sheet::new(&name);
            sh.part = part.clone();
            if let Some(src) = z.read_str(part) {
                load_sheet(&src, &sst, &mut sh);
            }
            book.sheets.push(sh);
        }
    }
    if book.sheets.is_empty() {
        return Err("sayfa yok".into());
    }
    book.zip = Some(z);
    book.recalc_quiet();
    Ok(book)
}

impl Book {
    /// Açılışta hesaplama: değer değişse bile hücreler "kirli" sayılmaz
    /// (dosya hiç dokunulmadan kaydedilirse aynı kalsın).
    pub fn recalc_quiet(&mut self) {
        let before: Vec<_> = self.sheets.iter().map(|s| s.dirty.clone()).collect();
        type Raws = Vec<((u32, u32), Option<String>)>;
        let raws: Vec<Raws> = self.sheets.iter().map(|s| s.cells.iter().filter(|(_, c)| c.f.is_some()).map(|(k, c)| (*k, c.raw.clone())).collect()).collect();
        self.recalc();
        for (i, s) in self.sheets.iter_mut().enumerate() {
            s.dirty = before[i].clone();
            for (k, raw) in &raws[i] {
                if let Some(c) = s.cells.get_mut(k) {
                    c.raw = raw.clone();
                }
            }
        }
    }
}

// ---------------------------------------------------------------- kaydetme

fn num_str(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn cell_xml(r: u32, c: u32, cell: &Cell) -> String {
    let at = formula::cell_name(r, c);
    let s = if cell.s != 0 { format!(" s=\"{}\"", cell.s) } else { String::new() };
    if let Some(f) = &cell.f {
        let (t, v) = match &cell.v {
            Val::Num(n) => ("", num_str(*n)),
            Val::Str(x) => (" t=\"str\"", escape(x)),
            Val::Bool(b) => (" t=\"b\"", if *b { "1" } else { "0" }.to_string()),
            Val::Err(e) => (" t=\"e\"", escape(e)),
            Val::Empty => ("", String::new()),
        };
        let v = if v.is_empty() { String::new() } else { format!("<v>{v}</v>") };
        return format!("<c r=\"{at}\"{s}{t}><f>{}</f>{v}</c>", escape(f));
    }
    match &cell.v {
        Val::Empty => format!("<c r=\"{at}\"{s}/>"),
        Val::Num(n) => format!("<c r=\"{at}\"{s}><v>{}</v></c>", num_str(*n)),
        Val::Str(x) => format!("<c r=\"{at}\"{s} t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>", escape(x)),
        Val::Bool(b) => format!("<c r=\"{at}\"{s} t=\"b\"><v>{}</v></c>", if *b { 1 } else { 0 }),
        Val::Err(e) => format!("<c r=\"{at}\"{s} t=\"e\"><v>{}</v></c>", escape(e)),
    }
}

fn sheet_data(sh: &Sheet) -> String {
    let mut rows: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for r in sh.rows.keys() {
        rows.entry(*r).or_default();
    }
    for ((r, c), cell) in &sh.cells {
        let x = match &cell.raw {
            Some(raw) if !sh.dirty.contains(&(*r, *c)) => raw.clone(),
            _ => cell_xml(*r, *c, cell),
        };
        rows.entry(*r).or_default().push(x);
    }
    let mut out = String::from("<sheetData>");
    for (r, cells) in rows {
        let open = sh.rows.get(&r).cloned().unwrap_or_else(|| format!("<row r=\"{}\">", r + 1));
        if cells.is_empty() {
            out.push_str(&format!("{}/>", open.trim_end_matches('>')));
        } else {
            out.push_str(&open);
            for c in cells {
                out.push_str(&c);
            }
            out.push_str("</row>");
        }
    }
    out.push_str("</sheetData>");
    out
}

fn dimension(sh: &Sheet) -> String {
    let (r, c) = sh.extent();
    if r == 0 {
        "A1".into()
    } else {
        format!("A1:{}", formula::cell_name(r - 1, c - 1))
    }
}

/// Sütun genişlikleri değiştiyse `<cols>` öğesini yeniden kurar. Özgün `<col>`
/// öğelerinin (stil, gizli gibi) öznitelikleri korunur; aralık, genişliği
/// değişen sütunlarda bölünür. Kapsanmayan yeni genişlikler sona eklenir.
fn cols_xml(old: Option<&xml::Node>, sh: &Sheet) -> String {
    let mut out: Vec<(u32, String)> = Vec::new();
    let mut covered = std::collections::HashSet::new();
    let w_of = |c: u32| sh.col_w.get(&c).copied();
    let fmt_w = |w: f32| format!("{}", (w * 100.0).round() / 100.0);
    if let Some(cols) = old {
        for c in cols.children_named("col") {
            let (Some(a), Some(b)) = (c.attr("min").and_then(|x| x.parse::<u32>().ok()), c.attr("max").and_then(|x| x.parse::<u32>().ok())) else { continue };
            let rest: String = c.attrs.iter().filter(|(k, _)| !matches!(k.as_str(), "min" | "max" | "width" | "customWidth")).map(|(k, v)| format!(" {k}=\"{}\"", escape(v))).collect();
            let (a, b) = (a.max(1) - 1, b.min(16384) - 1);
            let mut i = a;
            while i <= b {
                let w = w_of(i);
                let mut j = i;
                while j < b && w_of(j + 1) == w {
                    j += 1;
                }
                for k in i..=j {
                    covered.insert(k);
                }
                let width = match w {
                    Some(w) => format!(" width=\"{}\" customWidth=\"1\"", fmt_w(w)),
                    None => c.attr("width").map(|x| format!(" width=\"{x}\"")).unwrap_or_default(),
                };
                out.push((i, format!("<col min=\"{}\" max=\"{}\"{width}{rest}/>", i + 1, j + 1)));
                i = j + 1;
            }
        }
    }
    let mut extra: Vec<(u32, f32)> = sh.col_w.iter().filter(|(c, _)| !covered.contains(*c)).map(|(c, w)| (*c, *w)).collect();
    extra.sort_by_key(|e| e.0);
    for (c, w) in extra {
        out.push((c, format!("<col min=\"{0}\" max=\"{0}\" width=\"{1}\" customWidth=\"1\"/>", c + 1, fmt_w(w))));
    }
    out.sort_by_key(|o| o.0);
    if out.is_empty() {
        return String::new();
    }
    format!("<cols>{}</cols>", out.into_iter().map(|o| o.1).collect::<String>())
}

fn patch_sheet(src: &str, sh: &Sheet) -> Result<String, String> {
    let root = xml::parse(src)?;
    let mut edits = Vec::new();
    match root.child("sheetData") {
        Some(d) => {
            if sh.cols_dirty {
                let cols = cols_xml(root.child("cols"), sh);
                match root.child("cols") {
                    Some(c) => edits.push((c.start, c.end, cols)),
                    None => edits.push((d.start, d.start, cols)),
                }
            }
            edits.push((d.start, d.end, sheet_data(sh)));
        }
        None => return Err("sheetData yok".into()),
    }
    if let Some(d) = root.child("dimension") {
        edits.push((d.start, d.end, format!("<dimension ref=\"{}\"/>", dimension(sh))));
    }
    Ok(xml::splice(src, edits))
}

fn new_sheet_xml(sh: &Sheet) -> String {
    let mut cols = String::new();
    if !sh.col_w.is_empty() {
        let mut v: Vec<_> = sh.col_w.iter().collect();
        v.sort_by_key(|(c, _)| **c);
        cols.push_str("<cols>");
        for (c, w) in v {
            cols.push_str(&format!("<col min=\"{0}\" max=\"{0}\" width=\"{1}\" customWidth=\"1\"/>", c + 1, w));
        }
        cols.push_str("</cols>");
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<worksheet xmlns=\"{MAIN_NS}\" xmlns:r=\"{REL_NS}\"><dimension ref=\"{}\"/><sheetViews><sheetView workbookViewId=\"0\"/></sheetViews><sheetFormatPr defaultRowHeight=\"15\"/>{cols}<sheetData/><pageMargins left=\"0.7\" right=\"0.7\" top=\"0.75\" bottom=\"0.75\" header=\"0.3\" footer=\"0.3\"/></worksheet>",
        dimension(sh)
    )
}

/// Boş (Excel'in açabileceği en küçük) çalışma kitabı iskeleti.
fn skeleton() -> Zip {
    let mut z = Zip::new();
    z.set(
        "[Content_Types].xml",
        b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/></Types>".to_vec(),
    );
    z.set(
        "_rels/.rels",
        b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>".to_vec(),
    );
    z.set(
        "xl/workbook.xml",
        format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<workbook xmlns=\"{MAIN_NS}\" xmlns:r=\"{REL_NS}\"><workbookPr/><bookViews><workbookView/></bookViews><sheets></sheets><calcPr calcId=\"191029\"/></workbook>").into_bytes(),
    );
    z.set(
        "xl/_rels/workbook.xml.rels",
        b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/></Relationships>".to_vec(),
    );
    z.set(
        "xl/styles.xml",
        format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<styleSheet xmlns=\"{MAIN_NS}\"><fonts count=\"2\"><font><sz val=\"11\"/><name val=\"Calibri\"/><family val=\"2\"/></font><font><b/><sz val=\"11\"/><name val=\"Calibri\"/><family val=\"2\"/></font></fonts><fills count=\"2\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill></fills><borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs><cellXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/></cellXfs><cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles></styleSheet>").into_bytes(),
    );
    z
}

/// Yazı tipi öğesinin çocuk sırası (CT_Font).
fn remove_calc_chain(z: &mut Zip, wb_part: &str) {
    let chain: Vec<String> = z.names().filter(|n| n.ends_with("calcChain.xml")).map(String::from).collect();
    if chain.is_empty() {
        return;
    }
    for c in &chain {
        z.remove(c);
    }
    if let Some(ct) = z.read_str("[Content_Types].xml") {
        if let Ok(root) = xml::parse(&ct) {
            let edits: Vec<_> = root.children.iter().filter(|c| c.attr("PartName").is_some_and(|p| p.ends_with("calcChain.xml"))).map(|c| (c.start, c.end, String::new())).collect();
            z.set("[Content_Types].xml", xml::splice(&ct, edits).into_bytes());
        }
    }
    let rp = zip::rels_path(wb_part);
    if let Some(r) = z.read_str(&rp) {
        if let Ok(root) = xml::parse(&r) {
            let edits: Vec<_> = root.children.iter().filter(|c| c.attr("Target").is_some_and(|p| p.ends_with("calcChain.xml"))).map(|c| (c.start, c.end, String::new())).collect();
            z.set(&rp, xml::splice(&r, edits).into_bytes());
        }
    }
}

fn set_full_calc(src: &str) -> Result<String, String> {
    let root = xml::parse(src)?;
    if let Some(c) = root.child("calcPr") {
        if c.attr("fullCalcOnLoad").is_some() {
            return Ok(src.to_string());
        }
        let open = c.open_tag(src);
        let self_closing = c.self_closing(src);
        let body = open.trim_end_matches('>');
        let new = if self_closing { format!("{body} fullCalcOnLoad=\"1\"/>") } else { format!("{body} fullCalcOnLoad=\"1\">") };
        let end = if self_closing { c.end } else { c.inner.0 };
        return Ok(xml::splice(src, vec![(c.start, end, new)]));
    }
    let after = ["definedNames", "externalReferences", "functionGroups", "sheets"].iter().find_map(|n| root.child(n)).map(|n| n.end).ok_or("sheets yok")?;
    Ok(xml::splice(src, vec![(after, after, "<calcPr fullCalcOnLoad=\"1\"/>".into())]))
}

/// Yeni sayfaları çalışma kitabına bağlar (workbook.xml, ilişkiler, içerik türleri).
fn add_new_sheets(z: &mut Zip, wb_part: &str, book: &mut Book) -> Result<(), String> {
    let news: Vec<usize> = (0..book.sheets.len()).filter(|i| book.sheets[*i].part.is_empty()).collect();
    if news.is_empty() {
        return Ok(());
    }
    let rp = zip::rels_path(wb_part);
    let rels_src = z.read_str(&rp).unwrap_or_else(|| "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>".into());
    let wb = z.read_str(wb_part).ok_or("workbook.xml yok")?;
    let ct = z.read_str("[Content_Types].xml").ok_or("içerik türleri yok")?;
    let wroot = xml::parse(&wb)?;
    let sheets = wroot.child("sheets").ok_or("sheets yok")?;
    let mut max_id = sheets.children_named("sheet").filter_map(|s| s.attr("sheetId").and_then(|x| x.parse::<u32>().ok())).max().unwrap_or(0);
    let mut rid = 1u32;
    let mut rel_add = String::new();
    let mut sheet_add = String::new();
    let mut ct_add = String::new();
    let dir = wb_part.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let mut n = 1;
    for i in news {
        while rels_src.contains(&format!("Id=\"rId{rid}\"")) || rel_add.contains(&format!("Id=\"rId{rid}\"")) {
            rid += 1;
        }
        while z.has(&format!("{dir}/worksheets/sheet{n}.xml")) {
            n += 1;
        }
        let path = if dir.is_empty() { format!("worksheets/sheet{n}.xml") } else { format!("{dir}/worksheets/sheet{n}.xml") };
        z.set(&path, new_sheet_xml(&book.sheets[i]).into_bytes());
        book.sheets[i].part = path.clone();
        max_id += 1;
        rel_add.push_str(&format!("<Relationship Id=\"rId{rid}\" Type=\"{WS_TYPE}\" Target=\"worksheets/sheet{n}.xml\"/>"));
        sheet_add.push_str(&format!("<sheet name=\"{}\" sheetId=\"{max_id}\" r:id=\"rId{rid}\"/>", escape(&book.sheets[i].name)));
        ct_add.push_str(&format!("<Override PartName=\"/{path}\" ContentType=\"{WS_CT}\"/>"));
        rid += 1;
        n += 1;
    }
    let rroot = xml::parse(&rels_src)?;
    z.set(&rp, xml::splice(&rels_src, vec![(rroot.inner.1, rroot.inner.1, rel_add)]).into_bytes());
    let wb2 = if sheets.self_closing(&wb) {
        xml::splice(&wb, vec![(sheets.start, sheets.end, format!("<sheets>{sheet_add}</sheets>"))])
    } else {
        xml::splice(&wb, vec![(sheets.inner.1, sheets.inner.1, sheet_add)])
    };
    z.set(wb_part, wb2.into_bytes());
    let croot = xml::parse(&ct)?;
    z.set("[Content_Types].xml", xml::splice(&ct, vec![(croot.inner.1, croot.inner.1, ct_add)]).into_bytes());
    Ok(())
}

pub fn save(book: &mut Book) -> Result<Vec<u8>, String> {
    let mut z = match book.zip.clone() {
        Some(z) => z,
        None => {
            book.xf_base = 1;
            // Yeni kitapta tüm sayfalar yeni; stil 0 dışındakiler eklenecek.
            for s in &mut book.sheets {
                s.part.clear();
            }
            skeleton()
        }
    };
    let wb_part = workbook_part(&z);
    add_new_sheets(&mut z, &wb_part, book)?;
    let rel = rels(&z, &wb_part);
    if book.numfmt.len() > book.xf_base {
        let sp = rel.values().find(|(t, _)| t.ends_with("/styles")).map(|(_, p)| p.clone()).ok_or("styles.xml yok")?;
        let src = z.read_str(&sp).ok_or("styles.xml okunamadı")?;
        z.set(&sp, add_styles(&src, book, book.xf_base)?.into_bytes());
    }
    let mut any = false;
    for sh in &book.sheets {
        let src = z.read_str(&sh.part).ok_or_else(|| format!("{} okunamadı", sh.part))?;
        // Yeni sayfada (iskelet `<sheetData/>`) hücreler hep yazılmalı.
        if sh.dirty.is_empty() && !sh.cols_dirty && src.contains("<sheetData") && !src.contains("<sheetData/>") {
            continue;
        }
        any = true;
        z.set(&sh.part, patch_sheet(&src, sh)?.into_bytes());
    }
    if any {
        remove_calc_chain(&mut z, &wb_part);
        let wb = z.read_str(&wb_part).ok_or("workbook.xml yok")?;
        z.set(&wb_part, set_full_calc(&wb)?.into_bytes());
    }
    let bytes = z.to_bytes();
    // Kaydedilen hal yeni taban: kirli işaretleri temizle, ham halleri güncelle.
    let fresh = load(&bytes)?;
    let base = fresh.xf_base;
    *book = Book { kind: Some(Kind::Xlsx), ..fresh };
    book.xf_base = base;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_book_roundtrip() {
        let _lang = crate::test_lang("en");
        let mut b = Book::new_blank();
        b.set_input(0, 0, 0, "Ürün");
        b.set_input(0, 0, 1, "5");
        b.set_input(0, 1, 1, "7");
        b.set_input(0, 2, 1, "=SUM(B1:B2)");
        b.set_input(0, 3, 1, "15%");
        b.add_sheet();
        b.set_input(1, 0, 0, "ikinci");
        let bytes = save(&mut b).unwrap();
        let r = load(&bytes).unwrap();
        assert_eq!(r.sheets.len(), 2);
        assert_eq!(r.sheets[0].get(0, 0).unwrap().v, Val::Str("Ürün".into()));
        assert_eq!(r.sheets[0].get(2, 1).unwrap().f.as_deref(), Some("SUM(B1:B2)"));
        assert_eq!(r.sheets[0].get(2, 1).unwrap().v, Val::Num(12.0));
        assert_eq!(r.display(0, 3, 1), "15%");
        assert_eq!(r.sheets[1].get(0, 0).unwrap().v, Val::Str("ikinci".into()));
        // Dokunmadan yeniden kaydetmek aynı sayfa XML'ini verir.
        let mut r2 = r.clone();
        let again = save(&mut r2).unwrap();
        let z1 = Zip::parse(&bytes).unwrap();
        let z2 = Zip::parse(&again).unwrap();
        assert_eq!(z1.read_str("xl/worksheets/sheet1.xml"), z2.read_str("xl/worksheets/sheet1.xml"));
    }

    #[test]
    fn shared_formula_expand() {
        let mut sh = Sheet::new("S");
        let src = r#"<worksheet><sheetData><row r="1" spans="1:2"><c r="A1"><v>1</v></c><c r="B1"><f t="shared" ref="B1:B2" si="0">A1*2</f><v>2</v></c></row><row r="2"><c r="A2"><v>3</v></c><c r="B2"><f t="shared" si="0"/><v>6</v></c></row></sheetData></worksheet>"#;
        load_sheet(src, &[], &mut sh);
        assert_eq!(sh.get(1, 1).unwrap().f.as_deref(), Some("A2*2"));
        assert_eq!(sh.rows[&0], "<row r=\"1\">");
    }
}
