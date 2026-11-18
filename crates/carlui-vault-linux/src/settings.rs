//! Linux sürümünün ayarları: `~/.local/share/CarlUI/vault-linux.json`.
//! Kasanın kendisi (`vault/`) Windows'takiyle aynı biçimdedir; bu dosya
//! yalnızca bu pencereye aittir.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Bu kadar dakika işlem olmazsa kasa kilitlenir (0: hiç).
    pub auto_lock: u32,
    /// Tarayıcı eklentisiyle doldurma, kaydetme ve passkey.
    pub browser_fill: bool,
    /// "auto", "tr" ya da "en".
    pub lang: String,
    /// Kopyalanan değer bu kadar saniye sonra panodan silinir (0: silinmez).
    pub clipboard: u32,
    /// Pencere kapatılınca uygulama kapanmaz, simge durumuna küçülür (köprü açık kalır).
    pub keep_running: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { auto_lock: 15, browser_fill: true, lang: "auto".into(), clipboard: 30, keep_running: true }
    }
}

pub fn dir() -> PathBuf {
    carlui_vault::vault::data_root().join("CarlUI")
}

fn file() -> PathBuf {
    dir().join("vault-linux.json")
}

impl Settings {
    pub fn load() -> Self {
        std::fs::read(file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(dir());
        if let Ok(b) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(file(), b);
        }
    }
}
