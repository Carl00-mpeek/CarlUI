//! Dosya konumları. Ayarlar dolaşan profilde (%APPDATA%), log ve çalışma
//! durumu makineye özel profilde (%LOCALAPPDATA%) tutulur.

use std::path::PathBuf;

fn env_dir(var: &str) -> PathBuf {
    std::env::var_os(var).map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("CarlUI")
}

pub fn config_file() -> PathBuf {
    env_dir("APPDATA").join("config.toml")
}

pub fn log_dir() -> PathBuf {
    env_dir("LOCALAPPDATA").join("logs")
}

/// Modüllerin önbellek ve kullanım verileri (`%LOCALAPPDATA%\CarlUI`).
pub fn data_dir() -> PathBuf {
    let d = env_dir("LOCALAPPDATA");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Görev çubuğunu gizlediğimizi kaydeden dosya. Varsa, bir önceki oturum
/// sistemi değiştirmiş ve henüz geri almamış demektir.
pub fn state_file() -> PathBuf {
    env_dir("LOCALAPPDATA").join("taskbar-state.json")
}

/// Çalışan exe ile aynı klasördeki başka bir exe (modüller yan yana kurulur).
pub fn sibling_exe(name: &str) -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(name)))
        .unwrap_or_else(|| PathBuf::from(name))
}
