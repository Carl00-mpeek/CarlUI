//! Bu modül "aç / farklı kaydet pencereleri Office dosyalarını süzsün"
//! sorununu çözüyor: Windows'un ortak dosya penceresi (GetOpenFileNameW)
//! tür süzgeçleriyle açılır.

use std::path::PathBuf;
use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{Foundation::HWND, UI::Controls::Dialogs::*},
};


pub fn open_filter() -> Vec<(String, &'static str)> {
    vec![
        (carlui_core::t!("Tüm belgeler", "All documents").to_string(), "*.xlsx;*.xlsm;*.xls;*.csv;*.docx;*.pptx;*.pdf"),
        (carlui_core::t!("Tablolar", "Spreadsheets").to_string(), "*.xlsx;*.xlsm;*.xls;*.csv"),
        (carlui_core::t!("Word belgeleri", "Word documents").to_string(), "*.docx"),
        (carlui_core::t!("Sunular", "Presentations").to_string(), "*.pptx"),
        ("PDF".to_string(), "*.pdf"),
        (carlui_core::t!("Tüm dosyalar", "All files").to_string(), "*.*"),
    ]
}

fn run(owner: HWND, save: bool, suggested: &str, filter: &[(String, &str)], ext: &str) -> Option<PathBuf> {
    let mut buf = vec![0u16; 4096];
    for (i, u) in suggested.encode_utf16().take(4000).enumerate() {
        buf[i] = u;
    }
    let mut f = String::new();
    for (d, p) in filter {
        f.push_str(d);
        f.push('\0');
        f.push_str(p);
        f.push('\0');
    }
    f.push('\0');
    let filter: Vec<u16> = f.encode_utf16().collect();
    let def: Vec<u16> = ext.encode_utf16().chain([0]).collect();
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(buf.as_mut_ptr()),
        nMaxFile: buf.len() as u32,
        lpstrDefExt: if ext.is_empty() { PCWSTR::null() } else { PCWSTR(def.as_ptr()) },
        Flags: if save { OFN_OVERWRITEPROMPT | OFN_NOCHANGEDIR | OFN_PATHMUSTEXIST } else { OFN_FILEMUSTEXIST | OFN_NOCHANGEDIR },
        ..Default::default()
    };
    let ok = unsafe { if save { GetSaveFileNameW(&mut ofn).as_bool() } else { GetOpenFileNameW(&mut ofn).as_bool() } };
    if !ok {
        return None;
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..n]).into())
}

pub fn open(owner: HWND) -> Option<PathBuf> {
    run(owner, false, "", &open_filter(), "")
}

/// Yalnızca PDF seçtirir (birleştirme için).
pub fn open_pdf(owner: HWND) -> Option<PathBuf> {
    run(owner, false, "", &[("PDF".to_string(), "*.pdf")], "")
}

/// `ext` noktasız uzantı ("xlsx"); süzgeç yalnız o türü gösterir.
pub fn save(owner: HWND, name: &str, ext: &str) -> Option<PathBuf> {
    let desc = match ext {
        "xlsx" => carlui_core::t!("Excel çalışma kitabı", "Excel workbook"),
        "csv" => carlui_core::t!("CSV (virgülle ayrılmış)", "CSV (comma separated)"),
        "docx" => carlui_core::t!("Word belgesi", "Word document"),
        "pptx" => carlui_core::t!("PowerPoint sunusu", "PowerPoint presentation"),
        "pdf" => "PDF",
        _ => carlui_core::t!("Dosya", "File"),
    };
    let pat = format!("*.{ext}");
    run(owner, true, name, &[(desc.to_string(), pat.as_str())], ext)
}
