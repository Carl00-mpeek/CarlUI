//! Kullanıcının kaydettiği görünümler: `config.toml`'un yanındaki `looks.json`
//! (%APPDATA%\CarlUI). Ayar dosyasıyla birlikte dolaşsın diye oradadır ama
//! ayrı dosyadır; görünüm listesi büyüse de `config.toml` sade kalır.

use super::{sanitize, Look};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// En çok bu kadar kullanıcı görünümü saklanır.
pub const MAX_LOOKS: usize = 48;

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Clone)]
#[serde(default)]
pub struct Store {
    pub looks: Vec<Look>,
}

pub fn path() -> PathBuf {
    crate::paths::config_file().with_file_name("looks.json")
}

impl Store {
    /// Okur; dosya yoksa ya da bozuksa boş liste. Değerler aralığa çekilir.
    pub fn load() -> Self {
        let mut s: Store = std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        s.looks = s.looks.iter().map(sanitize::sanitized).filter(|l| !l.name.is_empty()).take(MAX_LOOKS).collect();
        s
    }

    pub fn save(&self) -> std::io::Result<()> {
        let p = path();
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(tmp, p)
    }

    fn find(&self, name: &str) -> Option<usize> {
        let n = name.to_lowercase();
        self.looks.iter().position(|l| l.name.to_lowercase() == n)
    }

    /// Aynı adlı görünüm varsa yerine yazar (büyük/küçük harf farkı sayılmaz),
    /// yoksa sona ekler. Ad boşsa ya da liste doluysa `false`.
    pub fn put(&mut self, look: Look) -> bool {
        let look = sanitize::sanitized(&look);
        if look.name.is_empty() {
            return false;
        }
        match self.find(&look.name) {
            Some(i) => self.looks[i] = look,
            None if self.looks.len() < MAX_LOOKS => self.looks.push(look),
            None => return false,
        }
        true
    }

    /// Yeniden adlandırır. Yeni ad boşsa ya da başka bir görünümde varsa `false`.
    pub fn rename(&mut self, i: usize, name: &str) -> bool {
        let name = sanitize::clean_name(name);
        if name.is_empty() || i >= self.looks.len() || self.find(&name).is_some_and(|j| j != i) {
            return false;
        }
        self.looks[i].name = name;
        true
    }

    pub fn remove(&mut self, i: usize) -> Option<Look> {
        (i < self.looks.len()).then(|| self.looks.remove(i))
    }

    /// Ad başka görünümde yoksa olduğu gibi, varsa "Ad (2)" gibi boş bir ad.
    pub fn free_name(&self, name: &str) -> String {
        let base = sanitize::clean_name(name);
        if self.find(&base).is_none() {
            return base;
        }
        for k in 2..=MAX_LOOKS + 1 {
            let tail = format!(" ({k})");
            let head: String = base.chars().take(sanitize::NAME_MAX - tail.chars().count()).collect();
            let n = format!("{}{tail}", head.trim_end());
            if self.find(&n).is_none() {
                return n;
            }
        }
        base
    }
}
