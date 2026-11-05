//! Bu modül "CSV dosyasını açıp kaydettiğimizde ayırıcı (`,` `;` sekme),
//! BOM ve kodlama (UTF-8 ya da Türkçe Windows-1254) aynı kalsın" sorununu
//! çözüyor. Türkçe Excel CSV'yi `;` ile ve çoğu zaman 1254 kodlamasıyla yazar.

use super::{general, Book, Cell, Kind, Sheet, Val};

#[derive(Clone, Debug)]
pub struct Meta {
    pub delim: char,
    pub bom: bool,
    /// UTF-8 değilse Windows-1254.
    pub utf8: bool,
}

impl Default for Meta {
    fn default() -> Self {
        Meta { delim: ',', bom: true, utf8: true }
    }
}

fn decode(bytes: &[u8]) -> (String, bool, bool) {
    if let Some(b) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return (String::from_utf8_lossy(b).into_owned(), true, true);
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let (s, _, _) = encoding_rs::UTF_16LE.decode(bytes);
        return (s.into_owned(), true, true);
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_string(), false, true),
        Err(_) => {
            let (s, _, _) = encoding_rs::WINDOWS_1254.decode(bytes);
            (s.into_owned(), false, false)
        }
    }
}

fn detect_delim(text: &str) -> char {
    // İlk birkaç satırda tırnak dışında en tutarlı görülen aday.
    let lines: Vec<&str> = text.lines().take(10).collect();
    let mut best = (',', 0usize);
    for d in [';', ',', '\t', '|'] {
        let counts: Vec<usize> = lines
            .iter()
            .map(|l| {
                let mut q = false;
                l.chars().filter(|c| {
                    if *c == '"' {
                        q = !q;
                    }
                    !q && *c == d
                }).count()
            })
            .collect();
        let first = counts.first().copied().unwrap_or(0);
        if first > 0 && counts.iter().all(|c| *c == first || *c == 0) && first > best.1 {
            best = (d, first);
        }
    }
    best.0
}

pub fn parse_rows(text: &str, d: char) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut q = false;
    let mut chars = text.chars().peekable();
    let mut any = false;
    while let Some(c) = chars.next() {
        any = true;
        if q {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    q = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => q = true,
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                any = false;
            }
            _ if c == d => row.push(std::mem::take(&mut field)),
            _ => field.push(c),
        }
    }
    if any || !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

pub fn load(bytes: &[u8]) -> Book {
    let (text, bom, utf8) = decode(bytes);
    let delim = detect_delim(&text);
    let mut sh = Sheet::new(carlui_core::t!("Sayfa1", "Sheet1"));
    for (r, row) in parse_rows(&text, delim).into_iter().enumerate() {
        for (c, f) in row.into_iter().enumerate() {
            if f.is_empty() {
                continue;
            }
            let v = parse_field(&f, delim);
            sh.cells.insert((r as u32, c as u32), Cell { v, raw: Some(f), ..Default::default() });
        }
    }
    let mut b = Book::new_blank();
    b.sheets = vec![sh];
    b.kind = Some(Kind::Csv);
    b.csv = Meta { delim, bom, utf8 };
    b
}

/// Alan değeri: ayırıcı `;` ise ondalık virgül, `,` ise ondalık nokta beklenir.
fn parse_field(f: &str, delim: char) -> Val {
    let t = f.trim();
    let num = if delim == ',' { t.to_string() } else { t.replace('.', "").replace(',', ".") };
    let ok = !num.is_empty() && num.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')) && num.chars().any(|c| c.is_ascii_digit());
    // Baştaki sıfır (posta kodu, telefon) metin kalsın.
    let lead_zero = t.len() > 1 && t.starts_with('0') && !t.starts_with("0.") && !t.starts_with("0,");
    if ok && !lead_zero {
        if let Ok(n) = num.parse::<f64>() {
            return Val::Num(n);
        }
    }
    Val::Str(f.to_string())
}

fn quote(s: &str, d: char) -> String {
    if s.contains(d) || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn save(book: &mut Book) -> Vec<u8> {
    let m = book.csv.clone();
    let sh = &book.sheets[0];
    let (rows, cols) = sh.extent();
    let mut out = String::new();
    for r in 0..rows {
        let mut line = Vec::with_capacity(cols as usize);
        for c in 0..cols {
            let s = match sh.get(r, c) {
                None => String::new(),
                Some(cell) => match (&cell.raw, sh.dirty.contains(&(r, c))) {
                    (Some(raw), false) => raw.clone(),
                    _ => match &cell.v {
                        Val::Num(n) if cell.s == 0 => {
                            let g = general(*n);
                            // general() dile göre ondalık ayırıcı kullanır; CSV ayırıcısına uydur.
                            if m.delim == ',' { g.replace(',', ".") } else { g.replace('.', ",") }
                        }
                        _ => book.display(0, r, c),
                    },
                },
            };
            line.push(quote(&s, m.delim));
        }
        out.push_str(&line.join(&m.delim.to_string()));
        out.push_str("\r\n");
    }
    let mut bytes = Vec::new();
    if m.utf8 {
        if m.bom {
            bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        }
        bytes.extend_from_slice(out.as_bytes());
    } else {
        let (b, _, _) = encoding_rs::WINDOWS_1254.encode(&out);
        bytes.extend_from_slice(&b);
    }
    for s in &mut book.sheets {
        s.dirty.clear();
    }
    for (k, cell) in book.sheets[0].cells.iter_mut() {
        let _ = k;
        if cell.f.is_none() && cell.raw.is_none() {
            if let Val::Str(s) = &cell.v {
                cell.raw = Some(s.clone());
            }
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_roundtrip() {
        let _lang = crate::test_lang("tr");
        let src = "Ad;Tutar;Posta\r\n\"Ali; Veli\";1.234,50;06100\r\nÇiğdem;12;34000\r\n";
        let (b1254, _, _) = encoding_rs::WINDOWS_1254.encode(src);
        let mut b = load(&b1254);
        assert_eq!(b.csv.delim, ';');
        assert!(!b.csv.utf8);
        assert_eq!(b.sheets[0].get(1, 0).unwrap().v, Val::Str("Ali; Veli".into()));
        assert_eq!(b.sheets[0].get(1, 1).unwrap().v, Val::Num(1234.5));
        assert_eq!(b.sheets[0].get(1, 2).unwrap().v, Val::Str("06100".into()));
        // Dokunmadan kaydetmek aynı baytları verir.
        assert_eq!(String::from_utf8_lossy(&save(&mut b)), String::from_utf8_lossy(&b1254));
        b.set_input(0, 2, 1, "=B2+12");
        let out = save(&mut b);
        let (text, _, _) = encoding_rs::WINDOWS_1254.decode(&out);
        assert!(text.contains("Çiğdem;1246,5;34000"), "{text}");
    }
}
