//! Bu modül "yapay zekâ özelliklerinin ayarları nerede durur?" sorununu
//! çözüyor: `config.toml`'daki `[ai]` bölümü. Her özellik için seçilen model
//! kimliği (boşsa önerilen model) ve ekran kartı kullanımı. Modellerin
//! kendisi `carlui-ai` crate'indeki katalogdadır; burada yalnızca seçim durur.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct AiConfig {
    /// Ekran kartı (DirectML) kullanılsın mı; kapalıysa yalnızca işlemci.
    pub gpu: bool,
    /// Resim büyütme modeli (boşsa önerilen).
    pub upscale: String,
    /// Arka plan silme modeli.
    pub background: String,
    /// Konuşma → yazı (dikte, yazıya dökme) modeli.
    pub speech: String,
    /// Sesli komut modeli.
    pub command: String,
    /// Çeviri modeli.
    pub translate: String,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self { gpu: true, upscale: String::new(), background: String::new(), speech: String::new(), command: String::new(), translate: String::new() }
    }
}

impl AiConfig {
    /// Özellik anahtarına (`upscale`, `background` …) göre seçili model kimliği.
    pub fn chosen(&self, key: &str) -> &str {
        match key {
            "upscale" => &self.upscale,
            "background" => &self.background,
            "speech" => &self.speech,
            "command" => &self.command,
            "translate" => &self.translate,
            _ => "",
        }
    }

    pub fn set(&mut self, key: &str, id: &str) {
        let s = match key {
            "upscale" => &mut self.upscale,
            "background" => &mut self.background,
            "speech" => &mut self.speech,
            "command" => &mut self.command,
            "translate" => &mut self.translate,
            _ => return,
        };
        *s = id.to_string();
    }
}
