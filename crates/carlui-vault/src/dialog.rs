//! Dosya seçme/kaydetme pencereleri (ek dosya için).

use windows::{
    core::PWSTR,
    Win32::{Foundation::HWND, UI::Controls::Dialogs::*},
};

fn run(owner: HWND, save: bool, suggested: &str) -> Option<std::path::PathBuf> {
    let mut buf = vec![0u16; 4096];
    for (i, u) in suggested.encode_utf16().take(4000).enumerate() {
        buf[i] = u;
    }
    let filter: Vec<u16> = "*.*\0*.*\0\0".encode_utf16().collect();
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: windows::core::PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(buf.as_mut_ptr()),
        nMaxFile: buf.len() as u32,
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
    run(owner, false, "")
}

pub fn save(owner: HWND, name: &str) -> Option<std::path::PathBuf> {
    run(owner, true, name)
}
