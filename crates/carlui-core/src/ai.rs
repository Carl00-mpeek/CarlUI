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
    /// Konuşma dili ("tr", "en" …); boşsa otomatik bulunur.
    pub speech_lang: String,
    /// Sesli komutla anlık tekrar kaydı ("son iki dakikayı kaydet"). Mikrofon
    /// sürekli dinlendiği için varsayılan kapalı; ses hiçbir yere gitmez.
    pub voice_command: bool,
    /// Sesli okuma (İngilizce ses) modeli; Türkçe metin her zaman Türkçe sesle okunur.
    pub speak: String,
    /// Okuma hızı (1.0 normal; 0.5–2.0).
    pub speak_speed: f32,
    /// Sesli asistan (bas-konuş komutları). Model ilk kullanımda indirilir.
    pub assistant: bool,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self { gpu: true, upscale: String::new(), background: String::new(), speech: String::new(), command: String::new(), translate: String::new(), speech_lang: String::new(), voice_command: false, speak: String::new(), speak_speed: 1.0, assistant: false }
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
            "speak" => &self.speak,
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
            "speak" => &mut self.speak,
            _ => return,
        };
        *s = id.to_string();
    }
}

impl crate::config::ShortcutsConfig {
    /// Dikte kısayolu (boşsa kapalı; geçersizse varsayılan Ctrl+Alt+D).
    pub fn dictate_combo(&self) -> Option<crate::keys::Combo> {
        if self.dictate.trim().is_empty() {
            return None;
        }
        crate::keys::parse_combo(&self.dictate)
            .or_else(|e| {
                crate::warn!("dikte kısayolu '{}' kullanılamaz: {e}; varsayılan (Ctrl+Alt+D) kullanılıyor", self.dictate);
                crate::keys::parse_combo(&Self::default().dictate)
            })
            .ok()
    }

    /// Sesli okuma kısayolu (boşsa kapalı; geçersizse varsayılan Ctrl+Alt+S).
    pub fn speak_combo(&self) -> Option<crate::keys::Combo> {
        if self.speak.trim().is_empty() {
            return None;
        }
        crate::keys::parse_combo(&self.speak)
            .or_else(|e| {
                crate::warn!("sesli okuma kısayolu '{}' kullanılamaz: {e}; varsayılan (Ctrl+Alt+S) kullanılıyor", self.speak);
                crate::keys::parse_combo(&Self::default().speak)
            })
            .ok()
    }

    /// Sesli asistan kısayolu (boşsa kapalı; geçersizse varsayılan Ctrl+Alt+A).
    pub fn assist_combo(&self) -> Option<crate::keys::Combo> {
        if self.assist.trim().is_empty() {
            return None;
        }
        crate::keys::parse_combo(&self.assist)
            .or_else(|e| {
                crate::warn!("sesli asistan kısayolu '{}' kullanılamaz: {e}; varsayılan (Ctrl+Alt+A) kullanılıyor", self.assist);
                crate::keys::parse_combo(&Self::default().assist)
            })
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use crate::config::ShortcutsConfig;

    #[test]
    fn dictate_combo() {
        assert!(ShortcutsConfig::default().dictate_combo().is_some());
        let off = ShortcutsConfig { dictate: String::new(), ..Default::default() };
        assert!(off.dictate_combo().is_none());
        let bad = ShortcutsConfig { dictate: "Sol tık".into(), ..Default::default() };
        assert_eq!(bad.dictate_combo(), ShortcutsConfig::default().dictate_combo());
    }
}
