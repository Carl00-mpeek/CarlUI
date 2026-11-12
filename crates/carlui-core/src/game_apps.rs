//! Bu modül "kullanıcı bu programı oyun diye işaretledi mi?" sorusunu çözüyor.
//! Pencere sınıfından tanınmayan oyunlar (ör. Roblox) ayarlardaki listeye
//! (`game.apps`, exe adları) eklenir; büyütülmüş ya da ekranı kaplayan
//! pencereleri oyun sayılır. Liste her süreçte ayar dosyası değiştikçe
//! yeniden okunur; ayrıca bir bağlantı gerekmez.

use std::{
    sync::Mutex,
    time::{Instant, SystemTime},
};

struct Cache {
    list: Vec<String>,
    stamp: Option<SystemTime>,
    checked: Option<Instant>,
}

static CACHE: Mutex<Cache> = Mutex::new(Cache { list: Vec::new(), stamp: None, checked: None });

/// Dosya değişti mi diye en fazla bu sıklıkla bakılır.
const RECHECK_MS: u128 = 2000;

/// `exe`: tam yol ya da yalnızca ad; büyük/küçük harf önemsiz.
pub fn is_listed(exe: &str) -> bool {
    let name = base_name(exe);
    if name.is_empty() {
        return false;
    }
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if c.checked.is_none_or(|t| t.elapsed().as_millis() >= RECHECK_MS) {
        c.checked = Some(Instant::now());
        let stamp = std::fs::metadata(crate::paths::config_file()).and_then(|m| m.modified()).ok();
        if stamp != c.stamp {
            c.stamp = stamp;
            if let Some(cfg) = crate::config::load_checked() {
                c.list = cfg.game.apps;
            }
        }
    }
    matches(&c.list, name)
}

fn base_name(exe: &str) -> &str {
    exe.rsplit(['\\', '/']).next().unwrap_or("").trim()
}

/// Listede bu ad var mı ("roblox" ile "RobloxPlayerBeta.exe" değil, tam ad; ".exe" yazılmasa da olur).
pub fn matches(list: &[String], name: &str) -> bool {
    let strip = |s: &str| {
        let s = s.trim().trim_matches('"');
        let s = base_name(s);
        s.strip_suffix(".exe").or_else(|| s.strip_suffix(".EXE")).unwrap_or(s).to_lowercase()
    };
    let name = strip(name);
    !name.is_empty() && list.iter().any(|a| strip(a) == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_without_case_or_extension() {
        let list = vec!["RobloxPlayerBeta.exe".to_string(), "terraria".to_string()];
        assert!(matches(&list, "robloxplayerbeta.exe"));
        assert!(matches(&list, r"c:\users\x\appdata\local\roblox\versions\v1\robloxplayerbeta.exe"));
        assert!(matches(&list, "Terraria.exe"));
        assert!(!matches(&list, "roblox.exe"));
        assert!(!matches(&list, ""));
        assert!(!matches(&[], "cs2.exe"));
    }
}
