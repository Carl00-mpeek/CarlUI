//! Banka kartı alanları. Sorun: kart numarası, son kullanma tarihi, CVV ve PIN
//! serbest metin olunca harf yazılabiliyor, numara tek parça okunmuyor ve
//! siteler bilgileri farklı biçimde istiyor (ay ve yıl ayrı kutuda, "AA/YYYY",
//! numara boşluksuz …). Çözüm:
//! - Biçimli alanlarda (`Format`) yalnızca rakam yazılır; numara 4'erli
//!   gruplanır (Amex 4-6-5), son kullanmada "/" kendiliğinden konur ("4" →
//!   "04/"), ay 01–12 dışına çıkamaz.
//! - Saklanan değer sadedir: numara yalnızca rakam, son kullanma "AA/YY".
//! - Hızlı kopyalama (`Part`) aynı kartı sitenin istediği parçalara böler:
//!   numara (boşluksuz ya da boşluklu), ad, ay, yıl (YY / YYYY), AA/YY, AA/YYYY, CVV.
//! - Kart türü numaradan bulunur, Luhn denetimi yanlış yazılmış numarayı yakalar.

use crate::entry::{Body, Category, Field, FieldKind, Format};
use zeroize::Zeroizing;

pub fn digits(s: &str) -> String {
    s.chars().filter(char::is_ascii_digit).collect()
}

/// Bu biçimde en çok kaç rakam yazılır.
pub fn max_digits(fmt: Format, raw: &str) -> usize {
    match fmt {
        Format::CardNumber if is_amex(raw) => 15,
        Format::CardNumber => 19,
        Format::Expiry => 4,
        Format::Cvv => 4,
        Format::Pin => 6,
        Format::Free | Format::Holder => usize::MAX,
    }
}

/// Yazılan karakter kabul edilirse kutuya eklenecek metin. `raw` kutudaki
/// rakamlar (biçimsiz). Son kullanmada ilk rakam 2–9 ise başına 0 konur.
pub fn push(fmt: Format, raw: &str, c: char) -> Option<String> {
    if !fmt.numeric() {
        return Some(c.to_string());
    }
    if !c.is_ascii_digit() || raw.len() >= max_digits(fmt, raw) {
        return None;
    }
    if fmt == Format::Expiry {
        match (raw.len(), raw.as_bytes().first()) {
            (0, _) if c >= '2' => return Some(format!("0{c}")),
            (1, Some(b'0')) if c == '0' => return None,
            (1, Some(b'1')) if c > '2' => return None,
            _ => {}
        }
    }
    Some(c.to_string())
}

fn is_amex(d: &str) -> bool {
    d.starts_with("34") || d.starts_with("37")
}

/// Rakamları gruplar: "4543123412341234" → "4543 1234 1234 1234". Maskeli
/// hâlde "•" de gelebilir; bu yüzden bayt değil karakter sayılır.
pub fn group(d: &str) -> String {
    group_as(d, d)
}

