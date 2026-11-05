//! Bu modül "Excel/CSV tablosunu bellekte tutalım, hücre düzenleyelim,
//! formülleri yeniden hesaplayalım, geri alalım" sorununu çözüyor. Dosya
//! biçimleri ayrı modüllerde: `xlsx` (yerinde yamalayarak kaydeder), `xls`
//! (yalnızca okuma; xlsx olarak kaydedilir), `csv`.

pub mod csv;
pub mod formula;
pub mod grid;
mod eval;
mod menus;
mod xlsx_styles;
pub mod numfmt;
pub mod view;
pub mod xls;
pub mod xlsx;

use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Default)]
pub enum Val {
    #[default]
    Empty,
    Num(f64),
    Str(String),
    Bool(bool),
    Err(String),
}

#[derive(Clone, Debug, Default)]
pub struct Cell {
    pub v: Val,
    /// Formül (başında `=` olmadan, dosyadaki İngilizce biçimde).
    pub f: Option<String>,
    /// Stil dizini (xlsx `s`).
    pub s: u32,
    /// Dosyadaki ham hali (değişmediyse olduğu gibi yazılır).
    pub raw: Option<String>,
    /// Paylaşılan formül grubu (xlsx `si`).
    pub si: Option<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct Sheet {
    pub name: String,
    pub cells: BTreeMap<(u32, u32), Cell>,
    /// Sütun genişliği (Excel karakter birimi).
    pub col_w: HashMap<u32, f32>,
    pub row_h: HashMap<u32, f32>,
    /// Sütun genişliği elle değişti (kaydederken `<cols>` yeniden yazılır).
    pub cols_dirty: bool,
    /// xlsx: satırların özgün açılış etiketi (spans olmadan).
    pub rows: BTreeMap<u32, String>,
    /// xlsx: zip içindeki yol.
    pub part: String,
    /// Değişen hücreler (kaydederken yeniden yazılır).
    pub dirty: HashSet<(u32, u32)>,
}

impl Sheet {
    pub fn new(name: &str) -> Self {
        Sheet { name: name.to_string(), ..Default::default() }
    }

    pub fn extent(&self) -> (u32, u32) {
        let mut r = 0;
        let mut c = 0;
        for (k, cell) in &self.cells {
            if cell.v != Val::Empty || cell.f.is_some() {
                r = r.max(k.0 + 1);
                c = c.max(k.1 + 1);
            }
        }
        (r, c)
    }

