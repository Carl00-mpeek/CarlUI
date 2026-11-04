//! Kurtarma anahtarı. Sorun: ana parola unutulursa kasa sonsuza dek kapalı
//! kalır. Çözüm (isteğe bağlı): 256 bitlik rastgele bir anahtar ana anahtarı
//! ikinci kez sarar. Kullanıcıya okunabilir biçimde gösterilir: Crockford
//! base32, 4'lü gruplar (ör. `7K2M-…`). Yazarken büyük/küçük harf, boşluk ve
//! tire önemsizdir; O→0, I/L→1 düzeltilir. Anahtar hiçbir yere gönderilmez ve
//! CarlUI tarafından saklanmaz; kaybedilirse (parola da unutulmuşsa) veri
//! kurtarılamaz.

use crate::crypto::{Key, Result, KEY_LEN};
use zeroize::Zeroizing;

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Anahtarı gösterilecek metne çevirir.
pub fn format(k: &Key) -> Zeroizing<String> {
    let mut out = Zeroizing::new(String::new());
    let (mut acc, mut bits, mut n) = (0u32, 0u32, 0usize);
    let mut push = |c: u8, out: &mut String| {
        if n > 0 && n % 4 == 0 {
            out.push('-');
        }
        out.push(c as char);
        n += 1;
    };
    for &b in k.bytes() {
        acc = (acc << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            push(ALPHABET[((acc >> bits) & 31) as usize], &mut out);
        }
    }
    if bits > 0 {
        push(ALPHABET[((acc << (5 - bits)) & 31) as usize], &mut out);
    }
    out
}

/// Kullanıcının yazdığı metni anahtara çevirir; biçim tutmazsa None.
pub fn parse(s: &str) -> Option<Key> {
    let mut bytes = Zeroizing::new(Vec::with_capacity(KEY_LEN + 1));
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.chars() {
        let c = match c.to_ascii_uppercase() {
            '-' | ' ' => continue,
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        };
        let v = ALPHABET.iter().position(|&a| a as char == c)? as u32;
        acc = ((acc << 5) | v) & 0xFFFF;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            bytes.push((acc >> bits) as u8);
        }
    }
    if bytes.len() != KEY_LEN {
        return None;
    }
    Key::from_bytes(&bytes)
}

pub fn new() -> Result<Key> {
    Key::random()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_lenient() {
        let k = new().unwrap();
        let s = format(&k);
        assert_eq!(s.len(), 52 + 12);
        assert_eq!(parse(&s).unwrap(), k);
        let messy = s.to_lowercase().replace('-', " ").replace('0', "o");
        assert_eq!(parse(&messy).unwrap(), k);
        assert!(parse("ABCD-EFGH").is_none());
        assert!(parse(&format!("{}U", &s[..s.len() - 1])).is_none());
    }
}
