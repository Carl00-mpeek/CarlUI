//! Şifreleme yapı taşları. Sorun: kasadaki her şey diskte şifreli durmalı,
//! ana parola hiçbir yere yazılmamalı ve yanlış parola/bozulmuş veri sessizce
//! çöp üretmek yerine açıkça reddedilmeli. Çözüm:
//! - Parola → Argon2id (bellek zorlayıcı) → 32 baytlık anahtar şifreleme anahtarı.
//! - Tüm şifreleme XChaCha20-Poly1305 (AEAD): 24 baytlık rastgele nonce, her
//!   mühürde yeni; doğrulama etiketi tutmazsa açma hata verir.
//! - "Ek veri" (AAD) her mührü bağlamına bağlar: bir kaydın şifreli gövdesi
//!   başka bir kaydın yerine konamaz, ana anahtar sarmalı kayıt anahtarı
//!   sarmalıyla karıştırılamaz.
//! - Anahtarlar `Key` içinde tutulur; düşerken bellekten silinir (`zeroize`).

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

pub const KEY_LEN: usize = 32;
/// Kasa dosyasındaki Argon2 ayarlarının üst sınırları (varsayılan 64 MiB, 3, 1).
const MAX_M_KIB: u32 = 1024 * 1024;
const MAX_T: u32 = 16;
const MAX_P: u32 = 16;
const NONCE_LEN: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Parola ya da kurtarma anahtarı yanlış (sarmal açılamadı).
    WrongKey,
    /// Etiket tutmadı: veri bozulmuş ya da değiştirilmiş.
    Corrupt,
    Random,
    Io(String),
    Format(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::WrongKey => f.write_str(carlui_core::t!("Parola yanlış.", "Wrong password.")),
            Error::Corrupt => f.write_str(carlui_core::t!("Kasa verisi bozulmuş ya da değiştirilmiş.", "Vault data is damaged or was modified.")),
            Error::Random => f.write_str(carlui_core::t!("Rastgele sayı üretilemedi.", "Couldn't generate random bytes.")),
            Error::Io(e) => write!(f, "{}: {e}", carlui_core::t!("Dosya hatası", "File error")),
            Error::Format(e) => write!(f, "{}: {e}", carlui_core::t!("Biçim hatası", "Format error")),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// 32 baytlık gizli anahtar; düşerken sıfırlanır, `Debug` içeriği göstermez.
#[derive(Clone, Zeroize, ZeroizeOnDrop, PartialEq, Eq)]
pub struct Key([u8; KEY_LEN]);

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Key(…)")
    }
}

impl Key {
    pub fn random() -> Result<Self> {
        let mut k = [0u8; KEY_LEN];
        getrandom::getrandom(&mut k).map_err(|_| Error::Random)?;
        Ok(Key(k))
    }

    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        let a: [u8; KEY_LEN] = b.try_into().ok()?;
        Some(Key(a))
    }

    pub fn bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }
}

pub fn random<const N: usize>() -> Result<[u8; N]> {
    let mut b = [0u8; N];
    getrandom::getrandom(&mut b).map_err(|_| Error::Random)?;
    Ok(b)
}

/// Argon2id ayarları; kasa başlığında saklanır ki ileride değiştirilebilsin.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct KdfParams {
    /// KiB cinsinden bellek.
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
    #[serde(with = "crate::hex")]
    pub salt: Vec<u8>,
}

impl KdfParams {
    /// Masaüstü için: 64 MiB, 3 geçiş, tek iş parçacığı (~0,3–0,6 sn).
    pub fn new() -> Result<Self> {
        Ok(Self { m_kib: 64 * 1024, t: 3, p: 1, salt: random::<16>()?.to_vec() })
    }

    /// Testler için hafif ayar (gerçek kasada kullanılmaz).
    #[cfg(test)]
    pub fn fast() -> Self {
        Self { m_kib: 64, t: 1, p: 1, salt: random::<16>().unwrap().to_vec() }
    }

    pub fn derive(&self, password: &[u8]) -> Result<Key> {
        // Saldırganın düşürdüğü ayarlarla zayıf bir kasa kabul edilmesin; aşırı
        // yükseltilmiş ayarlar da (ör. 4 GiB bellek) kasayı açarken dondurmasın.
        if self.salt.len() < 16 || self.salt.len() > 64 || !(8..=MAX_M_KIB).contains(&self.m_kib) || !(1..=MAX_T).contains(&self.t) || !(1..=MAX_P).contains(&self.p) {
            return Err(Error::Format("kdf".into()));
        }
        let params = Params::new(self.m_kib, self.t, self.p, Some(KEY_LEN)).map_err(|e| Error::Format(e.to_string()))?;
        let a = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let mut out = Zeroizing::new([0u8; KEY_LEN]);
        a.hash_password_into(password, &self.salt, out.as_mut()).map_err(|e| Error::Format(e.to_string()))?;
        Ok(Key(*out))
    }
}

