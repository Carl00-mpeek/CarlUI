//! Passkey (WebAuthn) doğrulayıcısı. Kasa, tarayıcı eklentisi üzerinden
//! sitelerin `navigator.credentials.create/get` isteklerini yanıtlayan bir
//! yazılım doğrulayıcıdır (Bitwarden/1Password eklentilerindeki gibi):
//! - Yalnızca ES256 (COSE -7, P-256 + SHA-256). Site başka algoritma isterse
//!   eklenti tarayıcının kendi akışına düşer.
//! - Onay "none": doğrulayıcı kimliği (AAGUID) sıfır, imzalı onay yok.
//! - Bayraklar: UP (kullanıcı kasada onayladı), UV (kasa ana parolayla açık),
//!   BE+BS (anahtar kasa dosyasında, yedeklenebilir). Kayıtta ayrıca AT.
//! - clientDataJSON burada, eklentinin değil tarayıcının bildirdiği sekme
//!   kökeninden üretilir (WebAuthn L3 §5.8.1.1 sırası: type, challenge, origin,
//!   crossOrigin).
//!
//! Özel anahtar kaydın gövdesinde (`Body::passkeys`) diğer gizli alanlar gibi
//! şifreli durur; çözülünce `Zeroizing`/`ZeroizeOnDrop` ile silinir.

use p256::ecdsa::{signature::Signer, Signature, SigningKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

/// COSE algoritma kimliği: ES256.
pub const ES256: i64 = -7;

pub const FLAG_UP: u8 = 0x01;
pub const FLAG_UV: u8 = 0x04;
pub const FLAG_BE: u8 = 0x08;
pub const FLAG_BS: u8 = 0x10;
pub const FLAG_AT: u8 = 0x40;
const FLAGS: u8 = FLAG_UP | FLAG_UV | FLAG_BE | FLAG_BS;

/// Kayıttaki passkey. Kimlikler ve anahtar base64url (dolgusuz) metindir.
#[derive(Serialize, Deserialize, Clone, Zeroize, ZeroizeOnDrop, Default, PartialEq, Eq)]
pub struct Passkey {
    pub rp_id: String,
    #[serde(default)]
    pub rp_name: String,
    pub user_name: String,
    #[serde(default)]
    pub user_display: String,
    /// Sitenin verdiği kullanıcı kimliği (`user.id`).
    pub user_handle: String,
    pub credential_id: String,
    /// P-256 özel anahtarı (32 bayt).
    pub key: String,
    #[zeroize(skip)]
    pub counter: u32,
    #[zeroize(skip)]
    pub created: u64,
}

impl std::fmt::Debug for Passkey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Passkey({}, {})", self.rp_id, self.user_name)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Random,
    BadKey,
    BadInput,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Error::Random => carlui_core::t!("Rastgele sayı üretilemedi.", "Couldn't generate random bytes."),
            Error::BadKey => carlui_core::t!("Passkey anahtarı bozuk.", "The passkey key is damaged."),
            Error::BadInput => carlui_core::t!("Sitenin passkey isteği geçersiz.", "The site's passkey request is invalid."),
        })
    }
}

// --- base64url ----------------------------------------------------------

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

