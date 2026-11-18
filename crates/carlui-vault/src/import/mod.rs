//! İçe aktarma. Sorun: başka bir şifre yöneticisinden ya da 2FA uygulamasından
//! geçerken yüzlerce kaydı elle yazmak gerekmesin. Çözüm: o uygulamanın
//! şifresiz dışa aktarım dosyası okunur, biçimi kendiliğinden tanınır ve
//! kayıtlar önizlenip tek seferde kasaya yazılır.
//!
//! Tanınanlar:
//! - CSV (`csv`): Bitwarden, Chrome / Edge / Brave / Opera / Vivaldi / Helium,
//!   Firefox, 1Password, LastPass, KeePass / KeePassXC, Proton Pass, Dashlane,
//!   NordPass ve sütun adı tanınan diğerleri. 2FA sütunu varsa kayda eklenir.
//! - JSON (`json`): Bitwarden (şifresiz; giriş, not, kart, kimlik), Aegis
//!   (şifresiz), 2FAS, andOTP.
//! - Düz metin: satır satır `otpauth://totp/…` (Ente Auth, Bitwarden
//!   Authenticator, Raivo …) ve Google Authenticator `otpauth-migration://`
//!   adresleri (`gauth`).
//!
//! Şifreli dışa aktarımlar okunamaz; bu durumda kullanıcıya şifresiz dışa
//! aktarması söylenir. Hiçbir şey ağa gitmez; dosya yalnızca okunur.

mod csv;
pub mod gauth;
mod json;

use crate::entry::{now, Body, Category, Field, FieldKind, Format, Meta};
use zeroize::Zeroizing;

/// Okunacak en büyük dosya.
pub const MAX_FILE: usize = 20 * 1024 * 1024;

/// İçe aktarılacak bir kayıt.
pub struct Imported {
    pub meta: Meta,
    pub body: Body,
}

/// Bir dosyadan çıkan kayıtlar.
pub struct Batch {
    /// Tanınan kaynak ("Bitwarden (CSV)" …).
    pub source: String,
    pub items: Vec<Imported>,
    /// Okunamayan ya da desteklenmeyen satırlar (HOTP, Steam …).
    pub skipped: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ImportError {
    Empty,
    TooBig,
    /// Dosya şifreli; kaynak uygulamanın adı.
    Encrypted(&'static str),
    Unknown,
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use carlui_core::t;
        match self {
            ImportError::Empty => f.write_str(t!("Dosyada içe aktarılacak kayıt bulunamadı.", "No items to import were found in the file.")),
            ImportError::TooBig => f.write_str(t!("Dosya çok büyük (en çok 20 MB).", "The file is too large (20 MB at most).")),
            ImportError::Encrypted(app) => write!(
                f,
                "{}",
                t!(
                    format!("Bu {app} dışa aktarımı şifreli. {app} içinden şifresiz (düz) dışa aktarıp onu seç."),
                    format!("This {app} export is encrypted. Export it unencrypted (plain) from {app} and pick that file.")
                )
            ),
            ImportError::Unknown => f.write_str(t!(
                "Dosya biçimi tanınmadı. CSV, JSON ya da otpauth:// satırları içeren bir metin dosyası seç.",
                "The file format wasn't recognised. Pick a CSV, JSON or a text file with otpauth:// lines."
            )),
        }
    }
}

/// Dosyanın içeriğini okur ve biçimini tanır.
pub fn parse(data: &[u8]) -> Result<Batch, ImportError> {
    if data.len() > MAX_FILE {
        return Err(ImportError::TooBig);
    }
    let text = Zeroizing::new(String::from_utf8_lossy(data).into_owned());
    let t = text.trim_start_matches('\u{FEFF}').trim();
    if t.is_empty() {
        return Err(ImportError::Empty);
    }
    let batch = if t.starts_with('{') || t.starts_with('[') {
        json::parse(t)?
    } else if t.lines().any(|l| starts_ci(l.trim(), "otpauth://") || starts_ci(l.trim(), "otpauth-migration://")) {
        uri_lines(t)
    } else {
        csv::parse(t)?
    };
    if batch.items.is_empty() {
        return Err(ImportError::Empty);
    }
    Ok(batch)
}

fn starts_ci(s: &str, p: &str) -> bool {
    s.get(..p.len()).is_some_and(|h| h.eq_ignore_ascii_case(p))
}

/// Satır satır otpauth / otpauth-migration adresleri.
fn uri_lines(t: &str) -> Batch {
    let mut b = Batch { source: t_("2FA adresleri", "2FA addresses"), items: Vec::new(), skipped: 0 };
    for l in t.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if starts_ci(l, "otpauth-migration://") {
            match gauth::parse(l) {
                Some(g) => {
                    b.source = "Google Authenticator".into();
                    b.items.extend(g.items);
                    b.skipped += g.skipped;
                }
                None => b.skipped += 1,
            }
        } else if starts_ci(l, "otpauth://") {
            match crate::totp::Totp::parse(l) {
                Ok(_) => {
                    let (issuer, account) = crate::totp::names(l);
                    b.items.push(two_factor(&issuer, &account, l));
                }
                Err(_) => b.skipped += 1,
            }
        }
    }
    b
}

