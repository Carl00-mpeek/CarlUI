//! Bu modül "eski Excel (.xls, 97-2003) dosyalarını da açalım" sorununu
//! çözüyor. Okuma calamine ile yapılır; ikili biçimi yazmak yerine
//! kaydederken yanına aynı adlı .xlsx oluşturulur (Excel'in "dönüştür"ü gibi).

use super::{formula, Book, Cell, Kind, Sheet, Val};
use calamine::{Data, Reader, Xls};
use std::io::Cursor;

pub fn load(bytes: &[u8]) -> Result<Book, String> {
    let mut wb: Xls<_> = Xls::new(Cursor::new(bytes.to_vec())).map_err(|e| e.to_string())?;
    let mut book = Book::new_blank();
    book.sheets.clear();
    book.kind = Some(Kind::Xls);
    let date = book.style_like("dd.mm.yyyy");
    for name in wb.sheet_names() {
        let mut sh = Sheet::new(&name);
        if let Ok(range) = wb.worksheet_range(&name) {
            let (r0, c0) = range.start().unwrap_or((0, 0));
            for (r, c, d) in range.used_cells() {
                let (r, c) = (r0 + r as u32, c0 + c as u32);
                let (v, s) = match d {
                    Data::Int(i) => (Val::Num(*i as f64), 0),
                    Data::Float(f) => (Val::Num(*f), 0),
                    Data::String(s) => (Val::Str(s.clone()), 0),
                    Data::Bool(b) => (Val::Bool(*b), 0),
                    Data::DateTime(dt) => (Val::Num(dt.as_f64()), date),
                    Data::DateTimeIso(s) | Data::DurationIso(s) => (Val::Str(s.clone()), 0),
                    Data::Error(e) => (Val::Err(format!("{e}")), 0),
                    Data::Empty => continue,
                };
                sh.cells.insert((r, c), Cell { v, s, ..Default::default() });
            }
        }
        if let Ok(fr) = wb.worksheet_formula(&name) {
            let (r0, c0) = fr.start().unwrap_or((0, 0));
            for (r, c, f) in fr.used_cells() {
                if f.is_empty() {
                    continue;
                }
                let key = (r0 + r as u32, c0 + c as u32);
                let cell = sh.cells.entry(key).or_default();
                cell.f = Some(formula::normalize(f.trim_start_matches('=')));
            }
        }
        book.sheets.push(sh);
    }
    if book.sheets.is_empty() {
        return Err("sayfa yok".into());
    }
    book.recalc();
    for s in &mut book.sheets {
        s.dirty.clear();
    }
    Ok(book)
}
