//! Bu modül "arama kutusuna yazılan hesap ya da çeviri" sorununu çözüyor:
//! - Hesap: `2+2*3`, `(5,5 x 2)^2`, `%20 * 150`, `sqrt(16)`, `pi*2`.
//! - Birim: `10 km mil`, `100 f to c`, `5 kg lb`, `2 gb mb`, `90 dk saat`.
//! - Döviz: `100 usd try`, `50 euro tl` (kurun kendisi `currency.rs`'den gelir).
//! Ondalık ayırıcı olarak hem virgül hem nokta kabul edilir.

/// Sayıyı Türkçe biçimde yazar (ondalık virgül, gereksiz sıfırlar atılır).
pub fn fmt(v: f64) -> String {
    if !v.is_finite() {
        return "tanımsız".into();
    }
    if v.abs() >= 1e15 || (v != 0.0 && v.abs() < 1e-9) {
        return format!("{v:e}").replace('.', ",");
    }
    let mut s = format!("{:.10}", v);
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    let (int, frac) = match s.split_once('.') {
        Some((i, f)) => (i.to_string(), Some(f.to_string())),
        None => (s.clone(), None),
    };
    let neg = int.starts_with('-');
    let digits: Vec<char> = int.trim_start_matches('-').chars().collect();
    let mut grouped = String::new();
    for (i, c) in digits.iter().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push('.');
        }
        grouped.push(*c);
    }
    let mut out = if neg { format!("-{grouped}") } else { grouped };
    if let Some(f) = frac {
        out.push(',');
        out.push_str(&f);
    }
    out
}

/// Panoya kopyalanacak sade biçim (binlik ayırıcısız).
pub fn plain(v: f64) -> String {
    let s = format!("{:.10}", v);
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
    s.replace('.', ",")
}

/// "1,5" ve "1.5" ikisi de 1.5; "1.000,5" binlik ayırıcılı yazım da anlaşılır.
pub fn parse_num(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let t = if s.contains(',') && s.contains('.') { s.replace('.', "").replace(',', ".") } else { s.replace(',', ".") };
    t.parse().ok()
}

// ---- İfade değerlendirici ----

struct P<'a> {
    s: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i] == b' ' {
            self.i += 1;
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }
    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        loop {
            match self.peek() {
                Some(b'+') => {
                    self.i += 1;
                    v += self.term()?;
                }
                Some(b'-') => {
                    self.i += 1;
                    v -= self.term()?;
                }
                _ => return Some(v),
            }
        }
    }
    fn term(&mut self) -> Option<f64> {
        let mut v = self.power()?;
        loop {
            match self.peek() {
                Some(b'*') => {
                    self.i += 1;
                    v *= self.power()?;
                }
                Some(b'/') => {
                    self.i += 1;
                    v /= self.power()?;
                }
                Some(b'%') if self.s.get(self.i + 1).is_some_and(|c| *c == b' ' || c.is_ascii_digit() || *c == b'(') => {
                    self.i += 1;
                    v %= self.power()?;
                }
                // Örtük çarpma: 2(3+4), 2pi
                Some(b'(') => v *= self.power()?,
                Some(c) if c.is_ascii_alphabetic() => v *= self.power()?,
                _ => return Some(v),
            }
        }
    }
    fn power(&mut self) -> Option<f64> {
        let b = self.unary()?;
        if self.peek() == Some(b'^') {
            self.i += 1;
            let e = self.power()?;
            return Some(b.powf(e));
        }
        Some(b)
    }
    fn unary(&mut self) -> Option<f64> {
        match self.peek() {
            Some(b'-') => {
                self.i += 1;
                Some(-self.unary()?)
            }
            Some(b'+') => {
                self.i += 1;
                self.unary()
            }
            _ => self.postfix(),
        }
    }
    fn postfix(&mut self) -> Option<f64> {
        let mut v = self.atom()?;
        // "50%" → 0,5 (ardından sayı gelmiyorsa yüzde demektir)
        while self.peek() == Some(b'%') && !self.s.get(self.i + 1).is_some_and(|c| *c == b' ' || c.is_ascii_digit() || *c == b'(') {
            self.i += 1;
            v /= 100.0;
        }
        if self.peek() == Some(b'!') {
            self.i += 1;
            if v < 0.0 || v > 170.0 || v.fract() != 0.0 {
                return None;
            }
            v = (1..=v as u64).map(|x| x as f64).product();
        }
        Some(v)
    }
    fn atom(&mut self) -> Option<f64> {
        match self.peek()? {
            b'(' => {
                self.i += 1;
                let v = self.expr()?;
                if self.peek() == Some(b')') {
                    self.i += 1;
                }
                Some(v)
            }
            // "%20" → 0,2
            b'%' => {
                self.i += 1;
                Some(self.atom()? / 100.0)
            }
            c if c.is_ascii_digit() || c == b'.' => {
                let st = self.i;
                while self.i < self.s.len() && (self.s[self.i].is_ascii_digit() || self.s[self.i] == b'.') {
                    self.i += 1;
                }
                std::str::from_utf8(&self.s[st..self.i]).ok()?.parse().ok()
            }
            c if c.is_ascii_alphabetic() => {
                let st = self.i;
                while self.i < self.s.len() && self.s[self.i].is_ascii_alphanumeric() {
                    self.i += 1;
                }
                let name = std::str::from_utf8(&self.s[st..self.i]).ok()?.to_ascii_lowercase();
                match name.as_str() {
                    "pi" => Some(std::f64::consts::PI),
                    "e" => Some(std::f64::consts::E),
                    _ => {
                        let a = self.atom()?;
                        Some(match name.as_str() {
                            "sqrt" | "kok" => a.sqrt(),
                            "sin" => a.to_radians().sin(),
                            "cos" => a.to_radians().cos(),
                            "tan" => a.to_radians().tan(),
                            "log" => a.log10(),
                            "ln" => a.ln(),
                            "abs" => a.abs(),
                            "round" => a.round(),
                            "floor" => a.floor(),
                            "ceil" => a.ceil(),
                            _ => return None,
                        })
                    }
                }
            }
            _ => None,
        }
    }
}

