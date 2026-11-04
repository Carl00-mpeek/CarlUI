//! Aşama 3: 2FA kodu (TOTP, RFC 6238). `FieldKind::Totp` alanının değeri ya
//! düz base32 gizli anahtardır ("JBSW Y3DP …", boşluk/küçük harf olabilir) ya da
//! QR kodlarındaki `otpauth://totp/…?secret=…&digits=…&period=…&algorithm=…`
//! adresi. Kod yalnızca düzenleyici açıkken, o anki saatten üretilir; hiçbir
//! yere yazılmaz. Çözülen anahtar düşerken bellekten silinir.

use hmac::{Hmac, Mac};
use zeroize::{Zeroize, Zeroizing};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Algo {
    Sha1,
    Sha256,
    Sha512,
}

pub struct Totp {
    secret: Zeroizing<Vec<u8>>,
    pub digits: u32,
    pub period: u64,
    pub algo: Algo,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TotpError {
    Empty,
    /// Base32 dışı karakter ya da çok kısa anahtar.
    BadSecret,
    /// otpauth adresi tanınmadı (HOTP, bilinmeyen algoritma, geçersiz hane/süre).
    BadUri,
}

impl std::fmt::Display for TotpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use carlui_core::t;
        f.write_str(match self {
            TotpError::Empty => t!("Gizli anahtar boş.", "The secret key is empty."),
            TotpError::BadSecret => t!("Gizli anahtar geçersiz (base32 olmalı).", "The secret key isn't valid (must be base32)."),
            TotpError::BadUri => t!("otpauth adresi desteklenmiyor.", "This otpauth address isn't supported."),
        })
    }
}

