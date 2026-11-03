//! Bu modül "en çok açtığım şey üstte çıksın" sorununu çözüyor: her açılışta
//! sonucun anahtarı sayılır ve `%LOCALAPPDATA%\CarlUI\launcher-usage.json`
//! dosyasına yazılır. İlk çalıştırmada eski CarlSpotlight'ın sayıları,
//! eşleştirilebilenler için içeri aktarılır.

use crate::index::{self, App};
use carlui_core::{info, paths};
use std::{collections::HashMap, path::PathBuf};

fn file() -> PathBuf {
    paths::data_dir().join("launcher-usage.json")
}

pub struct Usage {
    pub counts: HashMap<String, u32>,
    imported: bool,
}

impl Usage {
    pub fn load() -> Self {
        match std::fs::read(file()).ok().and_then(|b| serde_json::from_slice::<HashMap<String, u32>>(&b).ok()) {
            Some(counts) => Self { counts, imported: true },
            None => Self { counts: HashMap::new(), imported: false },
        }
    }

    /// Eski CarlSpotlight sayılarını uygulama listesiyle eşleştirip ekler (bir kez).
    pub fn import_legacy(&mut self, apps: &[App]) {
        if self.imported || apps.is_empty() {
            return;
        }
        self.imported = true;
        let legacy = index::legacy_usage();
        let mut n = 0;
        for (k, count) in legacy {
            let k = k.to_lowercase();
            // Eski anahtarlar: kısayol yolu (…\STEAM.LNK), AUMID ya da ms-settings adresi.
            let stem = std::path::Path::new(&k).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let hit = apps.iter().find(|a| a.parsing.to_lowercase() == k || a.aumid.to_lowercase() == k || (k.ends_with(".lnk") && a.name.to_lowercase() == stem));
            let key = match hit {
                Some(a) => format!("app:{}", a.parsing.to_lowercase()),
                None if k.starts_with("ms-settings:") => format!("set:{k}"),
                None => continue,
            };
            *self.counts.entry(key).or_default() += count;
            n += 1;
        }
        info!("eski CarlSpotlight kullanım verisinden {n} kayıt aktarıldı");
        self.save();
    }

    pub fn bump(&mut self, key: &str) {
        if key.is_empty() {
            return;
        }
        *self.counts.entry(key.to_string()).or_default() += 1;
        self.save();
    }

    fn save(&self) {
        if let Ok(j) = serde_json::to_vec(&self.counts) {
            let _ = std::fs::write(file(), j);
        }
    }
}
