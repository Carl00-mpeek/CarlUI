//! Görünüm paylaşımı: `.carllook` dosyasına dışa aktarma ve dosyadan içe
//! aktarma (Windows dosya pencereleri, `backup.rs` ile aynı yol). Dosyanın
//! okunması ve doğrulanması çekirdekte (`carlui_core::looks::file`).

use carlui_core::looks::{file, Look};
use std::path::PathBuf;
use windows::core::{w, PWSTR};
use windows::Win32::UI::Controls::Dialogs::*;
use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;

/// Kaydet (`save`) ya da aç penceresi; iptalde `None`. `default` önerilen dosya adı.
fn dialog(save: bool, default: &str) -> Option<PathBuf> {
    let mut buf = [0u16; 1024];
    if save {
        for (i, c) in default.encode_utf16().take(buf.len() - 1).enumerate() {
            buf[i] = c;
        }
    }
    let tr = carlui_core::lang::is_tr();
    unsafe {
        let mut ofn = OPENFILENAMEW {
            lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: GetActiveWindow(),
            lpstrFilter: if tr { w!("CarlUI görünümü (*.carllook)\0*.carllook\0\0") } else { w!("CarlUI look (*.carllook)\0*.carllook\0\0") },
            lpstrFile: PWSTR(buf.as_mut_ptr()),
            nMaxFile: buf.len() as u32,
            lpstrDefExt: w!("carllook"),
            ..Default::default()
        };
        let ok = if save {
            ofn.Flags = OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
            ofn.lpstrTitle = if tr { w!("Görünümü dışa aktar") } else { w!("Export look") };
            GetSaveFileNameW(&mut ofn).as_bool()
        } else {
            ofn.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
            ofn.lpstrTitle = if tr { w!("Görünümü içe aktar") } else { w!("Import look") };
            GetOpenFileNameW(&mut ofn).as_bool()
        };
        if !ok {
            return None;
        }
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]).into())
}

/// Görünümü seçilen dosyaya yazar; iptalde `Ok(false)`.
pub fn export(look: &Look) -> Result<bool, String> {
    let Some(path) = dialog(true, &file::suggested_name(look)) else { return Ok(false) };
    file::write(&path, look).map_err(|e| t!(format!("Dosya yazılamadı: {e}"), format!("Could not write the file: {e}")))?;
    carlui_core::info!("görünüm dışa aktarıldı: {}", path.display());
    Ok(true)
}

/// Seçilen dosyadaki görünümü okur; iptalde `Ok(None)`.
pub fn import() -> Result<Option<Look>, String> {
    let Some(path) = dialog(false, "") else { return Ok(None) };
    let look = file::read(&path).map_err(|e| e.message())?;
    carlui_core::info!("görünüm içe aktarıldı: {}", path.display());
    Ok(Some(look))
}
