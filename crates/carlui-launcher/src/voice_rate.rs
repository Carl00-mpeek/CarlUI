//! Bu modül "sesli asistana 'dolar kaç TL' deyince kuru nasıl söyletiriz?"
//! sorununu çözüyor. Cümlede bir döviz adı ve soru sözcüğü ("kaç", "ne kadar",
//! "kur", "how much") varsa istek kur sorusudur: kaynak ilk söylenen para,
//! hedef ikincisi (söylenmediyse TL). Önündeki sayı miktardır ("100 euro"),
//! yoksa 1. Kurun kendisi `currency` modülünden (Frankfurter) gelir.

use crate::{calc, currency};

/// Soru olduğunu gösteren sözcükler (ASCII'ye indirgenmiş kökler).
const CUES: &[&str] = &["kac", "kadar", "kur", "fiyat", "deger", "oldu", "olmus", "how", "much", "rate", "price", "worth"];

/// Bunlar varsa istek kur değil başka bir iştir ("dolar kuru ara", "Euro Truck'ı aç").
const OTHER: &[&str] = &["ara", "search", "google", "ac", "open", "cal", "play", "kapat", "close"];

/// "dolar", "doları", "dolara", "TL", "lirası" → para birimi kodu.
fn code(w: &str) -> Option<&'static str> {
    calc::currency_code(w).or_else(|| calc::CURRENCIES.iter().find(|(names, _)| names.iter().any(|n| n.len() >= 4 && w.starts_with(n))).map(|(_, c)| *c))
}

/// Sayı sözcüğü: "100", "1,5", "yüz", "elli", "bin".
fn num_word(base: &str, orig: &str) -> Option<f64> {
    if orig.starts_with(|c: char| c.is_ascii_digit()) {
        return calc::parse_num(orig);
    }
    const WORDS: &[(&[&str], f64)] = &[
        (&["iki", "two"], 2.0), (&["uc", "three"], 3.0), (&["dort", "four"], 4.0), (&["bes", "five"], 5.0), (&["alti", "six"], 6.0),
        (&["yedi", "seven"], 7.0), (&["sekiz", "eight"], 8.0), (&["dokuz", "nine"], 9.0), (&["on", "ten"], 10.0), (&["yirmi", "twenty"], 20.0),
        (&["otuz", "thirty"], 30.0), (&["kirk", "forty"], 40.0), (&["elli", "fifty"], 50.0), (&["altmis", "sixty"], 60.0), (&["yetmis", "seventy"], 70.0),
        (&["seksen", "eighty"], 80.0), (&["doksan", "ninety"], 90.0), (&["yuz", "hundred"], 100.0), (&["bin", "thousand"], 1000.0), (&["one"], 1.0),
    ];
    WORDS.iter().find(|(ws, _)| ws.contains(&base)).map(|(_, n)| *n)
}

/// Para adının hemen önündeki sayı sözcüklerinden miktar: "iki yüz elli" → 250.
fn amount(base: &[&str], orig: &[&str], end: usize) -> f64 {
    let start = (0..end).rev().take_while(|&i| num_word(base[i], orig[i]).is_some()).last().unwrap_or(end);
    let (mut total, mut cur) = (0.0, 0.0);
    for i in start..end {
        let v = num_word(base[i], orig[i]).unwrap_or(0.0);
        if v == 100.0 {
            cur = if cur == 0.0 { 100.0 } else { cur * 100.0 };
        } else if v == 1000.0 {
            total += if cur == 0.0 { 1000.0 } else { cur * 1000.0 };
            cur = 0.0;
        } else {
            cur += v;
        }
    }
    let n = total + cur;
    if n > 0.0 { n } else { 1.0 }
}