    pub fn get(&self, r: u32, c: u32) -> Option<&Cell> {
        self.cells.get(&(r, c))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Xlsx,
    /// Eski Excel: kaydederken xlsx'e dönüşür.
    Xls,
    Csv,
}

#[derive(Clone, Default)]
pub struct Book {
    pub sheets: Vec<Sheet>,
    /// Stil dizini → sayı biçim kodu.
    pub numfmt: Vec<String>,
    pub date1904: bool,
    pub kind: Option<Kind>,
    pub zip: Option<crate::zip::Zip>,
    pub csv: csv::Meta,
    /// Stil dizini → (kalın, yatay hizalama 0/1/2/3, dolgu rengi, yazı rengi).
    pub looks: Vec<Look>,
    /// styles.xml içinde zaten olan xf sayısı; fazlası kaydederken eklenir.
    pub xf_base: usize,
    /// Kaydederken eklenecek stil → türediği (dosyadaki) stil.
    pub style_src: HashMap<u32, u32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Look {
    pub bold: bool,
    pub italic: bool,
    /// 0: genel, 1: sol, 2: orta, 3: sağ.
    pub align: u8,
    pub fill: Option<u32>,
    pub color: Option<u32>,
    pub wrap: bool,
}

/// "Genel" biçimli sayı: en çok 10 anlamlı basamak, gereksiz sıfırlar yok.
pub fn general(n: f64) -> String {
    if !n.is_finite() {
        return "#NUM!".into();
    }
    let s = if n != 0.0 && (n.abs() >= 1e11 || n.abs() < 1e-9) {
        format!("{n:.5E}")
    } else {
        let digits = 10 - (n.abs().log10().floor() as i32 + 1).clamp(0, 10);
        let mut s = format!("{:.*}", digits.max(0) as usize, n);
        if s.contains('.') {
            s = s.trim_end_matches('0').trim_end_matches('.').to_string();
        }
        if s == "-0" {
            s = "0".into();
        }
        s
    };
    if carlui_core::lang::is_tr() {
        s.replace('.', ",")
    } else {
        s
    }
}

/// Kullanıcının yazdığı metni değere çevirir: sayı, yüzde, tarih, mantıksal.
pub fn parse_input(s: &str) -> Val {
    let t = s.trim();
    if t.is_empty() {
        return Val::Empty;
    }
    let tr = carlui_core::lang::is_tr();
    let norm = |x: &str| -> Option<f64> {
        let x = x.replace(' ', "");
        let x = if tr {
            // 1.234,5 → 1234.5 ; "3.5" de kabul edilir.
            if x.contains(',') {
                x.replace('.', "").replace(',', ".")
            } else if x.matches('.').count() > 1 {
                x.replace('.', "")
            } else {
                x
            }
        } else {
            x.replace(',', "")
        };
        if x.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')) && x.chars().any(|c| c.is_ascii_digit()) {
            x.parse().ok()
        } else {
            None
        }
    };
    if let Some(n) = norm(t) {
        return Val::Num(n);
    }
    if let Some(p) = t.strip_suffix('%').or_else(|| t.strip_prefix('%')) {
        if let Some(n) = norm(p) {
            return Val::Num(n / 100.0);
        }
    }
    match t.to_uppercase().as_str() {
        "TRUE" | "DOĞRU" => return Val::Bool(true),
        "FALSE" | "YANLIŞ" => return Val::Bool(false),
        _ => {}
    }
    Val::Str(s.to_string())
}

/// Tarih girişi (gg.aa.yyyy) → seri gün.
pub fn parse_date(s: &str) -> Option<f64> {
    let p: Vec<&str> = s.trim().split(['.', '/', '-']).collect();
    if p.len() != 3 {
        return None;
    }
    let (a, b, c): (u32, u32, i64) = (p[0].parse().ok()?, p[1].parse().ok()?, p[2].parse().ok()?);
    let (d, m, y) = if p[0].len() == 4 { (c as u32, b, a as i64) } else { (a, b, if c < 100 { 2000 + c } else { c }) };
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(numfmt::serial(y, m, d))
}

impl Book {
    pub fn new_blank() -> Self {
        Book { sheets: vec![Sheet::new(carlui_core::t!("Sayfa1", "Sheet1"))], numfmt: vec!["General".into()], looks: vec![Look::default()], ..Default::default() }
    }

    pub fn fmt_code(&self, s: u32) -> &str {
        self.numfmt.get(s as usize).map(|x| x.as_str()).unwrap_or("General")
    }

    pub fn look(&self, s: u32) -> Look {
        self.looks.get(s as usize).copied().unwrap_or_default()
    }

    pub fn sheet_index(&self, name: &str) -> Option<usize> {
        self.sheets.iter().position(|s| s.name.eq_ignore_ascii_case(name))
    }

    /// Hücrenin ekranda görünen metni.
    pub fn display(&self, sh: usize, r: u32, c: u32) -> String {
        let Some(cell) = self.sheets[sh].get(r, c) else { return String::new() };
        match &cell.v {
            Val::Empty => String::new(),
            Val::Num(n) => numfmt::format(*n, self.fmt_code(cell.s), self.date1904),
            Val::Str(s) => s.clone(),
            Val::Bool(b) => if *b { carlui_core::t!("DOĞRU", "TRUE") } else { carlui_core::t!("YANLIŞ", "FALSE") }.into(),
            Val::Err(e) => e.clone(),
        }
    }