/// RFC 4648 base32; boşluk, tire ve `=` dolgusu yok sayılır, harf büyüklüğü önemsiz.
fn base32(s: &str) -> Option<Zeroizing<Vec<u8>>> {
    let mut out = Zeroizing::new(Vec::with_capacity(s.len() * 5 / 8));
    let (mut acc, mut bits) = (0u64, 0u32);
    for c in s.chars().filter(|c| !matches!(c, ' ' | '-' | '=' | '\t')) {
        let v = match c.to_ascii_uppercase() {
            u @ 'A'..='Z' => u as u64 - 'A' as u64,
            d @ '2'..='7' => d as u64 - '2' as u64 + 26,
            _ => return None,
        };
        acc = ((acc << 5) | v) & 0xFFFF;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    acc.zeroize();
    Some(out)
}

/// `%XX` çözümü (anahtar ve sorgu değerleri için yeter).
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if let Some(x) = (b[i] == b'%').then(|| s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())).flatten() {
            out.push(x);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl Totp {
    pub fn parse(value: &str) -> Result<Totp, TotpError> {
        let v = value.trim();
        if v.is_empty() {
            return Err(TotpError::Empty);
        }
        let mut t = Totp { secret: Zeroizing::new(Vec::new()), digits: 6, period: 30, algo: Algo::Sha1 };
        let secret = if v.len() > 10 && v[..10].eq_ignore_ascii_case("otpauth://") {
            let rest = &v[10..];
            if !rest.get(..5).is_some_and(|k| k.eq_ignore_ascii_case("totp/")) {
                return Err(TotpError::BadUri);
            }
            let query = rest.split_once('?').map(|(_, q)| q).unwrap_or("");
            let mut secret = None;
            for pair in query.split('&') {
                let (k, val) = pair.split_once('=').unwrap_or((pair, ""));
                let val = unescape(val);
                match k.to_ascii_lowercase().as_str() {
                    "secret" => secret = Some(Zeroizing::new(val)),
                    "digits" => t.digits = val.parse().map_err(|_| TotpError::BadUri)?,
                    "period" => t.period = val.parse().map_err(|_| TotpError::BadUri)?,
                    "algorithm" => {
                        t.algo = match val.to_ascii_uppercase().as_str() {
                            "SHA1" => Algo::Sha1,
                            "SHA256" => Algo::Sha256,
                            "SHA512" => Algo::Sha512,
                            _ => return Err(TotpError::BadUri),
                        }
                    }
                    _ => {}
                }
            }
            if !(6..=8).contains(&t.digits) || !(1..=300).contains(&t.period) {
                return Err(TotpError::BadUri);
            }
            secret.ok_or(TotpError::BadUri)?
        } else {
            Zeroizing::new(v.to_string())
        };
        t.secret = base32(&secret).ok_or(TotpError::BadSecret)?;
        // 80 bitten kısa anahtar neredeyse her zaman yanlış kopyalanmıştır.
        if t.secret.len() < 10 {
            return Err(TotpError::BadSecret);
        }
        Ok(t)
    }

    fn mac(&self, msg: &[u8]) -> Zeroizing<Vec<u8>> {
        macro_rules! run {
            ($h:ty) => {{
                let mut m = <Hmac<$h> as Mac>::new_from_slice(&self.secret).expect("hmac her uzunluğu kabul eder");
                m.update(msg);
                Zeroizing::new(m.finalize().into_bytes().to_vec())
            }};
        }
        match self.algo {
            Algo::Sha1 => run!(sha1::Sha1),
            Algo::Sha256 => run!(sha2::Sha256),
            Algo::Sha512 => run!(sha2::Sha512),
        }
    }

    /// Verilen Unix saniyesindeki kod (baştaki sıfırlar korunur).
    pub fn code(&self, unix: u64) -> Zeroizing<String> {
        let h = self.mac(&(unix / self.period).to_be_bytes());
        let o = (h[h.len() - 1] & 0x0F) as usize;
        let n = u32::from_be_bytes([h[o] & 0x7F, h[o + 1], h[o + 2], h[o + 3]]) % 10u32.pow(self.digits);
        Zeroizing::new(format!("{n:0w$}", w = self.digits as usize))
    }

    /// Kodun geçerli kalacağı saniye (1..=period).
    pub fn remaining(&self, unix: u64) -> u64 {
        self.period - unix % self.period
    }
}

/// otpauth adresinden (sağlayıcı, hesap): `otpauth://totp/GitHub:carl?issuer=GitHub`
/// → ("GitHub", "carl"). Düz anahtarda ikisi de boş.
pub fn names(value: &str) -> (String, String) {
    let v = value.trim();
    let Some(rest) = v.get(..15).filter(|p| p.eq_ignore_ascii_case("otpauth://totp/")).map(|_| &v[15..]) else { return Default::default() };
    let (label, query) = rest.split_once('?').unwrap_or((rest, ""));
    let label = unescape(label);
    let (mut issuer, account) = match label.split_once(':') {
        Some((i, a)) => (i.trim().to_string(), a.trim().to_string()),
        None => (String::new(), label.trim().to_string()),
    };
    for (k, val) in query.split('&').filter_map(|p| p.split_once('=')) {
        if k.eq_ignore_ascii_case("issuer") && !val.is_empty() {
            issuer = unescape(val).trim().to_string();
        }
    }
    (issuer, account)
}

/// Gösterim için ortadan böler: "123456" → "123 456".
pub fn grouped(code: &str) -> String {
    let mid = code.len().div_ceil(2);
    format!("{} {}", &code[..mid], &code[mid..])
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 6238 ek B: SHA1/256/512 için ayrı ASCII anahtarlar, 8 hane.
    fn rfc(algo: &str, key: &[u8]) -> Totp {
        let mut b32 = String::new();
        let (mut acc, mut bits) = (0u32, 0);
        for &b in key {
            acc = (acc << 8) | b as u32;
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                b32.push(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567"[((acc >> bits) & 31) as usize] as char);
            }
        }
        if bits > 0 {
            b32.push(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567"[((acc << (5 - bits)) & 31) as usize] as char);
        }
        Totp::parse(&format!("otpauth://totp/X?secret={b32}&digits=8&algorithm={algo}")).unwrap()
    }

    #[test]
    fn rfc6238_vectors() {
        let s1 = rfc("SHA1", b"12345678901234567890");
        let s256 = rfc("SHA256", b"12345678901234567890123456789012");
        let s512 = rfc("SHA512", b"1234567890123456789012345678901234567890123456789012345678901234");
        let cases = [
            (59, "94287082", "46119246", "90693936"),
            (1111111109, "07081804", "68084774", "25091201"),
            (1111111111, "14050471", "67062674", "99943326"),
            (1234567890, "89005924", "91819424", "93441116"),
            (2000000000, "69279037", "90698825", "38618901"),
            (20000000000, "65353130", "77737706", "47863826"),
        ];
        for (t, a, b, c) in cases {
            assert_eq!(s1.code(t).as_str(), a, "sha1 {t}");
            assert_eq!(s256.code(t).as_str(), b, "sha256 {t}");
            assert_eq!(s512.code(t).as_str(), c, "sha512 {t}");
        }
    }

    #[test]
    fn plain_secret_forms() {
        // Aynı anahtarın farklı yazılışları aynı kodu verir.
        let a = Totp::parse("JBSWY3DPEHPK3PXP").unwrap();
        let b = Totp::parse(" jbsw y3dp ehpk 3pxp ").unwrap();
        let c = Totp::parse("otpauth://totp/GitHub:carl?secret=JBSWY3DPEHPK3PXP&issuer=GitHub").unwrap();
        assert_eq!(a.code(1_700_000_000).as_str(), b.code(1_700_000_000).as_str());
        assert_eq!(a.code(1_700_000_000).as_str(), c.code(1_700_000_000).as_str());
        assert_eq!(base32("JBSWY3DPEHPK3PXP").unwrap().as_slice(), &[0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x21, 0xde, 0xad, 0xbe, 0xef]);
        // .NET HMACSHA1 ile ayrıca hesaplanan değer.
        assert_eq!(a.code(1_793_787_336).as_str(), "775566");
        assert_eq!((a.digits, a.period, a.algo), (6, 30, Algo::Sha1));
        assert_eq!(a.remaining(1_700_000_000), 10);
        assert_eq!(grouped("123456"), "123 456");
    }

    #[test]
    fn names_from_uri() {
        assert_eq!(names("otpauth://totp/GitHub:carl%40ornek.com?secret=JBSWY3DPEHPK3PXP&issuer=GitHub"), ("GitHub".into(), "carl@ornek.com".into()));
        assert_eq!(names("otpauth://totp/Eski:ad?secret=X&issuer=Yeni%20Ad"), ("Yeni Ad".into(), "ad".into()));
        assert_eq!(names("otpauth://totp/yalniz-hesap?secret=X"), (String::new(), "yalniz-hesap".into()));
        assert_eq!(names("JBSWY3DPEHPK3PXP"), (String::new(), String::new()));
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(Totp::parse("").err(), Some(TotpError::Empty));
        assert_eq!(Totp::parse("ABC1").err(), Some(TotpError::BadSecret));
        assert_eq!(Totp::parse("JBSW").err(), Some(TotpError::BadSecret));
        assert_eq!(Totp::parse("otpauth://hotp/X?secret=JBSWY3DPEHPK3PXP").err(), Some(TotpError::BadUri));
        assert_eq!(Totp::parse("otpauth://totp/X?secret=JBSWY3DPEHPK3PXP&digits=4").err(), Some(TotpError::BadUri));
        assert_eq!(Totp::parse("otpauth://totp/X?secret=JBSWY3DPEHPK3PXP&algorithm=MD5").err(), Some(TotpError::BadUri));
        assert_eq!(Totp::parse("otpauth://totp/X?issuer=Y").err(), Some(TotpError::BadUri));
    }
}
