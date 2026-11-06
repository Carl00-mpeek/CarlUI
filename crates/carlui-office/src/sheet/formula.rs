//! Bu modül "tablodaki formülleri (=TOPLA(A1:A5)*2 gibi) hesaplayalım"
//! sorununu çözüyor: belirteçlere ayırma, ayrıştırma ve değerlendirme. Dosyada
//! işlev adları her zaman İngilizce tutulur; kullanıcı Türkçe Excel adlarını
//! (TOPLA, EĞER…) ve `;` ayırıcısını yazabilir, `normalize` bunları çevirir.
//! Paylaşılan formüller (xlsx `t="shared"`) `shift` ile her hücreye açılır.



#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Num(f64, String),
    Str(String),
    Ident(String),
    /// Tırnaklı sayfa adı: 'Sayfa 1'.
    Quoted(String),
    Op(String),
    LParen,
    RParen,
    Sep,
    Colon,
    Bang,
    Err(String),
}

pub fn tokenize(s: &str) -> Result<Vec<Tok>, String> {
    let c: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < c.len() {
        let ch = c[i];
        match ch {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            '"' => {
                let mut t = String::new();
                i += 1;
                loop {
                    if i >= c.len() {
                        return Err("tırnak kapanmadı".into());
                    }
                    if c[i] == '"' {
                        if c.get(i + 1) == Some(&'"') {
                            t.push('"');
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    t.push(c[i]);
                    i += 1;
                }
                out.push(Tok::Str(t));
            }
            '\'' => {
                let mut t = String::new();
                i += 1;
                while i < c.len() {
                    if c[i] == '\'' {
                        if c.get(i + 1) == Some(&'\'') {
                            t.push('\'');
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    t.push(c[i]);
                    i += 1;
                }
                i += 1;
                out.push(Tok::Quoted(t));
            }
            '#' => {
                let rest: String = c[i..].iter().take(8).collect::<String>().to_uppercase();
                let known = ["#DIV/0!", "#N/A", "#REF!", "#NAME?", "#VALUE!", "#NUM!", "#NULL!"];
                let Some(k) = known.iter().find(|k| rest.starts_with(*k)) else { return Err("bilinmeyen hata değeri".into()) };
                i += k.chars().count();
                out.push(Tok::Err(k.to_string()));
            }
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            ',' | ';' => {
                out.push(Tok::Sep);
                i += 1;
            }
            ':' => {
                out.push(Tok::Colon);
                i += 1;
            }
            '!' => {
                out.push(Tok::Bang);
                i += 1;
            }
            '<' | '>' => {
                let mut op = ch.to_string();
                if let Some(&n) = c.get(i + 1) {
                    if n == '=' || (ch == '<' && n == '>') {
                        op.push(n);
                    }
                }
                i += op.chars().count();
                out.push(Tok::Op(op));
            }
            '+' | '-' | '*' | '/' | '^' | '&' | '=' | '%' => {
                out.push(Tok::Op(ch.to_string()));
                i += 1;
            }
            d if d.is_ascii_digit() || (d == '.' && c.get(i + 1).is_some_and(|x| x.is_ascii_digit())) => {
                let st = i;
                while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.') {
                    i += 1;
                }
                if i < c.len() && (c[i] == 'E' || c[i] == 'e') && c.get(i + 1).is_some_and(|x| x.is_ascii_digit() || *x == '+' || *x == '-') {
                    i += 2;
                    while i < c.len() && c[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let t: String = c[st..i].iter().collect();
                // "1:3" gibi satır aralığı ya da sayı.
                out.push(Tok::Num(t.parse().map_err(|_| format!("sayı: {t}"))?, t));
            }
            _ if ch.is_alphanumeric() || ch == '_' || ch == '$' || ch == '\\' => {
                let st = i;
                while i < c.len() && (c[i].is_alphanumeric() || matches!(c[i], '_' | '$' | '.' | '\\')) {
                    i += 1;
                }
                out.push(Tok::Ident(c[st..i].iter().collect()));
            }
            _ => return Err(format!("beklenmeyen karakter: {ch}")),
        }
    }
    Ok(out)
}

/// "B12" / "$B$12" → (sütun, satır, sütun sabit, satır sabit); 0 tabanlı.
pub fn parse_ref(s: &str) -> Option<(u32, u32, bool, bool)> {
    let b = s.as_bytes();
    let mut i = 0;
    let ca = b.first() == Some(&b'$');
    if ca {
        i += 1;
    }
    let cs = i;
    while i < b.len() && b[i].is_ascii_alphabetic() {
        i += 1;
    }
    if i == cs || i - cs > 3 {
        return None;
    }
    let col = col_from(&s[cs..i])?;
    let ra = b.get(i) == Some(&b'$');
    if ra {
        i += 1;
    }
    if i >= b.len() || !b[i..].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let row: u32 = s[i..].parse().ok()?;
    if row == 0 || row > 1_048_576 || col >= 16_384 {
        return None;
    }
    Some((col, row - 1, ca, ra))
}

pub fn col_from(s: &str) -> Option<u32> {
    let mut n = 0u32;
    for ch in s.chars() {
        let u = ch.to_ascii_uppercase();
        if !u.is_ascii_uppercase() {
            return None;
        }
        n = n * 26 + (u as u32 - 'A' as u32 + 1);
    }
    n.checked_sub(1)
}

pub fn col_name(mut c: u32) -> String {
    let mut s = Vec::new();
    loop {
        s.push((b'A' + (c % 26) as u8) as char);
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    s.iter().rev().collect()
}

pub fn cell_name(r: u32, c: u32) -> String {
    format!("{}{}", col_name(c), r + 1)
}

#[derive(Clone, Debug)]
pub enum Expr {
    Num(f64),
    Str(String),
    Bool(bool),
    Err(String),
    /// (sayfa, satır, sütun)
    Ref(Option<String>, u32, u32),
    Range(Option<String>, (u32, u32), (u32, u32)),
    Un(String, Box<Expr>),
    Bin(String, Box<Expr>, Box<Expr>),
    Pct(Box<Expr>),
    Call(String, Vec<Expr>),
}

struct P<'a> {
    t: &'a [Tok],
    i: usize,
    /// İç içe parantez/işlev/işaret düzeyi (aşırısı yığını taşırırdı).
    depth: usize,
}

impl P<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.t.get(self.i).cloned();
        self.i += 1;
        t
    }
    fn op(&self, ops: &[&str]) -> Option<String> {
        match self.peek() {
            Some(Tok::Op(o)) if ops.contains(&o.as_str()) => Some(o.clone()),
            _ => None,
        }
    }

    fn expr(&mut self) -> Result<Expr, String> {
        let mut l = self.concat()?;
        while let Some(o) = self.op(&["=", "<>", "<", ">", "<=", ">="]) {
            self.i += 1;
            let r = self.concat()?;
            l = Expr::Bin(o, Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn concat(&mut self) -> Result<Expr, String> {
        let mut l = self.add()?;
        while self.op(&["&"]).is_some() {
            self.i += 1;
            let r = self.add()?;
            l = Expr::Bin("&".into(), Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn add(&mut self) -> Result<Expr, String> {
        let mut l = self.mul()?;
        while let Some(o) = self.op(&["+", "-"]) {
            self.i += 1;
            let r = self.mul()?;
            l = Expr::Bin(o, Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn mul(&mut self) -> Result<Expr, String> {
        let mut l = self.pow()?;
        while let Some(o) = self.op(&["*", "/"]) {
            self.i += 1;
            let r = self.pow()?;
            l = Expr::Bin(o, Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn pow(&mut self) -> Result<Expr, String> {
        let mut l = self.unary()?;
        while self.op(&["^"]).is_some() {
            self.i += 1;
            let r = self.unary()?;
            l = Expr::Bin("^".into(), Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn unary(&mut self) -> Result<Expr, String> {
        self.depth += 1;
        if self.depth > crate::guard::FORMULA_NEST {
            return Err("formül çok derin".into());
        }
        let r = self.unary_inner();
        self.depth -= 1;
        r
    }
    fn unary_inner(&mut self) -> Result<Expr, String> {
        if let Some(o) = self.op(&["-", "+"]) {
            self.i += 1;
            let e = self.unary()?;
            return Ok(Expr::Un(o, Box::new(e)));
        }
        let mut e = self.primary()?;
        while self.op(&["%"]).is_some() {
            self.i += 1;
            e = Expr::Pct(Box::new(e));
        }
        Ok(e)
    }

    fn reference(&mut self, sheet: Option<String>, first: &str) -> Result<Expr, String> {
        if self.peek() == Some(&Tok::Colon) {
            self.i += 1;
            let second = match self.next() {
                Some(Tok::Ident(s)) => s,
                Some(Tok::Num(_, s)) => s,
                _ => return Err("aralık bozuk".into()),
            };
            if let (Some(a), Some(b)) = (parse_ref(first), parse_ref(&second)) {
                return Ok(Expr::Range(sheet, (a.1, a.0), (b.1, b.0)));
            }
            // Sütun aralığı (A:C) ya da satır aralığı (1:3).
            let (fa, sa) = (first.trim_start_matches('$'), second.trim_start_matches('$'));
            if let (Some(a), Some(b)) = (col_from(fa), col_from(sa)) {
                return Ok(Expr::Range(sheet, (0, a), (1_048_575, b)));
            }
            if let (Ok(a), Ok(b)) = (fa.parse::<u32>(), sa.parse::<u32>()) {
                if a > 0 && b > 0 {
                    return Ok(Expr::Range(sheet, (a - 1, 0), (b - 1, 16_383)));
                }
            }
            return Err("aralık bozuk".into());
        }
        match parse_ref(first) {
            Some((c, r, _, _)) => Ok(Expr::Ref(sheet, r, c)),
            None => Err(format!("tanınmayan ad: {first}")),
        }
    }

    fn primary(&mut self) -> Result<Expr, String> {
        match self.next() {
            Some(Tok::Num(n, s)) => {
                if self.peek() == Some(&Tok::Colon) {
                    return self.reference(None, &s);
                }
                Ok(Expr::Num(n))
            }
            Some(Tok::Str(s)) => Ok(Expr::Str(s)),
            Some(Tok::Err(e)) => Ok(Expr::Err(e)),
            Some(Tok::LParen) => {
                let e = self.expr()?;
                if self.next() != Some(Tok::RParen) {
                    return Err("parantez kapanmadı".into());
                }
                Ok(e)
            }
            Some(Tok::Quoted(sheet)) => {
                if self.next() != Some(Tok::Bang) {
                    return Err("sayfa adından sonra ! bekleniyordu".into());
                }
                match self.next() {
                    Some(Tok::Ident(r)) => self.reference(Some(sheet), &r),
                    _ => Err("hücre bekleniyordu".into()),
                }
            }
            Some(Tok::Ident(id)) => {
                if self.peek() == Some(&Tok::Bang) {
                    self.i += 1;
                    return match self.next() {
                        Some(Tok::Ident(r)) => self.reference(Some(id), &r),
                        _ => Err("hücre bekleniyordu".into()),
                    };
                }
                if self.peek() == Some(&Tok::LParen) {
                    self.i += 1;
                    let mut args = Vec::new();
                    if self.peek() == Some(&Tok::RParen) {
                        self.i += 1;
                    } else {
                        loop {
                            // Boş bağımsız değişken: EĞER(A1;;2)
                            if matches!(self.peek(), Some(Tok::Sep) | Some(Tok::RParen)) {
                                args.push(Expr::Str(String::new()));
                            } else {
                                args.push(self.expr()?);
                            }
                            match self.next() {
                                Some(Tok::Sep) => continue,
                                Some(Tok::RParen) => break,
                                _ => return Err("işlev parantezi kapanmadı".into()),
                            }
                        }
                    }
                    let name = id.to_uppercase();
                    let name = name.strip_prefix("_XLFN.").unwrap_or(&name).to_string();
                    return Ok(Expr::Call(name, args));
                }
                match id.to_uppercase().as_str() {
                    "TRUE" | "DOĞRU" => return Ok(Expr::Bool(true)),
                    "FALSE" | "YANLIŞ" => return Ok(Expr::Bool(false)),
                    _ => {}
                }
                self.reference(None, &id)
            }
            _ => Err("ifade bekleniyordu".into()),
        }
    }
}

pub fn parse(src: &str) -> Result<Expr, String> {
    if src.len() > crate::guard::FORMULA_LEN {
        return Err("formül çok uzun".into());
    }
    let t = tokenize(src)?;
    let mut p = P { t: &t, i: 0, depth: 0 };
    let e = p.expr()?;
    if p.i < t.len() {
        return Err("fazladan ifade".into());
    }
    Ok(e)
}

/// Türkçe Excel işlev adları → dosyada kullanılan İngilizce adlar.
const TR_NAMES: &[(&str, &str)] = &[
    ("TOPLA", "SUM"),
    ("ORTALAMA", "AVERAGE"),
    ("MİN", "MIN"),
    ("MAK", "MAX"),
    ("BAĞ_DEĞ_SAY", "COUNT"),
    ("BAĞ_DEĞ_DOLU_SAY", "COUNTA"),
    ("EĞER", "IF"),
    ("YUVARLA", "ROUND"),
    ("YUKARIYUVARLA", "ROUNDUP"),
    ("AŞAĞIYUVARLA", "ROUNDDOWN"),
    ("MUTLAK", "ABS"),
    ("VE", "AND"),
    ("YADA", "OR"),
    ("DEĞİL", "NOT"),
    ("BİRLEŞTİR", "CONCATENATE"),
    ("ARALIKBİRLEŞTİR", "CONCAT"),
    ("UZUNLUK", "LEN"),
    ("BÜYÜKHARF", "UPPER"),
    ("KÜÇÜKHARF", "LOWER"),
    ("KIRP", "TRIM"),
    ("SOLDAN", "LEFT"),
    ("SAĞDAN", "RIGHT"),
    ("PARÇAAL", "MID"),
    ("EĞERHATA", "IFERROR"),
    ("ÇARPIM", "PRODUCT"),
    ("KAREKÖK", "SQRT"),
    ("TAMSAYI", "INT"),
    ("KUVVET", "POWER"),
    ("ETOPLA", "SUMIF"),
    ("EĞERSAY", "COUNTIF"),
    ("BOŞLUKSAY", "COUNTBLANK"),
    ("ORTANCA", "MEDIAN"),
    ("METNEÇEVİR", "TEXT"),
    ("SAYIYAÇEVİR", "VALUE"),
];

fn english(name: &str) -> &str {
    let up = name.to_uppercase();
    TR_NAMES.iter().find(|(tr, _)| *tr == up).map(|(_, en)| *en).unwrap_or(name)
}

fn quote_sheet(s: &str) -> String {
    if s.chars().all(|c| c.is_alphanumeric() || c == '_') && !s.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "''"))
    }
}

fn join(toks: &[Tok]) -> String {
    let mut s = String::new();
    for t in toks {
        match t {
            Tok::Num(_, raw) => s.push_str(raw),
            Tok::Str(x) => {
                s.push('"');
                s.push_str(&x.replace('"', "\"\""));
                s.push('"');
            }
            Tok::Ident(x) | Tok::Err(x) | Tok::Op(x) => s.push_str(x),
            Tok::Quoted(x) => s.push_str(&quote_sheet(x)),
            Tok::LParen => s.push('('),
            Tok::RParen => s.push(')'),
            Tok::Sep => s.push(','),
            Tok::Colon => s.push(':'),
            Tok::Bang => s.push('!'),
        }
    }
    s
}

/// Hücreye yazılan metnin formül gövdesini verir (başındaki `=` olmadan).
/// Olağan biçim `=A1+B5`; hesap makinesi alışkanlığıyla yazılan `a1+b5=` de
/// kabul edilir. Sondaki `=` yalnızca gövde gerçekten hücre başvurulu bir işlemse
/// formül sayılır; "Toplam=" ya da "x1=" gibi etiketler metin kalır.
pub fn typed(input: &str) -> Option<&str> {
    if let Some(f) = input.strip_prefix('=') {
        return Some(f).filter(|f| !f.trim().is_empty());
    }
    let body = input.trim().strip_suffix('=')?;
    let t = tokenize(body).ok()?;
    let has_ref = t.iter().any(|x| matches!(x, Tok::Ident(id) if parse_ref(id).is_some()));
    let acts = t.iter().any(|x| matches!(x, Tok::Op(_) | Tok::LParen));
    (has_ref && acts && parse(body).is_ok()).then_some(body)
}

/// Kullanıcı girdisini dosya biçimine çevirir: Türkçe adlar → İngilizce,
/// `;` → `,`. Başındaki `=` olmadan verilir.
pub fn normalize(src: &str) -> String {
    let Ok(mut t) = tokenize(src) else { return src.to_string() };
    for i in 0..t.len() {
        let is_call = t.get(i + 1) == Some(&Tok::LParen);
        if let Tok::Ident(id) = &t[i] {
            if is_call {
                t[i] = Tok::Ident(english(id).to_uppercase());
            } else if matches!(id.to_uppercase().as_str(), "DOĞRU") {
                t[i] = Tok::Ident("TRUE".into());
            } else if matches!(id.to_uppercase().as_str(), "YANLIŞ") {
                t[i] = Tok::Ident("FALSE".into());
            } else if parse_ref(id).is_some() {
                t[i] = Tok::Ident(id.to_uppercase());
            }
        }
    }
    join(&t)
}

/// Paylaşılan formülü başka hücreye taşır: göreli başvurular (dr, dc) kayar.
pub fn shift(src: &str, dr: i64, dc: i64) -> String {
    let Ok(mut t) = tokenize(src) else { return src.to_string() };
    for i in 0..t.len() {
        let is_call = t.get(i + 1) == Some(&Tok::LParen);
        if let Tok::Ident(id) = &t[i] {
            if is_call {
                continue;
            }
            if let Some((c, r, ca, ra)) = parse_ref(id) {
                let nc = if ca { c as i64 } else { c as i64 + dc };
                let nr = if ra { r as i64 } else { r as i64 + dr };
                let s = if nc < 0 || nr < 0 {
                    "#REF!".to_string()
                } else {
                    format!("{}{}{}{}", if ca { "$" } else { "" }, col_name(nc as u32), if ra { "$" } else { "" }, nr + 1)
                };
                t[i] = Tok::Ident(s);
            }
        }
    }
    join(&t)
}

pub use super::eval::{eval, Ctx, UNSUPPORTED};


#[cfg(test)]
mod tests {
    use super::*;
    use super::super::Val;
    use std::collections::HashMap;

    struct M(HashMap<(u32, u32), Val>);
    impl Ctx for M {
        fn get(&mut self, _: Option<&str>, r: u32, c: u32) -> Val {
            self.0.get(&(r, c)).cloned().unwrap_or(Val::Empty)
        }
        fn range(&mut self, _: Option<&str>, a: (u32, u32), b: (u32, u32)) -> Vec<Val> {
            self.0.iter().filter(|((r, c), _)| *r >= a.0 && *r <= b.0 && *c >= a.1 && *c <= b.1).map(|(_, v)| v.clone()).collect()
        }
        fn grid(&mut self, s: Option<&str>, a: (u32, u32), b: (u32, u32)) -> Vec<Vec<Val>> {
            (a.0..=b.0).map(|r| (a.1..=b.1).map(|c| self.get(s, r, c)).collect()).collect()
        }
    }

    fn run(f: &str, m: &mut M) -> Val {
        eval(&parse(&normalize(f)).unwrap(), m)
    }

    #[test]
    fn basics() {
        let mut m = M(HashMap::new());
        m.0.insert((0, 0), Val::Num(2.0));
        m.0.insert((1, 0), Val::Num(3.0));
        m.0.insert((2, 0), Val::Str("x".into()));
        assert_eq!(run("SUM(A1:A3)*2", &mut m), Val::Num(10.0));
        assert_eq!(run("TOPLA(A1;A2;5)", &mut m), Val::Num(10.0));
        assert_eq!(run("EĞER(A1>A2;\"büyük\";\"küçük\")", &mut m), Val::Str("küçük".into()));
        assert_eq!(run("1+2*3^2", &mut m), Val::Num(19.0));
        assert_eq!(run("-A1&\"tl\"", &mut m), Val::Str("-2tl".into()));
        assert_eq!(run("1/0", &mut m), Val::Err("#DIV/0!".into()));
        assert_eq!(run("COUNTA(A1:A3)", &mut m), Val::Num(3.0));
        assert_eq!(run("ROUND(2.345,2)", &mut m), Val::Num(2.35));
        assert_eq!(run("50%", &mut m), Val::Num(0.5));
        assert_eq!(run("ETOPLA(A1:A3;\">2\")", &mut m), Val::Num(3.0));
        assert_eq!(run("BÜYÜKHARF(\"istanbul\")", &mut m), Val::Str("İSTANBUL".into()));
        assert_eq!(run("XLOOKUP(1)", &mut m), Val::Err(UNSUPPORTED.into()));
    }

    #[test]
    fn names_and_shift() {
        assert_eq!(normalize("topla(a1;b2)"), "SUM(A1,B2)");
        assert_eq!(shift("A1+$B$2+B$3+'Sayfa 2'!C4", 2, 1), "B3+$B$2+C$3+'Sayfa 2'!D6");
        assert_eq!(col_name(0), "A");
        assert_eq!(col_name(26), "AA");
        assert_eq!(col_from("AB"), Some(27));
        assert_eq!(parse_ref("$C$10"), Some((2, 9, true, true)));
    }
}