    /// Formül çubuğunda düzenlenecek metin.
    pub fn edit_text(&self, sh: usize, r: u32, c: u32) -> String {
        let Some(cell) = self.sheets[sh].get(r, c) else { return String::new() };
        if let Some(f) = &cell.f {
            return format!("={f}");
        }
        match &cell.v {
            Val::Num(n) => {
                let code = self.fmt_code(cell.s);
                if numfmt::is_date(code) && !code.eq_ignore_ascii_case("general") {
                    let (y, m, d) = numfmt::ymd(*n, self.date1904);
                    return format!("{d:02}.{m:02}.{y}");
                }
                if code.contains('%') {
                    return format!("{}%", general(n * 100.0));
                }
                general(*n)
            }
            _ => self.display(sh, r, c),
        }
    }

    /// Kullanıcı girdisini hücreye yazar ve yeniden hesaplar.
    pub fn set_input(&mut self, sh: usize, r: u32, c: u32, input: &str) {
        let style = self.sheets[sh].get(r, c).map(|x| x.s).unwrap_or(0);
        let mut cell = Cell { s: style, ..Default::default() };
        if let Some(f) = input.strip_prefix('=').filter(|f| !f.trim().is_empty()) {
            cell.f = Some(formula::normalize(f));
        } else if let Some(d) = parse_date(input).filter(|_| input.contains('.') && input.matches('.').count() == 2 || input.contains('/')) {
            cell.v = Val::Num(d);
            if !numfmt::is_date(self.fmt_code(style)) {
                cell.s = self.date_style();
            }
        } else {
            cell.v = parse_input(input);
            if input.contains('%') && !self.fmt_code(style).contains('%') {
                cell.s = self.pct_style();
            }
        }
        self.put(sh, r, c, cell);
        self.recalc();
    }

    /// Hücreyi değiştirir (stil korunur), paylaşılan formül grubunu açar.
    pub fn put(&mut self, sh: usize, r: u32, c: u32, cell: Cell) {
        let s = &mut self.sheets[sh];
        if let Some(si) = s.cells.get(&(r, c)).and_then(|x| x.si) {
            // Grubun tüm hücreleri artık kendi formülüyle yazılmalı.
            let keys: Vec<_> = s.cells.iter().filter(|(_, x)| x.si == Some(si)).map(|(k, _)| *k).collect();
            for k in keys {
                if let Some(x) = s.cells.get_mut(&k) {
                    x.si = None;
                    x.raw = None;
                }
                s.dirty.insert(k);
            }
        }
        s.dirty.insert((r, c));
        if cell.v == Val::Empty && cell.f.is_none() && cell.s == 0 {
            s.cells.remove(&(r, c));
        } else {
            s.cells.insert((r, c), cell);
        }
    }

    pub(crate) fn style_like(&mut self, code: &str) -> u32 {
        if let Some(i) = self.numfmt.iter().position(|c| c == code) {
            return i as u32;
        }
        // xlsx'te yeni stil eklemek styles.xml'e dokunmayı gerektirir; kaydederken
        // `xlsx::save` eksik stilleri oluşturur.
        self.looks.resize(self.numfmt.len(), Look::default());
        self.numfmt.push(code.to_string());
        self.looks.push(Look::default());
        (self.numfmt.len() - 1) as u32
    }

    /// `s` stilinin görünümü `f` ile (ve istenirse sayı biçimi `code` ile)
    /// değişmiş halinin stil dizini. Aynısı varsa o kullanılır; yoksa yeni stil
    /// eklenir ve kaydederken özgün stilin yazı tipi/kenarlığı korunur.
    pub fn restyle(&mut self, s: u32, code: Option<&str>, f: impl Fn(&mut Look)) -> u32 {
        let mut look = self.look(s);
        f(&mut look);
        let code = code.map(String::from).unwrap_or_else(|| self.fmt_code(s).to_string());
        if look == self.look(s) && code == self.fmt_code(s) {
            return s;
        }
        let root = |b: &Book, i: u32| b.style_src.get(&i).copied().unwrap_or(i);
        let r = root(self, s);
        if let Some(i) = (0..self.numfmt.len() as u32).find(|i| root(self, *i) == r && self.numfmt[*i as usize] == code && self.look(*i) == look) {
            return i;
        }
        self.looks.resize(self.numfmt.len(), Look::default());
        self.numfmt.push(code);
        self.looks.push(look);
        let n = (self.numfmt.len() - 1) as u32;
        self.style_src.insert(n, r);
        n
    }

