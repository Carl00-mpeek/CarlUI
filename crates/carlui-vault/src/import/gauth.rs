//! Google Authenticator "Hesapları aktar". Uygulama hesapları dosyaya değil,
//! `otpauth-migration://offline?data=…` adresli QR kodlarına koyar. `data`
//! base64 kodlu bir protobuf iletisidir:
//!
//! ```text
//! MigrationPayload { repeated OtpParameters otp_parameters = 1; … }
//! OtpParameters { bytes secret = 1; string name = 2; string issuer = 3;
//!                 Algorithm algorithm = 4; DigitCount digits = 5; OtpType type = 6; }
//! ```
//!
//! Burada yalnızca bu alanları okuyan küçük bir protobuf çözücü var; her TOTP
//! hesabı bir otpauth adresine çevrilip 2FA kaydı olur. HOTP (sayaçlı)
//! hesaplar desteklenmez, atlanır.

use super::{base32, otpauth, two_factor, Batch};
use zeroize::Zeroizing;

/// Base64 (standart ya da URL için); dolgu ve boşluk yok sayılır.
fn base64(s: &str) -> Option<Zeroizing<Vec<u8>>> {
    let mut out = Zeroizing::new(Vec::with_capacity(s.len() * 3 / 4));
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes().filter(|c| !matches!(c, b'=' | b' ' | b'\n' | b'\r')) {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32;
        acc = ((acc << 6) | v) & 0xFFFFFF;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// Protobuf okuyucu: (alan numarası, tel türü) ve değer.
struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

enum Val<'a> {
    Int(u64),
    Bytes(&'a [u8]),
}

impl<'a> Reader<'a> {
    fn varint(&mut self) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let b = *self.b.get(self.i)?;
            self.i += 1;
            v |= ((b & 0x7F) as u64) << shift;
            if b & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }

    fn next(&mut self) -> Option<Option<(u64, Val<'a>)>> {
        if self.i >= self.b.len() {
            return Some(None);
        }
        let tag = self.varint()?;
        let val = match tag & 7 {
            0 => Val::Int(self.varint()?),
            1 => {
                self.i = self.i.checked_add(8).filter(|&i| i <= self.b.len())?;
                Val::Int(0)
            }
            2 => {
                let n = self.varint()? as usize;
                let end = self.i.checked_add(n).filter(|&e| e <= self.b.len())?;
                let s = &self.b[self.i..end];
                self.i = end;
                Val::Bytes(s)
            }
            5 => {
                self.i = self.i.checked_add(4).filter(|&i| i <= self.b.len())?;
                Val::Int(0)
            }
            _ => return None,
        };
        Some(Some((tag >> 3, val)))
    }
}

/// Adresi çözer; adres bozuksa None.
pub fn parse(uri: &str) -> Option<Batch> {
    let q = uri.trim().split_once('?')?.1;
    let data = q.split('&').find_map(|p| p.strip_prefix("data="))?;
    let raw = base64(&crate::totp::unescape(data))?;
    let mut b = Batch { source: "Google Authenticator".into(), items: Vec::new(), skipped: 0 };
    let mut top = Reader { b: &raw, i: 0 };
    while let Some((field, val)) = top.next()? {
        let (1, Val::Bytes(p)) = (field, val) else { continue };
        let (mut secret, mut name, mut issuer) = (Zeroizing::new(Vec::new()), String::new(), String::new());
        let (mut algo, mut digits, mut kind) = (1u64, 1u64, 2u64);
        let mut r = Reader { b: p, i: 0 };
        while let Some((f, v)) = r.next()? {
            match (f, v) {
                (1, Val::Bytes(s)) => secret = Zeroizing::new(s.to_vec()),
                (2, Val::Bytes(s)) => name = String::from_utf8_lossy(s).into_owned(),
                (3, Val::Bytes(s)) => issuer = String::from_utf8_lossy(s).into_owned(),
                (4, Val::Int(x)) => algo = x,
                (5, Val::Int(x)) => digits = x,
                (6, Val::Int(x)) => kind = x,
                _ => {}
            }
        }
        // type: 1 HOTP, 2 TOTP (0 = belirtilmemiş, TOTP sayılır); algoritma 4 = MD5.
        if kind == 1 || algo == 4 || secret.len() < 10 {
            b.skipped += 1;
            continue;
        }
        // Ad çoğu zaman "Sağlayıcı:hesap" biçimindedir.
        let account = match name.split_once(':') {
            Some((i, a)) if issuer.is_empty() || i.trim() == issuer.trim() => {
                if issuer.is_empty() {
                    issuer = i.trim().to_string();
                }
                a.trim().to_string()
            }
            _ => name.trim().to_string(),
        };
        let algo = match algo {
            2 => "SHA256",
            3 => "SHA512",
            _ => "SHA1",
        };
        let digits = if digits == 2 { 8 } else { 6 };
        let key = Zeroizing::new(base32(&secret));
        let uri = Zeroizing::new(otpauth(&issuer, &account, &key, digits, 30, algo));
        b.items.push(two_factor(&issuer, &account, &uri));
    }
    Some(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::FieldKind;

    #[test]
    fn known_example() {
        // Yaygın örnek: secret "Hello!\xde\xad\xbe\xef", ad "Example:alice@google.com", sağlayıcı "Example".
        let uri = "otpauth-migration://offline?data=CjEKCkhlbGxvId6tvu8SGEV4YW1wbGU6YWxpY2VAZ29vZ2xlLmNvbRoHRXhhbXBsZSABKAEwAhABGAEgACjr4JKK%2Bf%2F%2F%2F%2F8B";
        let b = parse(uri).unwrap();
        assert_eq!(b.items.len(), 1);
        let it = &b.items[0];
        assert_eq!(it.meta.name, "Example");
        let key = it.body.fields.iter().find(|f| f.kind == FieldKind::Totp).unwrap();
        assert_eq!(key.value, "otpauth://totp/Example:alice%40google.com?secret=JBSWY3DPEHPK3PXP&issuer=Example");
        let account = it.body.fields.iter().find(|f| f.kind == FieldKind::Text).unwrap();
        assert_eq!(account.value, "alice@google.com");
    }

    #[test]
    fn hotp_skipped_and_garbage() {
        // Elle kurulmuş ileti: biri HOTP (type 1), biri SHA256 8 haneli TOTP.
        let param = |kind: u8, algo: u8, digits: u8| {
            let mut p = vec![0x0A, 10];
            p.extend_from_slice(b"0123456789");
            p.extend_from_slice(&[0x12, 1, b'a', 0x20, algo, 0x28, digits, 0x30, kind]);
            let mut o = vec![0x0A, p.len() as u8];
            o.extend(p);
            o
        };
        let mut msg = param(1, 1, 1);
        msg.extend(param(2, 2, 2));
        let enc = {
            const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut s = String::new();
            for ch in msg.chunks(3) {
                let n = ch.iter().fold(0u32, |a, &b| (a << 8) | b as u32) << (8 * (3 - ch.len()));
                for k in 0..=ch.len() {
                    s.push(A[((n >> (18 - 6 * k)) & 63) as usize] as char);
                }
            }
            s
        };
        let b = parse(&format!("otpauth-migration://offline?data={enc}")).unwrap();
        assert_eq!((b.items.len(), b.skipped), (1, 1));
        let t = crate::totp::Totp::parse(&b.items[0].body.fields[1].value).unwrap();
        assert_eq!((t.digits, t.algo), (8, crate::totp::Algo::Sha256));
        assert!(parse("otpauth-migration://offline?data=!!!").is_none());
        assert!(parse("otpauth-migration://offline?data=CgU").is_none(), "kesik ileti");
    }
}