/// Sorgu bir hesap ifadesiyse sonucunu döndürür. Tek başına bir sayı hesap sayılmaz.
pub fn eval(q: &str) -> Option<f64> {
    let q = q.trim().trim_end_matches('=').trim();
    if q.is_empty() || !q.chars().any(|c| c.is_ascii_digit()) && !q.to_lowercase().contains("pi") {
        return None;
    }
    // En az bir işlem ya da işlev olmalı.
    let has_op = q.chars().skip(1).any(|c| "+-*/^%!x×÷(".contains(c)) || q.starts_with('%') || q.starts_with('(');
    let has_fn = ["sqrt", "kok", "sin", "cos", "tan", "log", "ln", "abs", "round", "pi"].iter().any(|f| q.to_lowercase().contains(f));
    if !has_op && !has_fn {
        return None;
    }
    // "5 x 3" çarpmadır; ondalık virgül noktaya çevrilir (binlik nokta yazımı desteklenmez).
    let norm: String = q
        .chars()
        .map(|c| match c {
            '×' | 'x' | 'X' => '*',
            '÷' | ':' => '/',
            ',' => '.',
            c => c,
        })
        .collect();
    // "x" bir işlev adının parçası değilse çarpmadır; "exp" gibi adlar desteklenmediği için güvenli.
    if !norm.is_ascii() {
        return None;
    }
    let mut p = P { s: norm.as_bytes(), i: 0 };
    let v = p.expr()?;
    p.ws();
    (p.i == p.s.len() && v.is_finite()).then_some(v)
}

// ---- Birimler ----

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cat {
    Length,
    Mass,
    Volume,
    Data,
    Time,
    Temp,
    Area,
    Speed,
}

