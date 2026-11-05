//! Stüdyonun dosya açma/kaydetme pencereleri (Windows'un kendi iletişim kutusu).

use std::path::PathBuf;
use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{Foundation::HWND, UI::Controls::Dialogs::*},
};

/// `filters`: (etiket, uzantı listesi "*.wav;*.mp3"). Kaydetmede seçilen
/// süzgecin sırası (0'dan) da döner.
fn run(owner: HWND, save: bool, name: &str, filters: &[(&str, &str)]) -> Option<(PathBuf, usize)> {
    let mut buf = vec![0u16; 4096];
    for (i, u) in name.encode_utf16().take(4000).enumerate() {
        buf[i] = u;
    }
    let mut f = String::new();
    for (label, pat) in filters {
        f.push_str(&format!("{label} ({pat})\0{pat}\0"));
    }
    f.push('\0');
    let filter: Vec<u16> = f.encode_utf16().collect();
    let ext: Vec<u16> = filters.first().map(|(_, p)| p.split(';').next().unwrap_or("").trim_start_matches("*.").to_string()).unwrap_or_default().encode_utf16().chain(std::iter::once(0)).collect();
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        nFilterIndex: 1,
        lpstrFile: PWSTR(buf.as_mut_ptr()),
        nMaxFile: buf.len() as u32,
        lpstrDefExt: PCWSTR(ext.as_ptr()),
        Flags: if save { OFN_OVERWRITEPROMPT | OFN_NOCHANGEDIR | OFN_PATHMUSTEXIST } else { OFN_FILEMUSTEXIST | OFN_NOCHANGEDIR },
        ..Default::default()
    };
    let ok = unsafe { if save { GetSaveFileNameW(&mut ofn).as_bool() } else { GetOpenFileNameW(&mut ofn).as_bool() } };
    if !ok {
        return None;
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let idx = (ofn.nFilterIndex as usize).saturating_sub(1).min(filters.len().saturating_sub(1));
    let mut path = PathBuf::from(String::from_utf16_lossy(&buf[..n]));
    if save {
        // Yazılan uzantı süzgeçlerden biriyse korunur; değilse seçilen biçiminki eklenir.
        let ext = |p: &str| p.split(';').next().unwrap_or("").trim_start_matches("*.").to_string();
        let want = ext(filters[idx].1);
        let has = path.extension().map(|e| e.to_string_lossy().to_lowercase());
        let known = has.as_ref().is_some_and(|h| filters.iter().any(|f| ext(f.1) == *h));
        if !want.is_empty() && !known {
            path.set_extension(want);
        }
    }
    Some((path, idx))
}

pub fn open(owner: HWND, filters: &[(&str, &str)]) -> Option<PathBuf> {
    run(owner, false, "", filters).map(|r| r.0)
}

pub fn save(owner: HWND, name: &str, filters: &[(&str, &str)]) -> Option<(PathBuf, usize)> {
    run(owner, true, name, filters)
}
