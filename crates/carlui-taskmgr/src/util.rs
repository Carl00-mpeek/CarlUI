//! Küçük ortak yardımcılar: Windows'a ait dosya mı, komut satırından exe yolu,
//! Gezgin'de dosya konumunu açma.

use carlui_core::config::expand_env;
use std::sync::OnceLock;
use windows::{
    core::*,
    Win32::{
        Foundation::HWND,
        Storage::FileSystem::SearchPathW,
        UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    },
};

fn windir() -> &'static str {
    static W: OnceLock<String> = OnceLock::new();
    W.get_or_init(|| std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()).to_lowercase() + "\\")
}

/// Windows'un kendi dosyası mı (Windows klasöründe ya da yolu yok).
pub fn is_windows_path(path: &str) -> bool {
    let p = path.to_lowercase();
    p.starts_with(windir()) || p.contains("\\windowsapps\\microsoft.windows") || p.contains("\\windows defender\\") || p.contains("\\windows security\\")
}

/// Komut satırının çalıştırdığı exe'nin tam yolu (bulunamazsa boş).
pub fn exe_of(command: &str) -> String {
    let c = expand_env(command.trim());
    let first = if let Some(rest) = c.strip_prefix('"') {
        rest.split('"').next().unwrap_or_default().to_string()
    } else if let Some(i) = c.to_lowercase().find(".exe") {
        c[..i + 4].to_string()
    } else {
        c.split_whitespace().next().unwrap_or_default().to_string()
    };
    if first.is_empty() {
        return String::new();
    }
    if std::path::Path::new(&first).is_absolute() {
        return first;
    }
    let mut buf = [0u16; 520];
    let n = unsafe { SearchPathW(None, &HSTRING::from(first.as_str()), w!(".exe"), Some(&mut buf), None) };
    if n > 0 { String::from_utf16_lossy(&buf[..n as usize]) } else { String::new() }
}

/// Gezgin'i açıp dosyayı seçili gösterir.
pub fn reveal(path: &str) {
    if path.is_empty() {
        return;
    }
    unsafe {
        ShellExecuteW(HWND::default(), w!("open"), w!("explorer.exe"), &HSTRING::from(format!("/select,\"{path}\"")), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

pub fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string())
}