fn t_(tr: &str, en: &str) -> String {
    carlui_core::t!(tr, en).to_string()
}

/// Adresin makinesi ("https://www.github.com/login" → "github.com").
pub fn host(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let h = rest.split(['/', '?', '#']).next().unwrap_or("").rsplit('@').next().unwrap_or("");
    let h = h.split(':').next().unwrap_or("").to_ascii_lowercase();
    h.strip_prefix("www.").unwrap_or(&h).to_string()
}

/// Ad yoksa adresin makinesi, o da yoksa kullanıcı adı.
fn name_or(name: &str, url: &str, user: &str) -> String {
    let n = name.trim();
    if !n.is_empty() {
        return n.to_string();
    }
    let h = host(url);
    if !h.is_empty() {
        return h;
    }
    if !user.trim().is_empty() {
        return user.trim().to_string();
    }
    t_("İçe aktarılan kayıt", "Imported item")
}

fn meta(name: String, category: Category) -> Meta {
    Meta { name, category, updated: now() }
}

fn field(label: &str, value: &str, kind: FieldKind) -> Field {
    Field { label: label.into(), value: value.into(), kind, fmt: Format::Free }
}

/// Giriş kaydı: kullanıcı adı, parola, adres, (varsa) 2FA ve not.
pub struct Login<'a> {
    pub name: &'a str,
    pub url: &'a str,
    pub user: &'a str,
    pub password: &'a str,
    pub totp: &'a str,
    pub notes: &'a str,
}

pub fn login(l: Login) -> Imported {
    use carlui_core::t;
    // Gmail, Outlook gibi sağlayıcıların girişi e-posta hesabıdır.
    let mail = crate::autofill::is_mail_site(l.url);
    let user_label = if mail { t!("E-posta adresi", "Email address") } else { t!("Kullanıcı adı", "Username") };
    let mut fields = vec![
        field(user_label, l.user.trim(), FieldKind::Text),
        field(t!("Parola", "Password"), l.password, FieldKind::Secret),
        field(t!("Adres", "Website"), l.url.trim(), FieldKind::Url),
    ];
    if !l.totp.trim().is_empty() {
        fields.push(field(t!("2FA anahtarı", "2FA key"), l.totp.trim(), FieldKind::Totp));
    }
    fields.push(field(t!("Not", "Note"), l.notes.trim(), FieldKind::Multiline));
    let category = if mail { Category::Email } else { Category::Password };
    Imported { meta: meta(name_or(l.name, l.url, l.user), category), body: Body { fields, file: None, passkeys: Vec::new() } }
}

/// Güvenli not.
pub fn note(name: &str, text: &str) -> Imported {
    let fields = vec![field(carlui_core::t!("Not", "Note"), text.trim(), FieldKind::Multiline)];
    Imported { meta: meta(name_or(name, "", ""), Category::Note), body: Body { fields, file: None, passkeys: Vec::new() } }
}