    fn date_style(&mut self) -> u32 {
        if let Some(i) = self.numfmt.iter().position(|c| numfmt::is_date(c) && !c.contains('h')) {
            return i as u32;
        }
        self.style_like("dd.mm.yyyy")
    }

    fn pct_style(&mut self) -> u32 {
        if let Some(i) = self.numfmt.iter().position(|c| c == "0%" || c == "0.00%") {
            return i as u32;
        }
        self.style_like("0%")
    }

    /// Tüm formülleri yeniden hesaplar. Desteklemediğimiz işlev çıkarsa
    /// dosyadaki son değer korunur.
    pub fn recalc(&mut self) {
        let mut targets = Vec::new();
        for (si, s) in self.sheets.iter().enumerate() {
            for (k, c) in &s.cells {
                if c.f.is_some() {
                    targets.push((si, k.0, k.1));
                }
            }
        }
        if targets.is_empty() {
            return;
        }
        // Uzun bağımlılık zinciri derin özyineleme demek: geniş yığında hesapla.
        let book: &Book = self;
        let results = crate::guard::deep(|| {
            let mut calc = Calc { book, memo: HashMap::new(), stack: HashSet::new(), sheet: 0 };
            targets.iter().map(|t| {
                calc.sheet = t.0;
                calc.cell(t.0, t.1, t.2)
            }).collect::<Vec<_>>()
        });
        for (t, v) in targets.into_iter().zip(results) {
            if let Some(c) = self.sheets[t.0].cells.get_mut(&(t.1, t.2)) {
                if v == Val::Err(formula::UNSUPPORTED.into()) && c.v != Val::Empty {
                    continue;
                }
                if c.v != v {
                    c.v = v;
                    // Formüllü hücrenin önbellek değeri değişti: ham hali eskidi.
                    if c.si.is_none() {
                        c.raw = None;
                        self.sheets[t.0].dirty.insert((t.1, t.2));
                    }
                }
            }
        }
    }
}

struct Calc<'a> {
    book: &'a Book,
    memo: HashMap<(usize, u32, u32), Val>,
    stack: HashSet<(usize, u32, u32)>,
    sheet: usize,
}

impl Calc<'_> {
    fn cell(&mut self, sh: usize, r: u32, c: u32) -> Val {
        let key = (sh, r, c);
        if let Some(v) = self.memo.get(&key) {
            return v.clone();
        }
        let Some(cell) = self.book.sheets[sh].get(r, c) else { return Val::Empty };
        let Some(f) = &cell.f else { return cell.v.clone() };
        if self.stack.len() >= crate::guard::CALC_DEPTH || !self.stack.insert(key) {
            return Val::Err("#CIRC!".into());
        }
        let prev = self.sheet;
        self.sheet = sh;
        let v = match formula::parse(f) {
            Ok(e) => formula::eval(&e, self),
            Err(_) => Val::Err(formula::UNSUPPORTED.into()),
        };
        // Desteklenmeyen işlevde dosyadaki değeri kullan (zincir kopmasın).
        let v = if v == Val::Err(formula::UNSUPPORTED.into()) && cell.v != Val::Empty { cell.v.clone() } else { v };
        self.sheet = prev;
        self.stack.remove(&key);
        self.memo.insert(key, v.clone());
        v
    }

    fn sheet_of(&self, name: Option<&str>) -> Option<usize> {
        match name {
            None => Some(self.sheet),
            Some(n) => self.book.sheet_index(n),
        }
    }
}