/// `shown` metnini `raw` numarasının grup düzeniyle böler (Amex maskeliyken de 4-6-5).
fn group_as(raw: &str, d: &str) -> String {
    let sizes: &[usize] = if is_amex(raw) { &[4, 6, 5] } else { &[4, 4, 4, 4, 3] };
    let chars: Vec<char> = d.chars().collect();
    let (mut out, mut at) = (String::with_capacity(d.len() + 5), 0);
    for &n in sizes.iter().chain(std::iter::repeat(&4)) {
        if at >= chars.len() {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.extend(&chars[at..(at + n).min(chars.len())]);
        at += n;
    }
    out
}

/// Kutuda görünen hâl. Maskelide numaranın yalnızca son 4 hanesi görünür.
pub fn display(fmt: Format, raw: &str, masked: bool) -> String {
    match fmt {
        Format::CardNumber if masked => {
            let shown = raw.len().saturating_sub(4);
            let hidden: String = raw.char_indices().map(|(i, c)| if i < shown { '•' } else { c }).collect();
            group_as(raw, &hidden)
        }
        Format::CardNumber => group(raw),
        Format::Expiry if raw.len() >= 2 => format!("{}/{}", &raw[..2], &raw[2..]),
        _ if masked => "•".repeat(raw.chars().count().min(40)),
        _ => raw.to_string(),
    }
}

/// Saklanacak değer (kutudaki rakamlardan).
pub fn store(fmt: Format, raw: &str) -> String {
    match fmt {
        Format::Expiry => display(fmt, raw, false),
        f if f.numeric() => digits(raw),
        _ => raw.to_string(),
    }
}

/// Saklanan değerden kutuya konacak rakamlar.
pub fn load(fmt: Format, stored: &str) -> String {
    match fmt {
        Format::Expiry => match expiry(stored) {
            Some((m, y)) => format!("{m:02}{:02}", y % 100),
            None => digits(stored).chars().take(4).collect(),
        },
        f if f.numeric() => digits(stored).chars().take(max_digits(f, &digits(stored))).collect(),
        _ => stored.to_string(),
    }
}

/// Son kullanma (ay, dört haneli yıl). "04/27", "4/2027", "0427", "2027-04" kabul edilir.
pub fn expiry(s: &str) -> Option<(u32, u32)> {
    let s = s.trim();
    let parts: Vec<&str> = s.split(['/', '-', '.', ' ']).filter(|p| !p.is_empty()).collect();
    let (m, y) = match parts.as_slice() {
        [a, b] if a.len() == 4 => (b.parse().ok()?, a.parse().ok()?),
        [a, b] => (a.parse().ok()?, b.parse().ok()?),
        [d] if d.len() == 4 && d.chars().all(|c| c.is_ascii_digit()) => (d[..2].parse().ok()?, d[2..].parse().ok()?),
        [d] if d.len() == 6 && d.chars().all(|c| c.is_ascii_digit()) => (d[..2].parse().ok()?, d[2..].parse().ok()?),
        _ => return None,
    };
    let y: u32 = if y < 100 { 2000 + y } else { y };
    ((1..=12).contains(&m) && (2000..=2099).contains(&y)).then_some((m, y))
}

/// (yıl, ay) Unix saniyesinden.
fn year_month(unix: u64) -> (u32, u32) {
    // Howard Hinnant'ın civil_from_days algoritması.
    let z = (unix / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y as u32, m as u32)
}

/// Kartın süresi geçmiş mi (son kullanma ayının sonuna kadar geçerlidir).
pub fn expired(stored: &str, unix: u64) -> bool {
    let Some((m, y)) = expiry(stored) else { return false };
    let (ny, nm) = year_month(unix);
    (y, m) < (ny, nm)
}

/// Luhn denetimi (kart numarasındaki yazım hatasını yakalar).
pub fn luhn(d: &str) -> bool {
    if d.len() < 12 || !d.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let sum: u32 = d.bytes().rev().enumerate().map(|(i, b)| {
        let n = (b - b'0') as u32;
        if i % 2 == 1 {
            let x = n * 2;
            if x > 9 { x - 9 } else { x }
        } else {
            n
        }
    }).sum();
    sum.is_multiple_of(10)
}

/// Kart türü numaranın başından.
pub fn brand(d: &str) -> Option<&'static str> {
    let p = |n: usize| d.get(..n).and_then(|s| s.parse::<u32>().ok());
    Some(match () {
        _ if d.starts_with('4') => "Visa",
        _ if p(4).is_some_and(|x| (2221..=2720).contains(&x)) || p(2).is_some_and(|x| (51..=55).contains(&x)) => "Mastercard",
        _ if is_amex(d) => "American Express",
        _ if d.starts_with("9792") => "Troy",
        _ if p(4).is_some_and(|x| (3528..=3589).contains(&x)) => "JCB",
        _ if d.starts_with("6011") || d.starts_with("65") || p(3).is_some_and(|x| (644..=649).contains(&x)) => "Discover",
        _ if d.starts_with("62") => "UnionPay",
        _ if d.starts_with("50") || p(2).is_some_and(|x| (56..=69).contains(&x)) => "Maestro",
        _ => return None,
    })
}

/// Etiketten biçim tahmini (biçimsiz eski kart kayıtları ve içe aktarma için).
pub fn guess(label: &str) -> Format {
    let l = label.to_lowercase();
    let has = |ks: &[&str]| ks.iter().any(|k| l.contains(k));
    if has(&["cvv", "cvc", "cv2", "güvenlik kodu", "security code"]) {
        Format::Cvv
    } else if has(&["son kullanma", "skt", "expir", "geçerlilik", "valid"]) {
        Format::Expiry
    } else if has(&["pin", "şifre"]) {
        Format::Pin
    } else if has(&["numara", "number", "no", "kart no"]) && !has(&["telefon", "phone"]) {
        Format::CardNumber
    } else if has(&["ad", "name", "isim", "sahibi", "holder"]) {
        Format::Holder
    } else {
        Format::Free
    }
}