/// 2FA kaydı: anahtar alanına otpauth adresinin tamamı (hane/süre/algoritma korunur).
pub fn two_factor(issuer: &str, account: &str, uri: &str) -> Imported {
    use carlui_core::t;
    let name = if issuer.trim().is_empty() { name_or(account, "", "") } else { issuer.trim().to_string() };
    let fields = vec![
        field(t!("Hesap", "Account"), account.trim(), FieldKind::Text),
        field(t!("Gizli anahtar", "Secret key"), uri.trim(), FieldKind::Totp),
        field(t!("Yedek kodlar", "Backup codes"), "", FieldKind::Multiline),
    ];
    Imported { meta: meta(name, Category::TwoFactor), body: Body { fields, file: None, passkeys: Vec::new() } }
}

/// Banka kartı (alanlar kasanın kart şablonuyla aynı sırada ve biçimde).
pub struct Card<'a> {
    pub name: &'a str,
    pub holder: &'a str,
    pub number: &'a str,
    pub month: &'a str,
    pub year: &'a str,
    pub cvv: &'a str,
    pub notes: &'a str,
}

pub fn card(c: Card) -> Imported {
    use crate::card::{digits, expiry};
    let mut body = Body { fields: Category::Card.template(), file: None, passkeys: Vec::new() };
    let exp = expiry(&format!("{}/{}", c.month.trim(), c.year.trim())).map(|(m, y)| format!("{m:02}/{:02}", y % 100)).unwrap_or_default();
    for f in body.fields.iter_mut() {
        f.value = match (f.fmt, f.kind) {
            (Format::Holder, _) => c.holder.trim().to_string(),
            (Format::CardNumber, _) => digits(c.number),
            (Format::Expiry, _) => exp.clone(),
            (Format::Cvv, _) => digits(c.cvv),
            (_, FieldKind::Multiline) => c.notes.trim().to_string(),
            _ => String::new(),
        };
    }
    let num = digits(c.number);
    let fallback = match (crate::card::brand(&num), num.len() >= 4) {
        (Some(b), true) => format!("{b} •{}", &num[num.len() - 4..]),
        _ => t_("Banka kartı", "Bank card"),
    };
    let name = if c.name.trim().is_empty() { fallback } else { c.name.trim().to_string() };
    Imported { meta: meta(name, Category::Card), body }
}

/// Kimlik kaydı.
pub fn identity(name: &str, full: &str, id_no: &str, notes: &str) -> Imported {
    // Şablon: ad soyad, kimlik no, doğum tarihi, not.
    let mut body = Body { fields: Category::Identity.template(), file: None, passkeys: Vec::new() };
    for (i, v) in [full, id_no, "", notes].into_iter().enumerate() {
        if let Some(f) = body.fields.get_mut(i) {
            f.value = v.trim().to_string();
        }
    }
    Imported { meta: meta(name_or(name, "", full), Category::Identity), body }
}

