//! Bu modül "son açılan dosyalar ana ekranda görünsün" sorununu çözüyor:
//! yollar `%LOCALAPPDATA%\CarlUI\office-recent.json` içinde, en yenisi başta,
//! en çok 12 tane tutulur. Artık var olmayan dosyalar listelenmez.

use std::path::{Path, PathBuf};

const MAX: usize = 12;

fn file() -> PathBuf {
    carlui_core::paths::data_dir().join("office-recent.json")
}

pub fn load() -> Vec<PathBuf> {
    let v: Vec<PathBuf> = std::fs::read(file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    v.into_iter().filter(|p| p.exists()).take(MAX).collect()
}

pub fn add(p: &Path) -> Vec<PathBuf> {
    let mut v = load();
    let p = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let p = PathBuf::from(p.to_string_lossy().trim_start_matches(r"\?\"));
    v.retain(|x| !x.as_os_str().eq_ignore_ascii_case(p.as_os_str()));
    v.insert(0, p);
    v.truncate(MAX);
    if let Ok(b) = serde_json::to_vec(&v) {
        let _ = std::fs::write(file(), b);
    }
    v
}
