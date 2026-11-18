//! Dosya seçme/kaydetme pencereleri (ek dosya, içe/dışa aktarma).

use windows::{
    core::PWSTR,
    Win32::{Foundation::HWND, UI::Controls::Dialogs::*},
};

/// `ext`: kaydederken önerilen uzantı ("carlvault"); boşsa her dosya.
fn run(owner: HWND, save: bool, suggested: &str, ext: &str) -> Option<std::path::PathBuf> {
    let mut buf = vec![0u16; 4096];
    for (i, u) in suggested.encode_utf16().take(4000).enumerate() {
        buf[i] = u;
    }
    let filter: Vec<u16> = if ext.is_empty() { "*.*\0*.*\0\0".to_string() } else { format!("*.{ext}\0*.{ext}\0*.*\0*.*\0\0") }.encode_utf16().collect();
    let def_ext: Vec<u16> = ext.encode_utf16().chain([0]).collect();
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: windows::core::PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(buf.as_mut_ptr()),
        nMaxFile: buf.len() as u32,
        lpstrDefExt: if ext.is_empty() { windows::core::PCWSTR::null() } else { windows::core::PCWSTR(def_ext.as_ptr()) },
        Flags: if save { OFN_OVERWRITEPROMPT | OFN_NOCHANGEDIR } else { OFN_FILEMUSTEXIST | OFN_NOCHANGEDIR },
        ..Default::default()
    };
    let ok = unsafe { if save { GetSaveFileNameW(&mut ofn).as_bool() } else { GetOpenFileNameW(&mut ofn).as_bool() } };
    if !ok {
        return None;
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..n]).into())
}

pub fn open(owner: HWND) -> Option<std::path::PathBuf> {
    run(owner, false, "", "")
}

pub fn save(owner: HWND, name: &str) -> Option<std::path::PathBuf> {
    run(owner, true, name, "")
}

/// Uzantısı verilmiş dosya kaydet (yedek): uzantı yazılmazsa eklenir.
pub fn save_as(owner: HWND, name: &str, ext: &str) -> Option<std::path::PathBuf> {
    run(owner, true, name, ext)
}
