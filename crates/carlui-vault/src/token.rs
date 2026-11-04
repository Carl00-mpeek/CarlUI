//! Tek kullanımlık pano belirteçleri (Aşama 2). Bir alan kopyalanınca değer
//! panoya değil bu depoya konur; panoya yalnızca 32 rastgele bayttan oluşan
//! belirteç gider. Belirteç bir kayıt + alan çiftine bağlıdır (aynı alan
//! yeniden kopyalanınca eskisi geçersiz olur), `TTL_MS` sonra ya da bir kez
//! kullanılınca silinir, kasa kilitlenince hepsi temizlenir. Değerler
//! `Zeroizing` içinde tutulur.

use crate::{crypto, hex};
use zeroize::Zeroizing;

pub use carlui_core::vaultlink::TTL_MS;

/// Aynı anda en çok bu kadar belirteç; fazlası en eskiden silinir.
const MAX: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenError {
    /// Hiç verilmedi, kullanıldı ya da kasa kilitlendi.
    Unknown,
    Expired,
}

struct Pending {
    token: [u8; 32],
    /// Kayıt kimliği (kaydedilmemiş yeni kayıtta boş) ve alan sırası.
    slot: (String, usize),
    label: String,
    value: Zeroizing<String>,
    expires: i64,
}

impl Drop for Pending {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.token);
    }
}

#[derive(Default)]
pub struct Tokens {
    list: Vec<Pending>,
}

/// Sabit süreli karşılaştırma.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Taşınan belirteç: kayıt adı "Ad · Alan".
pub struct Issued {
    pub token: Zeroizing<String>,
    pub label: String,
    pub expires: i64,
}

impl Tokens {
    /// `value` için yeni belirteç verir; aynı kayıt+alanın eski belirteci silinir.
    pub fn issue(&mut self, entry: &str, field: usize, label: &str, value: &str, now: i64) -> Result<Issued, crypto::Error> {
        self.sweep(now);
        self.list.retain(|p| p.slot != (entry.to_string(), field));
        if self.list.len() >= MAX {
            self.list.remove(0);
        }
        let token = crypto::random::<32>()?;
        let expires = now + TTL_MS;
        let out = Issued { token: Zeroizing::new(hex::encode(&token)), label: label.to_string(), expires };
        self.list.push(Pending { token, slot: (entry.to_string(), field), label: label.to_string(), value: Zeroizing::new(value.to_string()), expires });
        Ok(out)
    }

    /// Belirteci tüketir ve değeri verir. Süresi dolmuşsa da silinir.
    pub fn take(&mut self, token: &str, now: i64) -> Result<(Zeroizing<String>, String), TokenError> {
        let Some(raw) = hex::decode(token.trim()).map(Zeroizing::new) else { return Err(TokenError::Unknown) };
        let Some(i) = self.list.iter().position(|p| same(&p.token, &raw)) else { return Err(TokenError::Unknown) };
        let p = self.list.remove(i);
        if now >= p.expires {
            return Err(TokenError::Expired);
        }
        Ok((p.value.clone(), p.label.clone()))
    }

    /// Süresi dolanları siler.
    pub fn sweep(&mut self, now: i64) {
        self.list.retain(|p| now < p.expires);
    }

    pub fn clear(&mut self) {
        self.list.clear();
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_use() {
        let mut t = Tokens::default();
        let a = t.issue("e1", 0, "GitHub · Parola", "s3cret", 1000).unwrap();
        assert_eq!(a.token.len(), 64);
        let (v, label) = t.take(&a.token, 2000).unwrap();
        assert_eq!(v.as_str(), "s3cret");
        assert_eq!(label, "GitHub · Parola");
        assert_eq!(t.take(&a.token, 2000).unwrap_err(), TokenError::Unknown);
    }

    #[test]
    fn expiry() {
        let mut t = Tokens::default();
        let a = t.issue("e1", 0, "x", "v", 0).unwrap();
        assert_eq!(t.take(&a.token, TTL_MS).unwrap_err(), TokenError::Expired);
        assert!(t.is_empty());
        let b = t.issue("e1", 0, "x", "v", 0).unwrap();
        t.sweep(TTL_MS + 1);
        assert_eq!(t.take(&b.token, 1).unwrap_err(), TokenError::Unknown);
    }

    #[test]
    fn wrong_and_replaced_tokens() {
        let mut t = Tokens::default();
        let a = t.issue("e1", 0, "x", "old", 0).unwrap();
        let b = t.issue("e1", 0, "x", "new", 0).unwrap();
        let c = t.issue("e1", 1, "y", "other", 0).unwrap();
        assert_ne!(a.token.as_str(), b.token.as_str());
        assert_eq!(t.take(&a.token, 1).unwrap_err(), TokenError::Unknown, "aynı alanın eski belirteci geçersiz");
        assert_eq!(t.take("zz", 1).unwrap_err(), TokenError::Unknown);
        let mut flipped = b.token.to_string();
        flipped.replace_range(0..1, if flipped.starts_with('0') { "1" } else { "0" });
        assert_eq!(t.take(&flipped, 1).unwrap_err(), TokenError::Unknown);
        assert_eq!(t.take(&b.token, 1).unwrap().0.as_str(), "new");
        assert_eq!(t.take(&c.token, 1).unwrap().0.as_str(), "other");
        let d = t.issue("e2", 0, "z", "v", 0).unwrap();
        t.clear();
        assert_eq!(t.take(&d.token, 1).unwrap_err(), TokenError::Unknown);
    }

    #[test]
    fn bounded() {
        let mut t = Tokens::default();
        let first = t.issue("e", 0, "x", "v", 0).unwrap();
        for i in 1..40 {
            t.issue("e", i, "x", "v", 0).unwrap();
        }
        assert_eq!(t.len(), MAX);
        assert_eq!(t.take(&first.token, 1).unwrap_err(), TokenError::Unknown);
    }
}