/// Eski (biçimsiz) kart kaydının alanlarına biçim verir; değeri rakama çevirir.
/// Değeri biçime uymayan alan (ör. "PIN" alanına not yazılmış) olduğu gibi kalır.
pub fn upgrade(category: Category, body: &mut Body) {
    if category != Category::Card {
        return;
    }
    let mut used = Vec::new();
    for f in body.fields.iter_mut().filter(|f| f.fmt == Format::Free && f.kind != FieldKind::Multiline) {
        let g = guess(&f.label);
        if g == Format::Free || used.contains(&g) {
            continue;
        }
        let v = f.value.trim();
        let fits = match g {
            _ if v.is_empty() || !g.numeric() => true,
            Format::Expiry => expiry(v).is_some(),
            _ => v.chars().all(|c| c.is_ascii_digit() || c == ' ' || c == '-') && digits(v).len() <= max_digits(g, &digits(v)),
        };
        if fits {
            f.fmt = g;
            f.value = store(g, &load(g, &f.value));
            used.push(g);
        }
    }
}

/// Hızlı kopyalanabilen kart parçası.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Part {
    Number,
    NumberSpaced,
    Holder,
    Month,
    Year2,
    Year4,
    MmYy,
    MmYyyy,
    Cvv,
}

impl Part {
    pub const ALL: [Part; 9] = [Part::Number, Part::NumberSpaced, Part::Holder, Part::Month, Part::Year2, Part::Year4, Part::MmYy, Part::MmYyyy, Part::Cvv];

