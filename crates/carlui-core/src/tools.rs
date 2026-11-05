//! Bu modül "küçük araçların (renk seçici, toplu yeniden adlandırma, disk
//! analizörü, metin genişletici) ayarları nerede durur?" sorununu çözüyor:
//! `config.toml`'daki `[tools]` bölümü ve bu araçların `[shortcuts]`
//! kısayollarının doğrulanması. `config.rs` büyümesin diye ayrı dosyadadır.

use crate::config::ShortcutsConfig;
use serde::{Deserialize, Serialize};

/// Renk seçicinin panoya yazdığı biçim.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ColorFormat {
    /// `#1E90FF`
    #[default]
    Hex,
    /// `rgb(30, 144, 255)`
    Rgb,
    /// `hsl(210, 100%, 56%)`
    Hsl,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum RenameMode {
    /// Bul ve değiştir.
    #[default]
    Replace,
    /// Şablon: `{ad}`, `{n}`, `{tarih}`.
    Template,
    /// Büyük/küçük harf.
    Case,
}

/// "Harf" kipinde uygulanan dönüşüm.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum CaseKind {
    #[default]
    Lower,
    Upper,
    /// Her Kelimenin İlk Harfi Büyük.
    Title,
}

/// Toplu yeniden adlandırma penceresinin son kullanılan seçenekleri.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct RenameConfig {
    pub mode: RenameMode,
    pub find: String,
    pub replace: String,
    /// Büyük/küçük harf duyarlı arama.
    pub match_case: bool,
    pub template: String,
    /// `{n}` için başlangıç numarası.
    pub start: u32,
    /// Uzantı (.jpg) değiştirilmez.
    pub keep_ext: bool,
    pub case: CaseKind,
}

impl Default for RenameConfig {
    fn default() -> Self {
        Self { mode: RenameMode::Replace, find: String::new(), replace: String::new(), match_case: false, template: "{ad} {n}".into(), start: 1, keep_ext: true, case: CaseKind::Lower }
    }
}

/// Metin genişletici kısaltması: `;adres` yazınca `text` yazılır.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Snippet {
    pub key: String,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct ToolsConfig {
    pub color_format: ColorFormat,
    /// Toplu yeniden adlandırma modülü açık.
    pub rename_enabled: bool,
    pub rename: RenameConfig,
    /// Disk analizörü modülü açık.
    pub disk_enabled: bool,
    /// Metin genişletici modülü açık.
    pub expander: bool,
    pub snippets: Vec<Snippet>,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            color_format: ColorFormat::Hex,
            rename_enabled: true,
            rename: RenameConfig::default(),
            disk_enabled: true,
            expander: true,
            snippets: vec![
                Snippet { key: ";tarih".into(), text: "{tarih}".into() },
                Snippet { key: ";saat".into(), text: "{saat}".into() },
            ],
        }
    }
}

impl ToolsConfig {
    /// Geçerli kısaltmalar: boş olmayan, en az 2 karakterlik ve boşluksuz anahtarlar.
    pub fn valid_snippets(&self) -> impl Iterator<Item = &Snippet> {
        self.snippets.iter().filter(|s| s.key.chars().count() >= 2 && !s.key.chars().any(char::is_whitespace) && !s.text.is_empty())
    }
}

fn combo(value: &str, what: &str) -> Option<crate::keys::Combo> {
    if value.trim().is_empty() {
        return None;
    }
    crate::keys::parse_combo(value)
        .map_err(|e| crate::warn!("{what} kısayolu '{value}' kullanılamaz: {e}; kapalı sayılıyor"))
        .ok()
}

impl ShortcutsConfig {
    /// Renk seçici kısayolu (boşsa ya da geçersizse None; varsayılanı da boştur).
    pub fn color_picker_combo(&self) -> Option<crate::keys::Combo> {
        combo(&self.color_picker, "renk seçici")
    }

    /// Toplu yeniden adlandırma kısayolu (geçersizse varsayılan Ctrl+Shift+R).
    pub fn rename_combo(&self) -> Option<crate::keys::Combo> {
        if self.rename.trim().is_empty() {
            return None;
        }
        combo(&self.rename, "yeniden adlandırma").or_else(|| crate::keys::parse_combo(&Self::default().rename).ok())
    }

    /// Disk analizörü kısayolu (boşsa None).
    pub fn disk_combo(&self) -> Option<crate::keys::Combo> {
        combo(&self.disk, "disk analizörü")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_combos() {
        let s = ShortcutsConfig::default();
        assert!(s.color_picker_combo().is_none());
        assert!(s.disk_combo().is_none());
        assert!(s.rename_combo().is_some());
        let bad = ShortcutsConfig { rename: "Ctrl+C".into(), color_picker: "Sol tık".into(), ..Default::default() };
        assert!(bad.color_picker_combo().is_none());
        assert_eq!(bad.rename_combo(), s.rename_combo());
    }

    #[test]
    fn snippet_filter() {
        let t = ToolsConfig { snippets: vec![Snippet { key: ";a".into(), text: "x".into() }, Snippet { key: "a b".into(), text: "x".into() }, Snippet { key: ";b".into(), text: String::new() }], ..Default::default() };
        assert_eq!(t.valid_snippets().count(), 1);
    }

    #[test]
    fn toml_roundtrip() {
        let t = ToolsConfig::default();
        let s = toml::to_string(&t).unwrap();
        assert_eq!(toml::from_str::<ToolsConfig>(&s).unwrap(), t);
    }
}
