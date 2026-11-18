//! Kayıt içeriği. Bir kaydın iki ayrı şifreli parçası vardır:
//! - `Meta` (ad, kategori, tarih): kasa açılınca listelenmek için çözülür.
//! - `Body` (alanlar, ek dosya bilgisi): yalnızca o kayıt düzenlenirken çözülür.
//!
//! İkisi de düşerken bellekten silinir (`zeroize`). Kategoriler hazır alan
//! şablonlarıyla gelir; kullanıcı alan ekleyip çıkarabilir.

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Category {
    Password,
    Email,
    Card,
    Identity,
    Note,
    License,
    TwoFactor,
    File,
}

impl Category {
    pub const ALL: [Category; 8] = [Category::Password, Category::Email, Category::Card, Category::Identity, Category::Note, Category::License, Category::TwoFactor, Category::File];

    pub fn title(self) -> &'static str {
        match self {
            Category::Password => carlui_core::t!("Parolalar", "Passwords"),
            Category::Email => carlui_core::t!("E-posta", "Email"),
            Category::Card => carlui_core::t!("Banka kartı", "Bank card"),
            Category::Identity => carlui_core::t!("Kimlik", "Identity"),
            Category::Note => carlui_core::t!("Güvenli not", "Secure note"),
            Category::License => carlui_core::t!("Lisans", "License"),
            Category::TwoFactor => "2FA",
            Category::File => carlui_core::t!("Dosya", "File"),
        }
    }

    pub fn glyph(self) -> char {
        match self {
            Category::Password => '\u{E8D7}',
            Category::Email => '\u{E715}',
            Category::Card => '\u{E8C7}',
            Category::Identity => '\u{E779}',
            Category::Note => '\u{E70B}',
            Category::License => '\u{EB95}',
            Category::TwoFactor => '\u{E928}',
            Category::File => '\u{E8A5}',
        }
    }

    /// Yeni kaydın boş alanları.
    pub fn template(self) -> Vec<Field> {
        use carlui_core::t;
        use FieldKind::*;
        let f = |l: &str, k| Field::new(l, k);
        match self {
            Category::Password => vec![f(t!("Kullanıcı adı", "Username"), Text), f(t!("Parola", "Password"), Secret), f(t!("Adres", "Website"), Url), f(t!("Not", "Note"), Multiline)],
            // Adres alanı tarayıcıda doldurma için (ör. https://accounts.google.com).
            Category::Email => vec![f(t!("E-posta adresi", "Email address"), Text), f(t!("Parola", "Password"), Secret), f(t!("Giriş adresi", "Sign-in address"), Url), f(t!("Not", "Note"), Multiline)],
            // Kart alanları biçimlidir (bkz. `card`): yalnızca rakam, boşluk ve "/"
            // kendiliğinden konur.
            Category::Card => vec![
                f(t!("Kart üzerindeki ad", "Name on card"), Text).with(Format::Holder),
                f(t!("Kart numarası", "Card number"), Secret).with(Format::CardNumber),
                f(t!("Son kullanma (AA/YY)", "Expiry (MM/YY)"), Text).with(Format::Expiry),
                f("CVV", Secret).with(Format::Cvv),
                f("PIN", Secret).with(Format::Pin),
                f(t!("Banka", "Bank"), Text),
                f(t!("Not", "Note"), Multiline),
            ],
            Category::Identity => vec![f(t!("Ad soyad", "Full name"), Text), f(t!("Kimlik no", "ID number"), Secret), f(t!("Doğum tarihi", "Date of birth"), Text), f(t!("Not", "Note"), Multiline)],
            Category::Note => vec![f(t!("Not", "Note"), Multiline)],
            Category::License => vec![f(t!("Ürün", "Product"), Text), f(t!("Lisans anahtarı", "License key"), Secret), f(t!("E-posta", "Email"), Text), f(t!("Not", "Note"), Multiline)],
            // `Totp` alanı anahtarı saklar; düzenleyici kodu ondan üretir (bkz. `totp`).
            Category::TwoFactor => vec![f(t!("Hesap", "Account"), Text), f(t!("Gizli anahtar", "Secret key"), Totp), f(t!("Yedek kodlar", "Backup codes"), Multiline)],
            Category::File => vec![f(t!("Not", "Note"), Multiline)],
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum FieldKind {
    Text,
    /// Varsayılan olarak maskelenir.
    Secret,
    Url,
    Multiline,
    /// 2FA gizli anahtarı (base32 ya da otpauth adresi); kod `totp` ile üretilir.
    Totp,
}

impl FieldKind {
    pub fn masked(self) -> bool {
        matches!(self, FieldKind::Secret | FieldKind::Totp)
    }
}

#[derive(Serialize, Deserialize, Clone, Zeroize, ZeroizeOnDrop)]
pub struct Field {
    pub label: String,
    pub value: String,
    #[zeroize(skip)]
    pub kind: FieldKind,
    /// Yazım biçimi (kart numarası, son kullanma …); eski kayıtlarda yok = serbest.
    #[zeroize(skip)]
    #[serde(default, skip_serializing_if = "Format::is_free")]
    pub fmt: Format,
}

impl Field {
    pub fn new(label: &str, kind: FieldKind) -> Self {
        Self { label: label.into(), value: String::new(), kind, fmt: Format::Free }
    }

    pub fn with(mut self, fmt: Format) -> Self {
        self.fmt = fmt;
        self
    }
}

/// Alanın yazım biçimi. Serbest dışındakilerde yalnızca rakam yazılabilir;
/// gruplama boşlukları ve "/" kendiliğinden konur (bkz. `card`).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum Format {
    #[default]
    Free,
    /// Kart üzerindeki ad (serbest metin; hızlı kopyalamada "Ad" olarak bulunur).
    Holder,
    /// 12–19 rakam, 4'erli (Amex 4-6-5) gruplanır; rakam olarak saklanır.
    CardNumber,
    /// Son kullanma: "AA/YY" olarak saklanır.
    Expiry,
    /// 3–4 rakam.
    Cvv,
    /// 4–6 rakam.
    Pin,
}

impl Format {
    pub fn is_free(&self) -> bool {
        *self == Format::Free
    }

    /// Yalnızca rakam alan biçim mi.
    pub fn numeric(self) -> bool {
        matches!(self, Format::CardNumber | Format::Expiry | Format::Cvv | Format::Pin)
    }
}

/// Liste için çözülen kısım.
#[derive(Serialize, Deserialize, Clone, Zeroize, ZeroizeOnDrop)]
pub struct Meta {
    pub name: String,
    #[zeroize(skip)]
    pub category: Category,
    /// Son değişiklik (Unix saniye).
    #[zeroize(skip)]
    pub updated: u64,
}

/// Ek dosyanın bilgisi; içeriği ayrı bir şifreli dosyadadır (bkz. `Vault::attach`).
#[derive(Serialize, Deserialize, Clone, Zeroize, ZeroizeOnDrop, Default)]
pub struct Attachment {
    pub name: String,
    #[zeroize(skip)]
    pub size: u64,
}

/// Yalnızca düzenlenirken çözülen kısım.
#[derive(Serialize, Deserialize, Clone, Zeroize, ZeroizeOnDrop, Default)]
pub struct Body {
    pub fields: Vec<Field>,
    #[serde(default)]
    pub file: Option<Attachment>,
    /// Bu kayda bağlı passkey'ler (bkz. `passkey`); özel anahtarları gövdeyle şifrelenir.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub passkeys: Vec<crate::passkey::Passkey>,
}

impl std::fmt::Debug for Body {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Body({} alan, {} passkey)", self.fields.len(), self.passkeys.len())
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}