pub fn b64(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..=c.len() {
            s.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    s
}

/// base64url ya da standart base64; dolgu ve boşluk yok sayılır.
pub fn unb64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for ch in s.bytes() {
        let v = match ch {
            b'A'..=b'Z' => ch - b'A',
            b'a'..=b'z' => ch - b'a' + 26,
            b'0'..=b'9' => ch - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            b'=' | b' ' | b'\n' | b'\r' => continue,
            _ => return None,
        };
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

// --- Köken ve rpId ----------------------------------------------------------

/// "com.tr", "co.uk" gibi kayıt edilemeyen uzantılar (kısa liste; tam Genel
/// Sonek Listesi yok). rpId bunlardan biri olamaz.
fn public_suffix(rp: &str) -> bool {
    let l: Vec<&str> = rp.split('.').collect();
    l.len() == 1 || (l.len() == 2 && l[1].len() == 2 && ["com", "co", "org", "net", "gov", "edu", "ac", "gen", "web", "bel", "k12", "ne", "or"].contains(&l[0]))
}

/// Sekme adresinin kökeni ("https://webauthn.io", bağlantı noktası dahil) ve
/// makinesi. Passkey yalnızca https'te (ya da http://localhost) çalışır.
pub fn origin_of(url: &str) -> Option<(String, String)> {
    let origin = crate::autofill::origin(url)?;
    let (scheme, auth) = origin.split_once("://")?;
    let host = match auth.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => h,
        _ => auth,
    }
    .trim_end_matches('.')
    .to_string();
    let local = host == "localhost" || host.ends_with(".localhost");
    (scheme == "https" || local).then_some((origin, host))
}

/// Site bu rpId'yi isteyebilir mi: rpId sekmenin makinesinin kendisi ya da
/// onun kayıt edilebilir bir üst alanı olmalı (WebAuthn §5.1.3 adım 8).
pub fn rp_allowed(rp_id: &str, url: &str) -> bool {
    let Some((_, host)) = origin_of(url) else { return false };
    let rp = rp_id.trim().to_ascii_lowercase();
    if rp.is_empty() || rp.parse::<std::net::IpAddr>().is_ok() || host.parse::<std::net::IpAddr>().is_ok() {
        return false;
    }
    if rp == "localhost" {
        return host == "localhost";
    }
    !public_suffix(&rp) && (host == rp || host.ends_with(&format!(".{rp}")))
}

// --- Anahtar ve kodlama -------------------------------------------------

fn random<const N: usize>() -> Result<[u8; N], Error> {
    let mut b = [0u8; N];
    getrandom::getrandom(&mut b).map_err(|_| Error::Random)?;
    Ok(b)
}

impl Passkey {
    /// Yeni anahtar çifti ve 16 baytlık rastgele kimlik.
    pub fn generate(rp_id: &str, rp_name: &str, user_name: &str, user_display: &str, user_handle: &str, created: u64) -> Result<Self, Error> {
        let key = loop {
            let raw = Zeroizing::new(random::<32>()?);
            if let Ok(k) = SigningKey::from_slice(raw.as_ref()) {
                break k;
            }
        };
        let id = random::<16>()?;
        Ok(Self {
            rp_id: rp_id.to_ascii_lowercase(),
            rp_name: rp_name.into(),
            user_name: user_name.into(),
            user_display: user_display.into(),
            user_handle: user_handle.into(),
            credential_id: b64(&id),
            key: b64(&Zeroizing::new(key.to_bytes().to_vec())),
            counter: 0,
            created,
        })
    }

    fn signing_key(&self) -> Result<SigningKey, Error> {
        let raw = Zeroizing::new(unb64(&self.key).ok_or(Error::BadKey)?);
        SigningKey::from_slice(&raw).map_err(|_| Error::BadKey)
    }

    /// Açık anahtarın (x, y) koordinatları.
    fn point(&self) -> Result<([u8; 32], [u8; 32]), Error> {
        let p = self.signing_key()?.verifying_key().to_encoded_point(false);
        let (Some(x), Some(y)) = (p.x(), p.y()) else { return Err(Error::BadKey) };
        Ok(((*x).into(), (*y).into()))
    }
}

/// COSE_Key (RFC 9053): {1: 2 (EC2), 3: -7 (ES256), -1: 1 (P-256), -2: x, -3: y}.
pub fn cose_key(x: &[u8; 32], y: &[u8; 32]) -> Vec<u8> {
    let mut v = vec![0xA5, 0x01, 0x02, 0x03, 0x26, 0x20, 0x01, 0x21, 0x58, 0x20];
    v.extend_from_slice(x);
    v.extend_from_slice(&[0x22, 0x58, 0x20]);
    v.extend_from_slice(y);
    v
}

/// SubjectPublicKeyInfo (DER): `getPublicKey()` bunu döndürür.
pub fn spki(x: &[u8; 32], y: &[u8; 32]) -> Vec<u8> {
    const PREFIX: [u8; 27] = [0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01, 0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00, 0x04];
    let mut v = PREFIX.to_vec();
    v.extend_from_slice(x);
    v.extend_from_slice(y);
    v
}

fn cbor_head(major: u8, n: usize, out: &mut Vec<u8>) {
    let m = major << 5;
    match n {
        0..=23 => out.push(m | n as u8),
        24..=0xFF => out.extend_from_slice(&[m | 24, n as u8]),
        _ => {
            out.push(m | 25);
            out.extend_from_slice(&(n as u16).to_be_bytes());
        }
    }
}

fn cbor_text(s: &str, out: &mut Vec<u8>) {
    cbor_head(3, s.len(), out);
    out.extend_from_slice(s.as_bytes());
}

/// attestationObject: {"fmt": "none", "attStmt": {}, "authData": bytes}.
pub fn attestation_object(auth_data: &[u8]) -> Vec<u8> {
    let mut v = vec![0xA3];
    cbor_text("fmt", &mut v);
    cbor_text("none", &mut v);
    cbor_text("attStmt", &mut v);
    v.push(0xA0);
    cbor_text("authData", &mut v);
    cbor_head(2, auth_data.len(), &mut v);
    v.extend_from_slice(auth_data);
    v
}

/// authenticatorData: rpIdHash ‖ bayraklar ‖ sayaç (BE) ‖ [AAGUID ‖ kimlik uzunluğu ‖ kimlik ‖ COSE anahtarı].
pub fn auth_data(rp_id: &str, flags: u8, counter: u32, attested: Option<(&[u8], &[u8])>) -> Vec<u8> {
    let mut v = Sha256::digest(rp_id.as_bytes()).to_vec();
    v.push(flags | if attested.is_some() { FLAG_AT } else { 0 });
    v.extend_from_slice(&counter.to_be_bytes());
    if let Some((id, cose)) = attested {
        v.extend_from_slice(&[0u8; 16]);
        v.extend_from_slice(&(id.len() as u16).to_be_bytes());
        v.extend_from_slice(id);
        v.extend_from_slice(cose);
    }
    v
}

/// clientDataJSON (L3 §5.8.1.1 sırasıyla). `challenge` base64url olarak gelir;
/// çözülüp yeniden dolgusuz base64url yazılır.
pub fn client_data(kind: &str, challenge: &str, origin: &str) -> Result<String, Error> {
    let c = unb64(challenge).filter(|c| c.len() >= 16 && c.len() <= 1024).ok_or(Error::BadInput)?;
    let q = |s: &str| serde_json::to_string(s).unwrap_or_default();
    Ok(format!("{{\"type\":{},\"challenge\":{},\"origin\":{},\"crossOrigin\":false}}", q(kind), q(&b64(&c)), q(origin)))
}

/// `navigator.credentials.create` sonucu (hepsi base64url).
#[derive(Debug, Clone, Default)]
pub struct Registration {
    pub credential_id: String,
    pub client_data: String,
    pub authenticator_data: String,
    pub attestation_object: String,
    pub public_key: String,
}

/// `navigator.credentials.get` sonucu (hepsi base64url).
#[derive(Debug, Clone, Default)]
pub struct Assertion {
    pub credential_id: String,
    pub client_data: String,
    pub authenticator_data: String,
    pub signature: String,
    pub user_handle: String,
}

/// Yeni passkey'in kayıt yanıtı.
pub fn register(pk: &Passkey, origin: &str, challenge: &str) -> Result<Registration, Error> {
    let cd = client_data("webauthn.create", challenge, origin)?;
    let (x, y) = pk.point()?;
    let id = unb64(&pk.credential_id).ok_or(Error::BadKey)?;
    let ad = auth_data(&pk.rp_id, FLAGS, pk.counter, Some((&id, &cose_key(&x, &y))));
    Ok(Registration {
        credential_id: pk.credential_id.clone(),
        client_data: b64(cd.as_bytes()),
        authenticator_data: b64(&ad),
        attestation_object: b64(&attestation_object(&ad)),
        public_key: b64(&spki(&x, &y)),
    })
}

/// Girişte imza: authenticatorData ‖ SHA-256(clientDataJSON), DER ECDSA.
/// Sayaç bir artar (kayıt güncellenmeli).
pub fn assert(pk: &mut Passkey, origin: &str, challenge: &str) -> Result<Assertion, Error> {
    let cd = client_data("webauthn.get", challenge, origin)?;
    let key = pk.signing_key()?;
    pk.counter = pk.counter.wrapping_add(1);
    let ad = auth_data(&pk.rp_id, FLAGS, pk.counter, None);
    let mut msg = ad.clone();
    msg.extend_from_slice(&Sha256::digest(cd.as_bytes()));
    let sig: Signature = key.sign(&msg);
    Ok(Assertion {
        credential_id: pk.credential_id.clone(),
        client_data: b64(cd.as_bytes()),
        authenticator_data: b64(&ad),
        signature: b64(sig.to_der().as_bytes()),
        user_handle: pk.user_handle.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::{signature::Verifier, VerifyingKey};

    const CH: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";

    #[test]
    fn base64url() {
        for n in 0..40 {
            let d: Vec<u8> = (0..n as u8).map(|i| i.wrapping_mul(37)).collect();
            assert_eq!(unb64(&b64(&d)).unwrap(), d);
        }
        assert_eq!(b64(b"\xfb\xff"), "-_8");
        assert_eq!(unb64("+/8=").unwrap(), b"\xfb\xff");
        assert_eq!(unb64("a*b"), None);
    }

    #[test]
    fn rp_rules() {
        assert!(rp_allowed("webauthn.io", "https://webauthn.io/"));
        assert!(rp_allowed("example.com", "https://login.example.com/x"));
        assert!(rp_allowed("login.example.com", "https://login.example.com:8443/x"));
        assert!(!rp_allowed("login.example.com", "https://example.com/"), "alt alan üst alana ait değil");
        assert!(!rp_allowed("example.com", "https://example.com.evil.io/"));
        assert!(!rp_allowed("example.com", "https://evilexample.com/"));
        assert!(!rp_allowed("example.com", "http://example.com/"), "https gerekir");
        assert!(!rp_allowed("com", "https://example.com/"), "üst düzey alan");
        assert!(!rp_allowed("com.tr", "https://site.com.tr/"), "kayıt edilemeyen uzantı");
        assert!(rp_allowed("site.com.tr", "https://www.site.com.tr/"));
        assert!(rp_allowed("localhost", "http://localhost:3000/"));
        assert!(!rp_allowed("127.0.0.1", "https://127.0.0.1/"));
        assert!(!rp_allowed("example.com", "file:///C:/example.com/x.html"));
        assert_eq!(origin_of("https://WebAuthn.io/register?x=1").unwrap().0, "https://webauthn.io");
        assert_eq!(origin_of("http://localhost:3000/a").unwrap(), ("http://localhost:3000".into(), "localhost".into()));
    }

    #[test]
    fn client_data_format() {
        let cd = client_data("webauthn.create", CH, "https://webauthn.io").unwrap();
        assert_eq!(cd, format!("{{\"type\":\"webauthn.create\",\"challenge\":\"{CH}\",\"origin\":\"https://webauthn.io\",\"crossOrigin\":false}}"));
        // Standart base64 ve dolgu da kabul edilir, dolgusuz base64url yazılır.
        let padded = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";
        assert!(client_data("webauthn.get", padded, "https://a.io").unwrap().contains(CH));
        assert_eq!(client_data("webauthn.get", "AAEC", "https://a.io"), Err(Error::BadInput), "çok kısa meydan okuma");
    }

    #[test]
    fn registration_layout() {
        let pk = Passkey::generate("webauthn.io", "WebAuthn.io", "carl", "Carl", "dXNlcg", 1).unwrap();
        let r = register(&pk, "https://webauthn.io", CH).unwrap();
        let ad = unb64(&r.authenticator_data).unwrap();
        assert_eq!(&ad[..32], Sha256::digest(b"webauthn.io").as_slice(), "rpIdHash");
        assert_eq!(ad[32], FLAG_UP | FLAG_UV | FLAG_BE | FLAG_BS | FLAG_AT);
        assert_eq!(&ad[33..37], &[0, 0, 0, 0], "sayaç");
        assert_eq!(&ad[37..53], &[0u8; 16], "AAGUID");
        let id = unb64(&pk.credential_id).unwrap();
        assert_eq!(id.len(), 16);
        assert_eq!(&ad[53..55], &[0, 16]);
        assert_eq!(&ad[55..71], id.as_slice());
        let cose = &ad[71..];
        assert_eq!(cose.len(), 77);
        assert_eq!(&cose[..10], &[0xA5, 0x01, 0x02, 0x03, 0x26, 0x20, 0x01, 0x21, 0x58, 0x20]);
        // COSE ve SPKI aynı noktayı taşır.
        let spki = unb64(&r.public_key).unwrap();
        assert_eq!(spki.len(), 91);
        assert_eq!(&spki[27..59], &cose[10..42]);
        assert_eq!(&spki[59..], &cose[45..]);
        assert!(VerifyingKey::from_sec1_bytes(&spki[26..]).is_ok());
        // attestationObject: {"fmt":"none","attStmt":{},"authData":h'…'}
        let ao = unb64(&r.attestation_object).unwrap();
        let mut head = vec![0xA3, 0x63];
        head.extend_from_slice(b"fmt");
        head.push(0x64);
        head.extend_from_slice(b"none");
        head.push(0x67);
        head.extend_from_slice(b"attStmt");
        head.extend_from_slice(&[0xA0, 0x68]);
        head.extend_from_slice(b"authData");
        head.extend_from_slice(&[0x58, ad.len() as u8]);
        assert_eq!(&ao[..head.len()], head.as_slice());
        assert_eq!(&ao[head.len()..], ad.as_slice());
        let cd = String::from_utf8(unb64(&r.client_data).unwrap()).unwrap();
        assert!(cd.starts_with("{\"type\":\"webauthn.create\""));
    }

    #[test]
    fn assertion_signature_verifies() {
        let mut pk = Passkey::generate("webauthn.io", "", "carl", "", "dXNlcg", 1).unwrap();
        let reg = register(&pk, "https://webauthn.io", CH).unwrap();
        let spki = unb64(&reg.public_key).unwrap();
        let vk = VerifyingKey::from_sec1_bytes(&spki[26..]).unwrap();
        for n in 1..=2u32 {
            let a = assert(&mut pk, "https://webauthn.io", CH).unwrap();
            let ad = unb64(&a.authenticator_data).unwrap();
            assert_eq!(ad.len(), 37, "AT yok, uzantı yok");
            assert_eq!(ad[32], FLAG_UP | FLAG_UV | FLAG_BE | FLAG_BS);
            assert_eq!(u32::from_be_bytes(ad[33..37].try_into().unwrap()), n, "sayaç artar");
            let cd = unb64(&a.client_data).unwrap();
            assert!(String::from_utf8(cd.clone()).unwrap().starts_with("{\"type\":\"webauthn.get\""));
            let mut msg = ad.clone();
            msg.extend_from_slice(&Sha256::digest(&cd));
            let sig = Signature::from_der(&unb64(&a.signature).unwrap()).unwrap();
            vk.verify(&msg, &sig).expect("imza doğrulanmalı");
            let mut bad = msg.clone();
            bad[0] ^= 1;
            assert!(vk.verify(&bad, &sig).is_err());
            assert_eq!(a.user_handle, "dXNlcg");
            assert_eq!(a.credential_id, reg.credential_id);
        }
    }

    #[test]
    fn body_keeps_passkeys() {
        use crate::entry::Body;
        let old: Body = serde_json::from_str(r#"{"fields":[],"file":null}"#).unwrap();
        assert!(old.passkeys.is_empty(), "passkey'siz eski kayıt okunur");
        assert!(!serde_json::to_string(&old).unwrap().contains("passkeys"), "boşsa yazılmaz");
        let mut b = old.clone();
        b.passkeys.push(Passkey::generate("a.io", "A", "u", "", "dQ", 7).unwrap());
        let back: Body = serde_json::from_str(&serde_json::to_string(&b).unwrap()).unwrap();
        assert!(back.passkeys == b.passkeys);
        assert!(!format!("{:?}", back.passkeys[0]).contains(&b.passkeys[0].key), "Debug anahtarı yazmaz");
    }

    #[test]
    fn broken_key_is_rejected() {
        let mut pk = Passkey::generate("a.io", "", "u", "", "dQ", 1).unwrap();
        pk.key = b64(&[0u8; 32]);
        assert_eq!(assert(&mut pk, "https://a.io", CH).unwrap_err(), Error::BadKey);
    }
}