/// (adlar, kategori, temel birime çarpan, gösterim adı)
const UNITS: &[(&[&str], Cat, f64, &str)] = &[
    (&["mm", "milimetre", "millimeter", "millimeters"], Cat::Length, 0.001, "mm"),
    (&["cm", "santimetre", "santim", "centimeter", "centimeters"], Cat::Length, 0.01, "cm"),
    (&["m", "metre", "meter", "meters", "metres"], Cat::Length, 1.0, "m"),
    (&["km", "kilometre", "kilometer", "kilometers", "kilometres"], Cat::Length, 1000.0, "km"),
    (&["in", "inç", "inc", "inch", "inches", "\""], Cat::Length, 0.0254, "inç"),
    (&["ft", "feet", "foot", "fit"], Cat::Length, 0.3048, "ft"),
    (&["yd", "yard", "yards"], Cat::Length, 0.9144, "yd"),
    (&["mi", "mil", "mile", "miles"], Cat::Length, 1609.344, "mil"),
    (&["nmi", "denizmili"], Cat::Length, 1852.0, "deniz mili"),
    (&["mg", "miligram", "milligram", "milligrams"], Cat::Mass, 0.001, "mg"),
    (&["g", "gr", "gram", "grams"], Cat::Mass, 1.0, "g"),
    (&["kg", "kilo", "kilogram", "kilograms"], Cat::Mass, 1000.0, "kg"),
    (&["t", "ton", "tons"], Cat::Mass, 1_000_000.0, "ton"),
    (&["oz", "ons", "ounce", "ounces"], Cat::Mass, 28.349523125, "oz"),
    (&["lb", "lbs", "pound", "pounds", "libre"], Cat::Mass, 453.59237, "lb"),
    (&["ml", "mililitre", "milliliter", "milliliters"], Cat::Volume, 0.001, "ml"),
    (&["cl", "santilitre"], Cat::Volume, 0.01, "cl"),
    (&["l", "lt", "litre", "liter", "liters", "litres"], Cat::Volume, 1.0, "L"),
    (&["m3", "metreküp"], Cat::Volume, 1000.0, "m³"),
    (&["gal", "galon", "gallon", "gallons"], Cat::Volume, 3.785411784, "galon"),
    (&["cup", "bardak"], Cat::Volume, 0.2365882365, "bardak"),
    (&["b", "byte", "bytes", "bayt"], Cat::Data, 1.0, "B"),
    (&["kb", "kilobyte", "kilobayt"], Cat::Data, 1024.0, "KB"),
    (&["mb", "megabyte", "megabayt"], Cat::Data, 1048576.0, "MB"),
    (&["gb", "gigabyte", "gigabayt"], Cat::Data, 1073741824.0, "GB"),
    (&["tb", "terabyte", "terabayt"], Cat::Data, 1099511627776.0, "TB"),
    (&["ms", "milisaniye"], Cat::Time, 0.001, "ms"),
    (&["s", "sn", "saniye", "sec", "second", "seconds"], Cat::Time, 1.0, "sn"),
    (&["dk", "min", "dakika", "minute", "minutes"], Cat::Time, 60.0, "dk"),
    (&["h", "sa", "saat", "hour", "hours"], Cat::Time, 3600.0, "saat"),
    (&["gün", "gun", "day", "days"], Cat::Time, 86400.0, "gün"),
    (&["hafta", "week", "weeks"], Cat::Time, 604800.0, "hafta"),
    (&["yıl", "yil", "year", "years"], Cat::Time, 31557600.0, "yıl"),
    (&["c", "°c", "celsius", "santigrat"], Cat::Temp, 0.0, "°C"),
    (&["f", "°f", "fahrenheit"], Cat::Temp, 1.0, "°F"),
    (&["k", "kelvin"], Cat::Temp, 2.0, "K"),
    (&["m2", "metrekare"], Cat::Area, 1.0, "m²"),
    (&["km2", "kilometrekare"], Cat::Area, 1e6, "km²"),
    (&["dönüm", "donum"], Cat::Area, 1000.0, "dönüm"),
    (&["ha", "hektar", "hectare"], Cat::Area, 10000.0, "hektar"),
    (&["acre", "akre"], Cat::Area, 4046.8564224, "akre"),
    (&["ft2", "sqft"], Cat::Area, 0.09290304, "ft²"),
    (&["kmh", "km/h", "km/sa"], Cat::Speed, 1.0 / 3.6, "km/sa"),
    (&["mph"], Cat::Speed, 0.44704, "mph"),
    (&["m/s", "mps"], Cat::Speed, 1.0, "m/s"),
    (&["knot", "knots", "knot", "kt", "deniz"], Cat::Speed, 0.514444, "knot"),
];

fn unit(name: &str) -> Option<(Cat, f64, &'static str)> {
    let n = name.trim().to_lowercase();
    UNITS.iter().find(|u| u.0.contains(&n.as_str())).map(|u| (u.1, u.2, u.3))
}

fn temp(v: f64, from: f64, to: f64) -> f64 {
    let c = match from as i32 {
        0 => v,
        1 => (v - 32.0) * 5.0 / 9.0,
        _ => v - 273.15,
    };
    match to as i32 {
        0 => c,
        1 => c * 9.0 / 5.0 + 32.0,
        _ => c + 273.15,
    }
}

/// "sayı birim [to|in|->|=|kaç] birim" biçimini ayırır: (sayı, birim1, birim2).
pub fn split_conversion(q: &str) -> Option<(f64, String, String)> {
    let q = q.trim().to_lowercase();
    // Sayı ile birim bitişik olabilir: "10km mil"
    let num_end = q.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',' || c == '-' || c == ' ')).unwrap_or(q.len());
    if num_end == 0 {
        return None;
    }
    let v = parse_num(&q[..num_end])?;
    let rest = q[num_end..].trim();
    let words: Vec<&str> = rest
        .split(|c: char| c == ' ')
        .filter(|w| !w.is_empty() && !["to", "in", "->", "=", "kaç", "kac", "ne", "eder", "?", "into"].contains(w))
        .collect();
    if words.len() != 2 {
        return None;
    }
    Some((v, words[0].to_string(), words[1].to_string()))
}

