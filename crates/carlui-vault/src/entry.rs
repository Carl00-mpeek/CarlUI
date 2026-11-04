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
            Category::Email => vec![f(t!("E-posta adresi", "Email address"), Text), f(t!("Parola", "Password"), Secret), f(t!("Sunucu", "Server"), Text), f(t!("Not", "Note"), Multiline)],
            Category::Card => vec![
                f(t!("Kart üzerindeki ad", "Name on card"), Text),
                f(t!("Kart numarası", "Card number"), Secret),
                f(t!("Son kullanma", "Expiry"), Text),
                f("CVV", Secret),
                f("PIN", Secret),
                f(t!("Not", "Note"), Multiline),
            ],
            Category::Identity => vec![f(t!("Ad soyad", "Full name"), Text), f(t!("Kimlik no", "ID number"), Secret), f(t!("Doğum tarihi", "Date of birth"), Text), f(t!("Not", "Note"), Multiline)],
            Category::Note => vec![f(t!("Not", "Note"), Multiline)],
            Category::License => vec![f(t!("Ürün", "Product"), Text), f(t!("Lisans anahtarı", "License key"), Secret), f(t!("E-posta", "Email"), Text), f(t!("Not", "Note"), Multiline)],
            // Aşama 3: `Totp` alanı kod üretecek; şimdilik gizli anahtar saklanır.
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
    /// 2FA gizli anahtarı (base32). Aşama 3'te kod üretimi bu alandan yapılacak.
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
}

impl Field {
    pub fn new(label: &str, kind: FieldKind) -> Self {
        Self { label: label.into(), value: String::new(), kind }
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
}

impl std::fmt::Debug for Body {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Body({} alan)", self.fields.len())
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}