/// Kur sorusu mu? (miktar, kaynak, hedef). `base` ASCII'ye indirgenmiş kökler,
/// `orig` söylendiği gibi sözcükler (ikisi aynı uzunlukta).
pub(crate) fn parse(base: &[&str], orig: &[&str]) -> Option<(f64, &'static str, &'static str)> {
    let starts = |ws: &[&str]| base.iter().any(|w| ws.iter().any(|x| w.starts_with(x)));
    if base.iter().any(|w| OTHER.contains(w)) || starts(&["ara", "search"]) {
        return None;
    }
    let found: Vec<(usize, &'static str)> = base.iter().enumerate().filter_map(|(i, w)| code(w).map(|c| (i, c))).collect();
    let (i, from) = *found.first()?;
    let to = found.iter().map(|(_, c)| *c).find(|c| *c != from);
    let asks = starts(CUES) || to.is_some();
    match (from, to) {
        (_, _) if !asks => None,
        ("TRY", None) => None,
        (from, None) => Some((amount(base, orig, i), from, "TRY")),
        (from, Some(to)) => Some((amount(base, orig, i), from, to)),
    }
}

/// Ekranda ve seste kullanılacak ad.
fn name(code: &str) -> &str {
    if !carlui_core::lang::is_tr() {
        return code;
    }
    match code {
        "TRY" => "TL",
        "USD" => "dolar",
        "EUR" => "euro",
        "GBP" => "sterlin",
        "JPY" => "yen",
        "CHF" => "frank",
        "RUB" => "ruble",
        "AZN" => "manat",
        c => c,
    }
}

/// Küçük tutarlarda dört, ötekilerde iki ondalık.
fn round(v: f64) -> f64 {
    let k = if v.abs() < 1.0 { 10_000.0 } else { 100.0 };
    (v * k).round() / k
}

/// Para yazımı: "50,1" değil "50,10".
fn money(v: f64) -> String {
    let s = calc::fmt(v);
    let mark = if carlui_core::lang::is_tr() { ',' } else { '.' };
    match s.rsplit_once(mark) {
        Some((_, frac)) if frac.len() == 1 => format!("{s}0"),
        _ => s,
    }
}

/// Kur yanıtı: (ekran yazısı, okunacak cümle).
pub fn text(amount: f64, from: &str, to: &str, rate: f64, date: &str) -> (String, String) {
    let v = round(amount * rate);
    let shown = format!("{} {} = {} {} · {date}", calc::fmt(amount), name(from), money(v), name(to));
    let spoken = if carlui_core::lang::is_tr() && to == "TRY" && v >= 1.0 {
        let lira = v.trunc();
        let kurus = ((v - lira) * 100.0).round();
        let kurus = if kurus > 0.0 { format!(" {kurus} kuruş") } else { String::new() };
        format!("{} {}, {lira} lira{kurus}", calc::fmt(amount), name(from))
    } else if carlui_core::lang::is_tr() {
        format!("{} {}, {} {}", calc::fmt(amount), name(from), calc::fmt(v), if to == "TRY" { "lira" } else { name(to) })
    } else {
        format!("{} {from} is {} {to}", calc::fmt(amount), calc::fmt(v))
    };
    (shown, spoken)
}

/// Kuru alır (bekler). Bağlantı yoksa `None`.
pub fn answer(amount: f64, from: &str, to: &str) -> Option<(String, String)> {
    let (rate, date) = currency::fetch(from, to)?;
    Some(text(amount, from, to, rate, &date))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(s: &str) -> Option<(f64, &'static str, &'static str)> {
        match crate::voice::parse(s) {
            crate::voice::Intent::Rate { amount, from, to } => Some((amount, from, to)),
            _ => None,
        }
    }

    #[test]
    fn hears_rate_questions() {
        assert_eq!(ask("Dolar kaç TL?"), Some((1.0, "USD", "TRY")));
        assert_eq!(ask("dolar kaç"), Some((1.0, "USD", "TRY")));
        assert_eq!(ask("1 dolar kaç lira"), Some((1.0, "USD", "TRY")));
        assert_eq!(ask("bir dolar kaç Türk lirası"), Some((1.0, "USD", "TRY")));
        assert_eq!(ask("100 euro kaç TL"), Some((100.0, "EUR", "TRY")));
        assert_eq!(ask("iki yüz elli euro ne kadar"), Some((250.0, "EUR", "TRY")));
        assert_eq!(ask("euro kaç dolar"), Some((1.0, "EUR", "USD")));
        assert_eq!(ask("sterlin kuru ne"), Some((1.0, "GBP", "TRY")));
        assert_eq!(ask("doların kuru"), Some((1.0, "USD", "TRY")));
        assert_eq!(ask("How much is a dollar in lira?"), Some((1.0, "USD", "TRY")));
    }

    #[test]
    fn leaves_other_commands() {
        assert_eq!(ask("YouTube'da dolar kuru ara"), None);
        assert_eq!(ask("Euro Truck Simulator'ı aç"), None);
        assert_eq!(ask("TL kaç"), None);
        assert_eq!(ask("Spotify'ı aç"), None);
    }

    /// Ağ ister: `cargo test -p carlui-launcher --lib live_rate -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_rate() {
        let a = answer(1.0, "USD", "TRY").expect("kur alınamadı");
        println!("{a:?}");
    }

    #[test]
    fn says_rate_in_lira() {
        let (shown, spoken) = text(1.0, "USD", "TRY", 42.3456, "07.11.2026");
        if carlui_core::lang::is_tr() {
            assert_eq!(shown, "1 dolar = 42,35 TL · 07.11.2026");
            assert_eq!(spoken, "1 dolar, 42 lira 35 kuruş");
        }
        assert!(shown.contains("07.11.2026"));
        if carlui_core::lang::is_tr() {
            assert_eq!(text(100.0, "EUR", "TRY", 58.0501, "x").0, "100 euro = 5.805,01 TL · x");
            assert_eq!(text(1.0, "USD", "TRY", 50.1, "x"), ("1 dolar = 50,10 TL · x".into(), "1 dolar, 50 lira 10 kuruş".into()));
        }
    }
}