/// Birim çevirisi: (sonuç, açıklama) — ör. (6,21, "10 km = 6,21 mil").
pub fn convert_units(q: &str) -> Option<(f64, String)> {
    let (v, a, b) = split_conversion(q)?;
    let (ca, fa, na) = unit(&a)?;
    let (cb, fb, nb) = unit(&b)?;
    if ca != cb {
        return None;
    }
    let r = if ca == Cat::Temp { temp(v, fa, fb) } else { v * fa / fb };
    let r = (r * 1e6).round() / 1e6;
    Some((r, format!("{} {na} = {} {nb}", fmt(v), fmt(r))))
}

// ---- Döviz ----

const CURRENCIES: &[(&[&str], &str)] = &[
    (&["try", "tl", "lira", "₺"], "TRY"),
    (&["usd", "dolar", "dollar", "$"], "USD"),
    (&["eur", "euro", "avro", "€"], "EUR"),
    (&["gbp", "sterlin", "pound", "£"], "GBP"),
    (&["jpy", "yen"], "JPY"),
    (&["chf", "frank"], "CHF"),
    (&["cad"], "CAD"),
    (&["aud"], "AUD"),
    (&["cny", "yuan"], "CNY"),
    (&["rub", "ruble"], "RUB"),
    (&["sek"], "SEK"),
    (&["nok"], "NOK"),
    (&["dkk"], "DKK"),
    (&["pln", "zloti"], "PLN"),
    (&["inr", "rupi"], "INR"),
    (&["krw", "won"], "KRW"),
    (&["mxn"], "MXN"),
    (&["brl"], "BRL"),
    (&["huf"], "HUF"),
    (&["czk"], "CZK"),
    (&["ron"], "RON"),
    (&["bgn"], "BGN"),
    (&["aed"], "AED"),
    (&["sar"], "SAR"),
    (&["azn", "manat"], "AZN"),
];

fn currency(name: &str) -> Option<&'static str> {
    let n = name.trim().to_lowercase();
    CURRENCIES.iter().find(|c| c.0.contains(&n.as_str())).map(|c| c.1)
}

/// Döviz isteği: (miktar, kaynak, hedef). "100 dolar" tek başına TL'ye çevrilir.
pub fn currency_request(q: &str) -> Option<(f64, &'static str, &'static str)> {
    if let Some((v, a, b)) = split_conversion(q) {
        let (x, y) = (currency(&a)?, currency(&b)?);
        return (x != y).then_some((v, x, y));
    }
    // "100 usd"
    let q = q.trim().to_lowercase();
    let (num, cur) = q.rsplit_once(' ')?;
    let v = parse_num(num)?;
    let c = currency(cur)?;
    (c != "TRY").then_some((v, c, "TRY"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic() {
        assert_eq!(eval("2+2*3"), Some(8.0));
        assert_eq!(eval("(1,5 x 2)^2"), Some(9.0));
        assert_eq!(eval("%20 * 150"), Some(30.0));
        assert_eq!(eval("150*20%"), Some(30.0));
        assert_eq!(eval("10 % 3"), Some(1.0));
        assert_eq!(eval("sqrt(16)"), Some(4.0));
        assert_eq!(eval("5!"), Some(120.0));
        assert_eq!(eval("2pi").map(|v| (v * 100.0).round()), Some(628.0));
        assert_eq!(eval("42"), None);
        assert_eq!(eval("chrome"), None);
        assert_eq!(eval("2+"), None);
    }

    #[test]
    fn units() {
        assert_eq!(convert_units("10 km mil").map(|r| (r.0 * 100.0).round()), Some(621.0));
        assert_eq!(convert_units("100 f to c").map(|r| (r.0 * 10.0).round()), Some(378.0));
        assert_eq!(convert_units("1 gb mb").map(|r| r.0), Some(1024.0));
        assert_eq!(convert_units("90 dk saat").map(|r| r.0), Some(1.5));
        assert!(convert_units("10 km kg").is_none());
    }

    #[test]
    fn currencies() {
        assert_eq!(currency_request("100 usd try"), Some((100.0, "USD", "TRY")));
        assert_eq!(currency_request("50 euro tl"), Some((50.0, "EUR", "TRY")));
        assert_eq!(currency_request("100 dolar"), Some((100.0, "USD", "TRY")));
        assert_eq!(currency_request("100 km mil"), None);
    }

    #[test]
    fn formatting() {
        assert_eq!(fmt(1234567.5), "1.234.567,5");
        assert_eq!(fmt(-0.25), "-0,25");
        assert_eq!(fmt(3.0), "3");
    }
}