impl formula::Ctx for Calc<'_> {
    fn get(&mut self, sheet: Option<&str>, r: u32, c: u32) -> Val {
        match self.sheet_of(sheet) {
            Some(sh) => self.cell(sh, r, c),
            None => Val::Err("#REF!".into()),
        }
    }

    fn range(&mut self, sheet: Option<&str>, a: (u32, u32), b: (u32, u32)) -> Vec<Val> {
        let Some(sh) = self.sheet_of(sheet) else { return vec![Val::Err("#REF!".into())] };
        let (r0, r1, c0, c1) = (a.0.min(b.0), a.0.max(b.0), a.1.min(b.1), a.1.max(b.1));
        let keys: Vec<(u32, u32)> = self.book.sheets[sh].cells.range((r0, 0)..=(r1, u32::MAX)).filter(|(k, _)| k.1 >= c0 && k.1 <= c1).map(|(k, _)| *k).collect();
        keys.into_iter().map(|k| self.cell(sh, k.0, k.1)).collect()
    }

    fn grid(&mut self, sheet: Option<&str>, a: (u32, u32), b: (u32, u32)) -> Vec<Vec<Val>> {
        let Some(sh) = self.sheet_of(sheet) else { return Vec::new() };
        let (ext_r, ext_c) = self.book.sheets[sh].extent();
        let (r0, c0) = (a.0.min(b.0), a.1.min(b.1));
        let r1 = a.0.max(b.0).min(ext_r.max(r0 + 1) - 1);
        let c1 = a.1.max(b.1).min(ext_c.max(c0 + 1) - 1);
        // Uzak köşeye tek hücre koyup dev bir aralık açtırmak belleği tüketirdi.
        if (r1 - r0 + 1) as u64 * (c1 - c0 + 1) as u64 > crate::guard::GRID_CELLS {
            return vec![vec![Val::Err("#NUM!".into())]];
        }
        (r0..=r1).map(|r| (c0..=c1).map(|c| self.cell(sh, r, c)).collect()).collect()
    }
}

/// Geri alma kaydı: değişmeden önceki hücreler.
#[derive(Clone, Default)]
pub struct Undo {
    pub sheet: usize,
    pub cells: Vec<((u32, u32), Option<Cell>)>,
}

impl Book {
    pub fn snapshot(&self, sh: usize, keys: impl IntoIterator<Item = (u32, u32)>) -> Undo {
        Undo { sheet: sh, cells: keys.into_iter().map(|k| (k, self.sheets[sh].cells.get(&k).cloned())).collect() }
    }

    pub fn restore(&mut self, u: Undo) -> Undo {
        let back = self.snapshot(u.sheet, u.cells.iter().map(|(k, _)| *k));
        let s = &mut self.sheets[u.sheet];
        for (k, c) in u.cells {
            s.dirty.insert(k);
            match c {
                Some(mut c) => {
                    c.raw = None;
                    s.cells.insert(k, c);
                }
                None => {
                    s.cells.remove(&k);
                }
            }
        }
        self.recalc();
        back
    }

    /// Seçili aralığı sekmeyle ayrılmış metne çevirir (Excel ile uyumlu pano).
    pub fn copy_tsv(&self, sh: usize, a: (u32, u32), b: (u32, u32)) -> String {
        let mut out = String::new();
        for r in a.0..=b.0 {
            let row: Vec<String> = (a.1..=b.1).map(|c| self.display(sh, r, c)).collect();
            out.push_str(&row.join("\t"));
            out.push_str("\r\n");
        }
        out
    }

    /// Sekmeli metni (r, c)'den başlayarak yapıştırır; geri alma kaydını döndürür.
    pub fn paste_tsv(&mut self, sh: usize, r: u32, c: u32, text: &str) -> Undo {
        let rows: Vec<Vec<&str>> = text.trim_end_matches(['\r', '\n']).split('\n').map(|l| l.trim_end_matches('\r').split('\t').collect()).collect();
        let keys: Vec<(u32, u32)> = rows.iter().enumerate().flat_map(|(i, row)| (0..row.len()).map(move |j| (r + i as u32, c + j as u32))).collect();
        let undo = self.snapshot(sh, keys);
        for (i, row) in rows.iter().enumerate() {
            for (j, v) in row.iter().enumerate() {
                let (rr, cc) = (r + i as u32, c + j as u32);
                let style = self.sheets[sh].get(rr, cc).map(|x| x.s).unwrap_or(0);
                let mut cell = Cell { s: style, ..Default::default() };
                if let Some(f) = v.strip_prefix('=') {
                    cell.f = Some(formula::normalize(f));
                } else {
                    cell.v = parse_input(v);
                }
                self.put(sh, rr, cc, cell);
            }
        }
        self.recalc();
        undo
    }