/// RFC 4648 base32 (dolgusuz).
pub fn base32(b: &[u8]) -> String {
    const A: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let (mut out, mut acc, mut bits) = (String::with_capacity(b.len() * 8 / 5 + 1), 0u32, 0u32);
    for &x in b {
        acc = (acc << 8) | x as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(A[((acc >> bits) & 31) as usize] as char);
        }
        acc &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(A[((acc << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// Adres parçası için %XX kodlama.
fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

/// otpauth adresi kurar (düz anahtarı olan uygulamalar için).
pub fn otpauth(issuer: &str, account: &str, secret_b32: &str, digits: u32, period: u64, algo: &str) -> String {
    let secret: String = secret_b32.chars().filter(|c| !c.is_whitespace() && *c != '=').collect::<String>().to_ascii_uppercase();
    let label = match (issuer.trim(), account.trim()) {
        ("", a) => esc(a),
        (i, "") => esc(i),
        (i, a) => format!("{}:{}", esc(i), esc(a)),
    };
    let mut uri = format!("otpauth://totp/{label}?secret={secret}");
    if !issuer.trim().is_empty() {
        uri.push_str(&format!("&issuer={}", esc(issuer.trim())));
    }
    let algo = algo.trim().to_ascii_uppercase().replace('-', "");
    if !algo.is_empty() && algo != "SHA1" {
        uri.push_str(&format!("&algorithm={algo}"));
    }
    if digits != 6 && digits != 0 {
        uri.push_str(&format!("&digits={digits}"));
    }
    if period != 30 && period != 0 {
        uri.push_str(&format!("&period={period}"));
    }
    uri
}

/// Aynı kaydı ikinci kez eklememek için anahtar: kategori + ad + düz alanlar.
pub fn key(category: Category, name: &str, body: &Body) -> Zeroizing<String> {
    let mut k = Zeroizing::new(format!("{category:?}\u{1f}{}", name.trim().to_lowercase()));
    for f in body.fields.iter().filter(|f| f.kind != FieldKind::Multiline && !f.value.trim().is_empty()) {
        k.push('\u{1f}');
        k.push_str(f.value.trim());
    }
    k
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mail_logins_go_to_email() {
        let csv = "name,url,username,password,note
accounts.google.com,https://accounts.google.com/,carl@gmail.com,p1,
github.com,https://github.com/login,carl,p2,
login.live.com,https://login.live.com/,carl@outlook.com,p3,
";
        let b = parse(csv.as_bytes()).unwrap();
        let cats: Vec<_> = b.items.iter().map(|i| i.meta.category).collect();
        assert_eq!(cats, [Category::Email, Category::Password, Category::Email]);
        assert_eq!(b.items[0].body.fields[0].label, carlui_core::t!("E-posta adresi", "Email address"));
    }

    #[test]
    fn otpauth_roundtrip() {
        let u = otpauth("Git Hub", "carl@ornek.com", "jbsw y3dp ehpk 3pxp", 6, 30, "SHA1");
        assert_eq!(u, "otpauth://totp/Git%20Hub:carl%40ornek.com?secret=JBSWY3DPEHPK3PXP&issuer=Git%20Hub");
        assert_eq!(crate::totp::names(&u), ("Git Hub".into(), "carl@ornek.com".into()));
        let t = crate::totp::Totp::parse(&otpauth("X", "", "JBSWY3DPEHPK3PXP", 8, 60, "SHA256")).unwrap();
        assert_eq!((t.digits, t.period, t.algo), (8, 60, crate::totp::Algo::Sha256));
        assert_eq!(base32(&[0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x21, 0xde, 0xad, 0xbe, 0xef]), "JBSWY3DPEHPK3PXP");
    }

    #[test]
    fn uri_list() {
        let b = parse(b"otpauth://totp/GitHub:carl?secret=JBSWY3DPEHPK3PXP&issuer=GitHub\n\notpauth://hotp/X?secret=JBSWY3DPEHPK3PXP\n").unwrap();
        assert_eq!(b.items.len(), 1);
        assert_eq!(b.skipped, 1, "HOTP atlanır");
        assert_eq!(b.items[0].meta.name, "GitHub");
        assert_eq!(b.items[0].meta.category, Category::TwoFactor);
    }

    #[test]
    fn errors() {
        assert_eq!(parse(b"").err(), Some(ImportError::Empty));
        assert_eq!(parse(b"merhaba\ndunya").err(), Some(ImportError::Unknown));
    }

    #[test]
    fn cards_and_hosts() {
        let c = card(Card { name: "", holder: "CARL", number: "4111-1111-1111-1111", month: "4", year: "2027", cvv: "123", notes: "" });
        assert_eq!(c.meta.name, "Visa •1111");
        assert_eq!(crate::card::Part::MmYyyy.value(&c.body).unwrap().as_str(), "04/2027");
        assert_eq!(host("https://www.GitHub.com:443/login?x"), "github.com");
        assert_eq!(host("github.com"), "github.com");
    }
}
