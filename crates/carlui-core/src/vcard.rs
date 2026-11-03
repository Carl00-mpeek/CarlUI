//! Bu modül "WhatsApp kişileri elle yazılmasın, herkesin kendi rehberi gelsin"
//! sorununu şu yöntemle çözüyor: telefondan dışa aktarılan rehber dosyası
//! (.vcf, vCard 2.1/3.0/4.0) okunur; her kişinin adı ve cep numarası alınır,
//! numara WhatsApp'ın istediği ülke kodlu biçime getirilir. Dosya yalnızca
//! yerelde okunur, hiçbir yere gönderilmez.

use std::collections::BTreeMap;

/// Ülke kodu yazılmamış numaralar için varsayılan (0532… → 90532…).
const COUNTRY: &str = "90";

/// Dosya içeriğinden ad → numara listesi. Aynı ada sahip kişiler "(2)" ekiyle ayrılır.
pub fn parse(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut name = String::new();
    let mut alt = String::new();
    let mut phones: Vec<(bool, String)> = Vec::new();
    for line in unfold(text) {
        let Some((head, value)) = line.split_once(':') else { continue };
        let mut parts = head.split(';');
        let key = parts.next().unwrap_or("").rsplit('.').next().unwrap_or("").to_ascii_uppercase();
        let params: Vec<String> = parts.map(|p| p.to_ascii_uppercase()).collect();
        let qp = params.iter().any(|p| p.contains("QUOTED-PRINTABLE"));
        let value = if qp { decode_qp(value) } else { value.to_string() };
        match key.as_str() {
            "BEGIN" => {
                name.clear();
                alt.clear();
                phones.clear();
            }
            "FN" => name = unescape(&value),
            "N" => {
                // Soyad;Ad;İkinci ad;Önek;Sonek → "Ad Soyad"
                let f: Vec<&str> = value.split(';').collect();
                alt = [f.get(1), f.get(2), f.first()].iter().flatten().map(|s| unescape(s)).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
            }
            "TEL" => {
                let cell = params.iter().any(|p| p.contains("CELL") || p.contains("MOBILE"));
                let num = phone(value.trim_start_matches("tel:"));
                if !num.is_empty() {
                    phones.push((cell, num));
                }
            }
            "END" => {
                let n = if name.trim().is_empty() { alt.trim() } else { name.trim() };
                let best = phones.iter().find(|(c, _)| *c).or(phones.first());
                if let (false, Some((_, num))) = (n.is_empty(), best) {
                    let mut key = n.to_string();
                    let mut i = 2;
                    while out.contains_key(&key) {
                        key = format!("{n} ({i})");
                        i += 1;
                    }
                    out.insert(key, num.clone());
                }
            }
            _ => {}
        }
    }
    out
}

/// Ülke kodlu, yalnız rakam: "+90 532 …" / "0532 …" / "532 …" → "90532…".
pub fn phone(s: &str) -> String {
    let plus = s.trim_start().starts_with('+');
    let d: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    if d.len() < 7 {
        return String::new();
    }
    if plus {
        d
    } else if let Some(r) = d.strip_prefix("00") {
        r.to_string()
    } else if let Some(r) = d.strip_prefix('0') {
        format!("{COUNTRY}{r}")
    } else if d.len() == 10 {
        format!("{COUNTRY}{d}")
    } else {
        d
    }
}

/// Satır katlamasını açar: boşlukla başlayan satır öncekine eklenir;
/// quoted-printable'da "=" ile biten satır da sonrakiyle birleşir.
fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut soft = false;
    for raw in text.lines() {
        if soft {
            if let Some(last) = out.last_mut() {
                last.pop(); // "="
                last.push_str(raw.trim_start());
                soft = raw.ends_with('=');
                continue;
            }
        }
        if (raw.starts_with(' ') || raw.starts_with('\t')) && !out.is_empty() {
            out.last_mut().unwrap().push_str(&raw[1..]);
        } else {
            out.push(raw.to_string());
        }
        soft = raw.to_ascii_uppercase().contains("QUOTED-PRINTABLE") && raw.ends_with('=');
    }
    out
}

fn decode_qp(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'=' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn unescape(s: &str) -> String {
    s.replace("\\,", ",").replace("\\;", ";").replace("\\n", " ").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_and_iphone() {
        let v = "BEGIN:VCARD\r\nVERSION:2.1\r\nN;CHARSET=UTF-8;ENCODING=QUOTED-PRINTABLE:Y=C4=B1lmaz;Demhat;;;\r\nFN;CHARSET=UTF-8;ENCODING=QUOTED-PRINTABLE:Demhat Y=C4=B1lmaz\r\nTEL;HOME:0212 111 22 33\r\nTEL;CELL:0555 123 45 67\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nVERSION:3.0\r\nN:Kaya;Ali;;;\r\nFN:Ali Kaya\r\nitem1.TEL;type=CELL;type=VOICE;type=pref:+49 151 2345678\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nVERSION:3.0\r\nN:;Annem;;;\r\nTEL;TYPE=CELL:5321234567\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Numarasız\r\nEND:VCARD\r\n";
        let m = parse(v);
        assert_eq!(m.get("Demhat Yılmaz").map(String::as_str), Some("905551234567"));
        assert_eq!(m.get("Ali Kaya").map(String::as_str), Some("491512345678"));
        assert_eq!(m.get("Annem").map(String::as_str), Some("905321234567"));
        assert_eq!(m.len(), 3);
    }

    #[test]
    fn folded_and_duplicates() {
        let v = "BEGIN:VCARD\nFN;ENCODING=QUOTED-PRINTABLE:=C3=96=\nzge\nTEL:+90 532 000 00 01\nEND:VCARD\nBEGIN:VCARD\nFN:Özge\nTEL:+90 532 000 00 02\nEND:VCARD\n";
        let m = parse(v);
        assert_eq!(m.get("Özge").map(String::as_str), Some("905320000001"));
        assert_eq!(m.get("Özge (2)").map(String::as_str), Some("905320000002"));
    }
}
