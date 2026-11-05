//! Bu modül "hücredeki 45231 sayısı tarih biçimliyse 01.11.2023, para
//! biçimliyse 1.234,50 ₺ görünsün" sorununu çözüyor: Excel sayı biçim
//! kodlarının (yerleşik kimlikler + özel kodlar) yaygın kısmını uygular.

/// Yerleşik biçim kimliği → kod.
pub fn builtin(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "General",
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        14 => "dd.mm.yyyy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "dd.mm.yyyy h:mm",
        37 => "#,##0 ;(#,##0)",
        38 => "#,##0 ;[Red](#,##0)",
        39 => "#,##0.00;(#,##0.00)",
        40 => "#,##0.00;[Red](#,##0.00)",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mmss.0",
        48 => "##0.0E+0",
        49 => "@",
        _ => return None,
    })
}

/// Excel seri günü → (yıl, ay, gün). 1900 artık yıl hatası dahil.
pub fn ymd(serial: f64, date1904: bool) -> (i64, u32, u32) {
    let mut days = serial.floor() as i64;
    if date1904 {
        days += 1462;
    }
    // 1899-12-30 tabanı (60'tan küçük seriler için 1 gün kaydır).
    if !date1904 && days < 61 {
        days += 1;
    }
    let z = days - 25_569 + 719_468; // 1970-01-01'e göre
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

/// (yıl, ay, gün) → Excel seri günü.
pub fn serial(y: i64, m: u32, d: u32) -> f64 {
    let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * m as i64 + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468 + 25_569;
    days as f64
}

const MONTHS_TR: [&str; 12] = ["Ocak", "Şubat", "Mart", "Nisan", "Mayıs", "Haziran", "Temmuz", "Ağustos", "Eylül", "Ekim", "Kasım", "Aralık"];
const MONTHS_EN: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// Kodu bölümlerine ayırır (`;`), tırnak ve köşeli parantez içini korur.
fn sections(code: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let (mut q, mut br) = (false, false);
    let mut esc = false;
    for c in code.chars() {
        if esc {
            out.last_mut().unwrap().push(c);
            esc = false;
            continue;
        }
        match c {
            '\\' if !q => {
                esc = true;
                out.last_mut().unwrap().push(c);
                continue;
            }
            '"' => q = !q,
            '[' if !q => br = true,
            ']' if !q => br = false,
            ';' if !q && !br => {
                out.push(String::new());
                continue;
            }
            _ => {}
        }
        out.last_mut().unwrap().push(c);
    }
    out
}

/// Tırnak/köşeli parantez dışındaki biçim harfleri.
fn bare(code: &str) -> String {
    let mut s = String::new();
    let (mut q, mut br, mut esc) = (false, false, false);
    for c in code.chars() {
        if esc {
            esc = false;
            continue;
        }
        match c {
            '\\' if !q => esc = true,
            '"' => q = !q,
            '[' if !q => br = true,
            ']' if !q => br = false,
            _ if !q && !br => s.push(c),
            _ => {}
        }
    }
    s
}

pub fn is_date(code: &str) -> bool {
    let b = bare(code).to_lowercase();
    (b.contains('d') || b.contains('y') || b.contains('h') || (b.contains('m') && b.contains('s'))) && !b.contains('0') && !b.contains('#') || b.contains("yy") || b.contains("dd")
}

fn date_fmt(v: f64, code: &str, date1904: bool) -> String {
    let (y, mo, d) = ymd(v, date1904);
    let secs = ((v - v.floor()) * 86_400.0).round() as i64;
    let (hh, mi, ss) = (secs / 3600 % 24, secs / 60 % 60, secs % 60);
    let ampm = code.to_uppercase().contains("AM/PM");
    let tr = carlui_core::lang::is_tr();
    let c: Vec<char> = code.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut last_h = false;
    while i < c.len() {
        let ch = c[i];
        let lower = ch.to_ascii_lowercase();
        let run = c[i..].iter().take_while(|x| x.to_ascii_lowercase() == lower).count();
        match lower {
            '"' => {
                i += 1;
                while i < c.len() && c[i] != '"' {
                    out.push(c[i]);
                    i += 1;
                }
                i += 1;
                continue;
            }
            '\\' => {
                if let Some(n) = c.get(i + 1) {
                    out.push(*n);
                }
                i += 2;
                continue;
            }
            '[' => {
                // [h] geçen süre, [Red] renk: içini atla ama [h] saatini yaz.
                let end = c[i..].iter().position(|x| *x == ']').map(|p| i + p).unwrap_or(c.len() - 1);
                let inner: String = c[i + 1..end].iter().collect::<String>().to_lowercase();
                if inner.starts_with('h') {
                    out.push_str(&format!("{}", (v * 24.0).floor() as i64));
                    last_h = true;
                }
                i = end + 1;
                continue;
            }
            'y' => out.push_str(&if run <= 2 { format!("{:02}", y % 100) } else { format!("{y}") }),
            'd' => match run {
                1 => out.push_str(&d.to_string()),
                2 => out.push_str(&format!("{d:02}")),
                _ => {
                    let wd = ((v.floor() as i64 + 6) % 7) as usize;
                    let names = if tr { ["Pazar", "Pazartesi", "Salı", "Çarşamba", "Perşembe", "Cuma", "Cumartesi"] } else { ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"] };
                    let n = names[wd];
                    out.push_str(if run == 3 { &n[..n.char_indices().nth(3).map(|x| x.0).unwrap_or(n.len())] } else { n });
                }
            },
            'm' => {
                // h'den sonra ya da s'den önce gelen m dakikadır.
                let next_s = c[i + run..].iter().find(|x| x.is_ascii_alphabetic()).is_some_and(|x| x.eq_ignore_ascii_case(&'s'));
                if last_h || next_s {
                    out.push_str(&if run == 1 { mi.to_string() } else { format!("{mi:02}") });
                } else {
                    let names = if tr { MONTHS_TR } else { MONTHS_EN };
                    let n = names[(mo as usize).clamp(1, 12) - 1];
                    match run {
                        1 => out.push_str(&mo.to_string()),
                        2 => out.push_str(&format!("{mo:02}")),
                        3 => out.push_str(&n.chars().take(3).collect::<String>()),
                        _ => out.push_str(n),
                    }
                }
            }
            'h' => {
                let h = if ampm { (hh + 11) % 12 + 1 } else { hh };
                out.push_str(&if run == 1 { h.to_string() } else { format!("{h:02}") });
                last_h = true;
                i += run;
                continue;
            }
            's' => out.push_str(&if run == 1 { ss.to_string() } else { format!("{ss:02}") }),
            'a' if code[code.char_indices().nth(i).map(|x| x.0).unwrap_or(0)..].to_uppercase().starts_with("AM/PM") => {
                out.push_str(if hh < 12 { "AM" } else { "PM" });
                i += 5;
                continue;
            }
            _ => {
                out.push(ch);
                i += 1;
                continue;
            }
        }
        if lower != 'h' && lower.is_ascii_alphabetic() && lower != 'm' {
            last_h = false;
        }
        if lower == 'm' {
            last_h = false;
        }
        i += run;
    }
    out
}

fn group(int: &str, sep: char) -> String {
    let mut out = String::new();
    let n = int.len();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (n - i).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(ch);
    }
    out
}

fn number_fmt(v: f64, code: &str) -> String {
    let tr = carlui_core::lang::is_tr();
    let (dec_sep, grp_sep) = if tr { (',', '.') } else { ('.', ',') };
    let b = bare(code);
    let pct = b.contains('%');
    let v = if pct { v * 100.0 } else { v };
    if b.to_uppercase().contains("E+") {
        let decimals = b.split('.').nth(1).map(|s| s.chars().take_while(|c| *c == '0').count()).unwrap_or(0);
        return format!("{:.*E}", decimals, v).replace('.', &dec_sep.to_string());
    }
    let decimals = b.split('.').nth(1).map(|s| s.chars().take_while(|c| matches!(c, '0' | '#' | '?')).count()).unwrap_or(0);
    let grouping = b.split('.').next().unwrap_or("").contains(',');
    let s = format!("{:.*}", decimals, v.abs());
    let (int, frac) = s.split_once('.').unwrap_or((&s, ""));
    let mut num = if grouping { group(int, grp_sep) } else { int.to_string() };
    if !frac.is_empty() {
        num.push(dec_sep);
        num.push_str(frac);
    }
    // Biçimdeki sabit metinleri (₺, TL, $) ve sayı yerini koru.
    let mut out = String::new();
    let mut placed = false;
    let c: Vec<char> = code.chars().collect();
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            '"' => {
                i += 1;
                while i < c.len() && c[i] != '"' {
                    out.push(c[i]);
                    i += 1;
                }
            }
            '\\' => {
                if let Some(n) = c.get(i + 1) {
                    out.push(*n);
                }
                i += 1;
            }
            '[' => {
                // [$₺-41F] para birimi
                let end = c[i..].iter().position(|x| *x == ']').map(|p| i + p).unwrap_or(c.len() - 1);
                let inner: String = c[i + 1..end].iter().collect();
                if let Some(cur) = inner.strip_prefix('$') {
                    out.push_str(cur.split('-').next().unwrap_or(""));
                }
                i = end;
            }
            '0' | '#' | '?' | '.' | ',' => {
                if !placed {
                    out.push_str(&num);
                    placed = true;
                }
            }
            '_' | '*' => i += 1,
            '%' => out.push('%'),
            '@' => {}
            ch => out.push(ch),
        }
        i += 1;
    }
    if !placed {
        out.push_str(&num);
    }
    let out = out.trim().to_string();
    if v < 0.0 && s.chars().any(|c| c.is_ascii_digit() && c != '0') {
        format!("-{out}")
    } else {
        out
    }
}

