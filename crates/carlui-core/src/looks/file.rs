//! Paylaşılan görünüm dosyası (`.carllook`): insan okuyabilen, sürümlü küçük
//! bir TOML. İçinde yalnızca görünüm değerleri vardır; yol, komut ya da
//! uygulama listesi taşımaz. İçe aktarma güvenli olsun diye:
//! - dosya en çok `MAX_BYTES` olabilir,
//! - türü yanlış ya da bozuk alan dosyayı reddeder (tanınmayan alanlar yok sayılır),
//! - sayılar aralığa çekilir, renkler doğrulanır, ad temizlenip kısaltılır.
//!
//! Örnek:
//! ```toml
//! format = "carllook"
//! version = 1
//! name = "Gece"
//!
//! [theme]
//! mode = "dark"
//! palette = "graphite"
//! ...
//! ```

use super::{sanitize, Look, LookCursor, LookDock, LookIsland, LookTheme};
use serde::{Deserialize, Serialize};
use std::{io::Read, path::Path};

/// Dosya uzantısı (noktasız).
pub const EXT: &str = "carllook";
/// Biçim adı; başka TOML dosyaları yanlışlıkla görünüm sanılmasın.
pub const FORMAT: &str = "carllook";
/// Yazılan ve okunabilen en yeni sürüm.
pub const VERSION: u32 = 1;
/// Görünüm dosyası bundan büyük olamaz (gerçek bir dosya ~1 KB).
pub const MAX_BYTES: usize = 16 * 1024;

#[derive(Serialize, Deserialize)]
struct FileV1 {
    format: String,
    version: u32,
    #[serde(default)]
    name: String,
    theme: LookTheme,
    dock: LookDock,
    island: LookIsland,
    cursor: LookCursor,
}

/// İçe aktarma neden olmadı.
#[derive(Debug, PartialEq)]
pub enum ImportError {
    TooLarge,
    Unreadable,
    /// TOML değil, eksik bölüm ya da yanlış türde değer.
    Corrupt,
    /// Başka bir dosya türü (`format` alanı yok ya da farklı).
    NotALook,
    /// Daha yeni bir CarlUI'nin yazdığı dosya.
    Newer(u32),
}

impl ImportError {
    pub fn message(&self) -> String {
        match self {
            ImportError::TooLarge => t!("Dosya bir görünüm için fazla büyük.", "The file is too large to be a look.").into(),
            ImportError::Unreadable => t!("Dosya okunamadı.", "The file couldn't be read.").into(),
            ImportError::Corrupt => t!("Dosya bozuk ya da eksik.", "The file is damaged or incomplete.").into(),
            ImportError::NotALook => t!("Bu bir CarlUI görünüm dosyası değil.", "This isn't a CarlUI look file.").into(),
            ImportError::Newer(v) => t!(format!("Dosya daha yeni bir CarlUI'den (sürüm {v})."), format!("The file is from a newer CarlUI (version {v}).")),
        }
    }
}

/// Görünümü dosya metnine çevirir (değerler önce aralığa çekilir).
pub fn to_text(look: &Look) -> String {
    let l = sanitize::sanitized(look);
    let f = FileV1 { format: FORMAT.into(), version: VERSION, name: l.name, theme: l.theme, dock: l.dock, island: l.island, cursor: l.cursor };
    let body = toml::to_string_pretty(&f).unwrap_or_default();
    format!("# CarlUI görünümü / CarlUI look\n{body}")
}

/// Dosya metninden görünüm okur; değerler aralığa çekilmiş döner.
pub fn from_text(text: &str) -> Result<Look, ImportError> {
    if text.len() > MAX_BYTES {
        return Err(ImportError::TooLarge);
    }
    // Önce yalnızca başlığa bakılır: başka bir TOML'a "bozuk" yerine doğru hata verilsin.
    #[derive(Deserialize)]
    struct Head {
        format: Option<String>,
        version: Option<u32>,
    }
    let head: Head = toml::from_str(text).map_err(|_| ImportError::Corrupt)?;
    if head.format.as_deref() != Some(FORMAT) {
        return Err(ImportError::NotALook);
    }
    match head.version {
        Some(v) if v > VERSION => return Err(ImportError::Newer(v)),
        Some(v) if v >= 1 => {}
        _ => return Err(ImportError::Corrupt),
    }
    let f: FileV1 = toml::from_str(text).map_err(|_| ImportError::Corrupt)?;
    let look = Look { name: f.name, theme: f.theme, dock: f.dock, island: f.island, cursor: f.cursor };
    let mut look = sanitize::sanitized(&look);
    if look.name.is_empty() {
        look.name = t!("İçe aktarılan", "Imported").into();
    }
    Ok(look)
}

/// Dosyadan okur; boyut okumadan önce ve okurken denetlenir.
pub fn read(path: &Path) -> Result<Look, ImportError> {
    let file = std::fs::File::open(path).map_err(|_| ImportError::Unreadable)?;
    if file.metadata().map(|m| m.len() > MAX_BYTES as u64).unwrap_or(false) {
        return Err(ImportError::TooLarge);
    }
    let mut buf = Vec::new();
    file.take(MAX_BYTES as u64 + 1).read_to_end(&mut buf).map_err(|_| ImportError::Unreadable)?;
    if buf.len() > MAX_BYTES {
        return Err(ImportError::TooLarge);
    }
    let text = String::from_utf8(buf).map_err(|_| ImportError::Corrupt)?;
    from_text(text.trim_start_matches('\u{FEFF}'))
}

/// Görünümü dosyaya yazar.
pub fn write(path: &Path, look: &Look) -> std::io::Result<()> {
    std::fs::write(path, to_text(look))
}

/// Dosya adı önerisi: "Gece.carllook".
pub fn suggested_name(look: &Look) -> String {
    let n = sanitize::clean_name(&look.name);
    let n = if n.is_empty() { "CarlUI".to_string() } else { n };
    format!("{n}.{EXT}")
}
