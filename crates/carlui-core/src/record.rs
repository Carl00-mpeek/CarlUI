//! Bu modül "alan kaydının videoları nereye gider?" sorununu çözüyor:
//! `config.toml`'daki `[record]` bölümü. Eski sürümlerin `[replay]` bölümü de
//! okunur (yalnızca klasör kalır; anlık tekrar ayarları yok sayılır).

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct RecordConfig {
    /// Kayıtların klasörü; boşsa Videolar\CarlUI.
    pub folder: String,
}

impl RecordConfig {
    /// Kaydedilen videoların klasörü (`folder` boşsa Videolar\CarlUI).
    pub fn folder(&self) -> std::path::PathBuf {
        if !self.folder.trim().is_empty() {
            return std::path::PathBuf::from(self.folder.trim());
        }
        crate::win::videos_dir().unwrap_or_else(|| crate::paths::data_dir()).join("CarlUI")
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    #[test]
    fn old_replay_section_keeps_folder() {
        let c: Config = toml::from_str("[replay]\nenabled = true\nminutes = 5\nfolder = 'D:\\Klipler'\n").unwrap();
        assert_eq!(c.record.folder, r"D:\Klipler");
        let back = toml::to_string(&c).unwrap();
        assert!(back.contains("[record]") && !back.contains("[replay]"));
    }
}