/// Mühürlenmiş veri: nonce + şifreli metin (etiket sonda).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Sealed {
    #[serde(with = "crate::hex")]
    pub n: Vec<u8>,
    #[serde(with = "crate::hex")]
    pub c: Vec<u8>,
}

pub fn seal(key: &Key, aad: &[u8], plain: &[u8]) -> Result<Sealed> {
    let cipher = XChaCha20Poly1305::new(key.0.as_ref().into());
    let n = random::<NONCE_LEN>()?;
    let c = cipher.encrypt(XNonce::from_slice(&n), Payload { msg: plain, aad }).map_err(|_| Error::Corrupt)?;
    Ok(Sealed { n: n.to_vec(), c })
}

/// Açar; etiket tutmazsa `Corrupt` (yanlış anahtar da buraya düşer, bkz. `unwrap_key`).
pub fn open(key: &Key, aad: &[u8], s: &Sealed) -> Result<Zeroizing<Vec<u8>>> {
    if s.n.len() != NONCE_LEN {
        return Err(Error::Corrupt);
    }
    let cipher = XChaCha20Poly1305::new(key.0.as_ref().into());
    cipher.decrypt(XNonce::from_slice(&s.n), Payload { msg: &s.c, aad }).map(Zeroizing::new).map_err(|_| Error::Corrupt)
}

/// Bir anahtarı başka bir anahtarla sarar.
pub fn wrap_key(kek: &Key, aad: &[u8], k: &Key) -> Result<Sealed> {
    seal(kek, aad, &k.0)
}

/// Sarmalı açar. Burada etiket hatası "anahtar yanlış" demektir.
pub fn unwrap_key(kek: &Key, aad: &[u8], s: &Sealed) -> Result<Key> {
    let b = open(kek, aad, s).map_err(|_| Error::WrongKey)?;
    Key::from_bytes(&b).ok_or(Error::Corrupt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let k = Key::random().unwrap();
        let s = seal(&k, b"ctx", b"merhaba").unwrap();
        assert_ne!(s.c, b"merhaba");
        assert_eq!(&open(&k, b"ctx", &s).unwrap()[..], b"merhaba");
    }

    #[test]
    fn nonce_is_fresh() {
        let k = Key::random().unwrap();
        let a = seal(&k, b"", b"x").unwrap();
        let b = seal(&k, b"", b"x").unwrap();
        assert_ne!(a.n, b.n);
        assert_ne!(a.c, b.c);
    }

    #[test]
    fn tamper_and_context() {
        let k = Key::random().unwrap();
        let mut s = seal(&k, b"a", b"gizli").unwrap();
        assert_eq!(open(&k, b"b", &s), Err(Error::Corrupt));
        assert_eq!(open(&Key::random().unwrap(), b"a", &s), Err(Error::Corrupt));
        s.c[0] ^= 1;
        assert_eq!(open(&k, b"a", &s), Err(Error::Corrupt));
    }

    #[test]
    fn kdf_deterministic_and_salted() {
        let p = KdfParams::fast();
        assert_eq!(p.derive(b"pw").unwrap(), p.derive(b"pw").unwrap());
        assert_ne!(p.derive(b"pw").unwrap(), p.derive(b"pW").unwrap());
        assert_ne!(p.derive(b"pw").unwrap(), KdfParams::fast().derive(b"pw").unwrap());
        let weak = KdfParams { salt: vec![0; 4], ..KdfParams::fast() };
        assert!(weak.derive(b"pw").is_err());
    }

    #[test]
    fn kdf_rejects_tampered_params() {
        // Dosyada kurcalanmış ayarlar Argon2 çalışmadan reddedilir (anında döner).
        let base = KdfParams::fast();
        let started = std::time::Instant::now();
        for bad in [
            KdfParams { m_kib: 4 * 1024 * 1024, ..base.clone() },
            KdfParams { m_kib: u32::MAX, ..base.clone() },
            KdfParams { m_kib: 4, ..base.clone() },
            KdfParams { t: 0, ..base.clone() },
            KdfParams { t: 1_000_000, ..base.clone() },
            KdfParams { p: 0, ..base.clone() },
            KdfParams { p: 255, ..base.clone() },
            KdfParams { salt: vec![0; 1 << 20], ..base.clone() },
        ] {
            assert!(matches!(bad.derive(b"pw"), Err(Error::Format(_))), "m {} t {} p {}", bad.m_kib, bad.t, bad.p);
        }
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        assert!(KdfParams::new().unwrap().derive(b"pw").is_ok(), "varsayılan ayar sınırın içinde");
    }

    #[test]
    fn wrap() {
        let kek = Key::random().unwrap();
        let k = Key::random().unwrap();
        let w = wrap_key(&kek, b"mk", &k).unwrap();
        assert_eq!(unwrap_key(&kek, b"mk", &w).unwrap(), k);
        assert_eq!(unwrap_key(&Key::random().unwrap(), b"mk", &w), Err(Error::WrongKey));
    }
}