    pub fn clear(&mut self, sh: usize, a: (u32, u32), b: (u32, u32)) -> Undo {
        let keys: Vec<(u32, u32)> = self.sheets[sh].cells.range((a.0, 0)..=(b.0, u32::MAX)).filter(|(k, _)| k.1 >= a.1 && k.1 <= b.1).map(|(k, _)| *k).collect();
        let undo = self.snapshot(sh, keys.clone());
        for k in keys {
            let s = self.sheets[sh].get(k.0, k.1).map(|x| x.s).unwrap_or(0);
            self.put(sh, k.0, k.1, Cell { s, ..Default::default() });
        }
        self.recalc();
        undo
    }

    pub fn add_sheet(&mut self) -> usize {
        let mut n = self.sheets.len() + 1;
        let base = carlui_core::t!("Sayfa", "Sheet");
        while self.sheet_index(&format!("{base}{n}")).is_some() {
            n += 1;
        }
        self.sheets.push(Sheet::new(&format!("{base}{n}")));
        self.sheets.len() - 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_and_recalc() {
        let _lang = crate::test_lang("tr");
        let mut b = Book::new_blank();
        b.set_input(0, 0, 0, "1.234,5");
        assert_eq!(b.sheets[0].get(0, 0).unwrap().v, Val::Num(1234.5));
        b.set_input(0, 1, 0, "10");
        b.set_input(0, 2, 0, "=TOPLA(A1:A2)");
        assert_eq!(b.sheets[0].get(2, 0).unwrap().f.as_deref(), Some("SUM(A1:A2)"));
        assert_eq!(b.sheets[0].get(2, 0).unwrap().v, Val::Num(1244.5));
        b.set_input(0, 1, 0, "20");
        assert_eq!(b.sheets[0].get(2, 0).unwrap().v, Val::Num(1254.5));
        b.set_input(0, 3, 0, "=A4+1");
        assert_eq!(b.sheets[0].get(3, 0).unwrap().v, Val::Err("#CIRC!".into()));
        b.set_input(0, 4, 0, "01.11.2023");
        assert_eq!(b.display(0, 4, 0), "01.11.2023");
        b.set_input(0, 5, 0, "%15");
        assert_eq!(b.display(0, 5, 0), "15%");
        let u = b.paste_tsv(0, 10, 0, "a\tb\r\n1\t2\r\n");
        assert_eq!(b.display(0, 11, 1), "2");
        b.restore(u);
        assert!(b.sheets[0].get(11, 1).is_none());
        assert_eq!(general(0.1 + 0.2), "0,3");
    }

    /// Kötü niyetli tablo: tersine uzun zincir, aşırı derin formül, dev aralık.
    /// Hiçbiri süreci düşürmemeli.
    #[test]
    fn hostile_book() {
        let mut b = Book::new_blank();
        let n = 30_000u32;
        for r in 0..n {
            let f = if r + 1 < n { format!("A{}+1", r + 2) } else { "1".into() };
            b.sheets[0].cells.insert((r, 0), Cell { f: Some(f), ..Default::default() });
        }
        let deep = format!("{}1{}", "(".repeat(5000), ")".repeat(5000));
        b.sheets[0].cells.insert((0, 1), Cell { f: Some(deep), ..Default::default() });
        b.sheets[0].cells.insert((1_048_575, 16_383), Cell { v: Val::Num(1.0), ..Default::default() });
        b.sheets[0].cells.insert((1, 1), Cell { f: Some("COUNTBLANK(A1:XFD1048576)".into()), ..Default::default() });
        b.recalc();
        assert_eq!(b.sheets[0].get(0, 0).unwrap().v, Val::Num(n as f64));
        assert!(matches!(b.sheets[0].get(0, 1).unwrap().v, Val::Err(_)));
        // Dev aralık 17 milyar hücre açmadan biter (sonuç önemli değil).
        assert!(b.sheets[0].get(1, 1).is_some());
    }
}