/// Sayıyı biçim koduna göre yazıya çevirir.
pub fn format(v: f64, code: &str, date1904: bool) -> String {
    if code.is_empty() || code.eq_ignore_ascii_case("general") || code == "@" {
        return super::general(v);
    }
    let secs = sections(code);
    let sec = if v < 0.0 && secs.len() > 1 {
        // İkinci bölüm eksi sayılar için; işaret bölümde yazılıdır.
        return number_or_date(-v, &secs[1], date1904);
    } else if v == 0.0 && secs.len() > 2 {
        &secs[2]
    } else {
        &secs[0]
    };
    number_or_date(v, sec, date1904)
}

fn number_or_date(v: f64, sec: &str, date1904: bool) -> String {
    if sec.eq_ignore_ascii_case("general") {
        return super::general(v);
    }
    if is_date(sec) {
        date_fmt(v, sec, date1904)
    } else {
        number_fmt(v, sec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(ymd(45231.0, false), (2023, 11, 1));
        assert_eq!(ymd(1.0, false), (1900, 1, 1));
        assert_eq!(serial(2023, 11, 1), 45231.0);
        assert!(is_date("dd.mm.yyyy"));
        assert!(is_date("[$-41F]d mmmm yyyy;@"));
        assert!(!is_date("#,##0.00"));
        assert_eq!(date_fmt(45231.5, "dd.mm.yyyy hh:mm", false), "01.11.2023 12:00");
    }

    #[test]
    fn numbers() {
        let _lang = crate::test_lang("en");
        assert_eq!(format(1234.5, "#,##0.00", false), "1,234.50");
        assert_eq!(format(0.256, "0.0%", false), "25.6%");
        assert_eq!(format(-5.0, "0;(0)", false), "(5)");
        assert_eq!(format(1500.0, "#,##0 \"₺\"", false), "1,500 ₺");
        assert_eq!(format(12.0, "[$$-409]#,##0.00", false), "$12.00");
    }
}