    pub fn title(self) -> &'static str {
        use carlui_core::t;
        match self {
            Part::Number => t!("Numara", "Number"),
            Part::NumberSpaced => t!("Numara (boşluklu)", "Number (spaced)"),
            Part::Holder => t!("Ad", "Name"),
            Part::Month => t!("Ay (AA)", "Month (MM)"),
            Part::Year2 => t!("Yıl (YY)", "Year (YY)"),
            Part::Year4 => t!("Yıl (YYYY)", "Year (YYYY)"),
            Part::MmYy => t!("AA/YY", "MM/YY"),
            Part::MmYyyy => t!("AA/YYYY", "MM/YYYY"),
            Part::Cvv => "CVV",
        }
    }

    /// Token deposundaki yuva (alan sıralarıyla çakışmasın).
    pub fn slot(self) -> usize {
        10_000 + self as usize
    }

    /// Bu parçanın değeri; kartta yoksa None.
    pub fn value(self, body: &Body) -> Option<Zeroizing<String>> {
        let field = |fmt: Format| body.fields.iter().find(|f: &&Field| f.fmt == fmt && !f.value.trim().is_empty()).map(|f| f.value.trim());
        let exp = || field(Format::Expiry).and_then(expiry);
        let v = match self {
            Part::Number => digits(field(Format::CardNumber)?),
            Part::NumberSpaced => group(&digits(field(Format::CardNumber)?)),
            Part::Holder => field(Format::Holder)?.to_string(),
            Part::Month => format!("{:02}", exp()?.0),
            Part::Year2 => format!("{:02}", exp()?.1 % 100),
            Part::Year4 => exp()?.1.to_string(),
            Part::MmYy => format!("{:02}/{:02}", exp()?.0, exp()?.1 % 100),
            Part::MmYyyy => format!("{:02}/{}", exp()?.0, exp()?.1),
            Part::Cvv => digits(field(Format::Cvv)?),
        };
        Some(Zeroizing::new(v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(fmt: Format, s: &str) -> String {
        let mut raw = String::new();
        for c in s.chars() {
            if let Some(p) = push(fmt, &raw, c) {
                raw.push_str(&p);
            }
        }
        raw
    }

    #[test]
    fn number_typing() {
        let raw = typed(Format::CardNumber, "4543 12a34-1234 1234 99999");
        assert_eq!(raw, "4543123412341234999", "harf ve tire atlanır, en çok 19 rakam");
        assert_eq!(display(Format::CardNumber, "4543123412341234", false), "4543 1234 1234 1234");
        assert_eq!(display(Format::CardNumber, "4543123412341234", true), "•••• •••• •••• 1234");
        assert_eq!(display(Format::CardNumber, "378282246310005", false), "3782 822463 10005", "Amex 4-6-5");
        assert_eq!(display(Format::CardNumber, "378282246310005", true), "•••• •••••• •0005");
        assert_eq!(typed(Format::CardNumber, "3782822463100059999").len(), 15, "Amex 15 hane");
        assert_eq!(store(Format::CardNumber, "4543 1234"), "45431234");
    }

    #[test]
    fn expiry_typing() {
        assert_eq!(typed(Format::Expiry, "4"), "04", "tek haneli ay başına 0 alır");
        assert_eq!(typed(Format::Expiry, "1327"), "127", "13. ay yok: 3 atlanır");
        assert_eq!(typed(Format::Expiry, "00427"), "0427");
        assert_eq!(typed(Format::Expiry, "12/2930"), "1229");
        assert_eq!(display(Format::Expiry, "04", false), "04/");
        assert_eq!(display(Format::Expiry, "0427", false), "04/27");
        assert_eq!(store(Format::Expiry, "0427"), "04/27");
        assert_eq!(load(Format::Expiry, "4/2027"), "0427");
        assert_eq!(load(Format::Expiry, "2027-04"), "0427");
        assert_eq!(expiry("04/27"), Some((4, 2027)));
        assert_eq!(expiry("13/27"), None);
        assert_eq!(expiry("bozuk"), None);
    }

    #[test]
    fn short_codes() {
        assert_eq!(typed(Format::Cvv, "12a345"), "1234");
        assert_eq!(typed(Format::Pin, "1234567"), "123456");
        assert_eq!(display(Format::Cvv, "123", true), "•••");
    }

    #[test]
    fn brands_and_luhn() {
        assert!(luhn("4111111111111111"));
        assert!(!luhn("4111111111111112"));
        assert!(luhn("378282246310005"));
        assert_eq!(brand("4111"), Some("Visa"));
        assert_eq!(brand("5555"), Some("Mastercard"));
        assert_eq!(brand("2223"), Some("Mastercard"));
        assert_eq!(brand("3782"), Some("American Express"));
        assert_eq!(brand("9792"), Some("Troy"));
        assert_eq!(brand("1234"), None);
    }

    #[test]
    fn expiry_dates() {
        // 2026-11-04
        let now = 1_793_750_400;
        assert!(!expired("11/26", now), "bu ayın sonuna kadar geçerli");
        assert!(expired("10/26", now));
        assert!(!expired("01/27", now));
    }

    #[test]
    fn parts_and_upgrade() {
        let mut body = Body {
            fields: vec![
                Field { label: "Kart üzerindeki ad".into(), value: "CARL TEST".into(), kind: FieldKind::Text, fmt: Format::Free },
                Field { label: "Kart numarası".into(), value: "4111 1111 1111 1111".into(), kind: FieldKind::Secret, fmt: Format::Free },
                Field { label: "Son kullanma".into(), value: "4/2027".into(), kind: FieldKind::Text, fmt: Format::Free },
                Field { label: "CVV".into(), value: "123".into(), kind: FieldKind::Secret, fmt: Format::Free },
                Field { label: "PIN".into(), value: "not: bankadan sor".into(), kind: FieldKind::Secret, fmt: Format::Free },
            ],
            file: None,
            passkeys: Vec::new(),
        };
        upgrade(Category::Card, &mut body);
        assert_eq!(body.fields[1].value, "4111111111111111");
        assert_eq!(body.fields[2].value, "04/27");
        assert_eq!(body.fields[4].fmt, Format::Free, "rakam olmayan değer bozulmaz");
        assert_eq!(body.fields[4].value, "not: bankadan sor");
        let v = |p: Part| p.value(&body).map(|z| z.to_string());
        assert_eq!(v(Part::Number).as_deref(), Some("4111111111111111"));
        assert_eq!(v(Part::NumberSpaced).as_deref(), Some("4111 1111 1111 1111"));
        assert_eq!(v(Part::Holder).as_deref(), Some("CARL TEST"));
        assert_eq!(v(Part::Month).as_deref(), Some("04"));
        assert_eq!(v(Part::Year2).as_deref(), Some("27"));
        assert_eq!(v(Part::Year4).as_deref(), Some("2027"));
        assert_eq!(v(Part::MmYyyy).as_deref(), Some("04/2027"));
        assert_eq!(v(Part::Cvv).as_deref(), Some("123"));
    }
}
