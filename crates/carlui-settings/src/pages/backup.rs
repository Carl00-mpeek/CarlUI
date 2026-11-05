//! Bu bölüm "ayarları başka bilgisayara taşıma / yedekleme" sorununu şu
//! yöntemle çözüyor: Genel sayfasındaki iki düğme. Dışa aktar, `Config`'i
//! seçilen .toml dosyasına yazar; içe aktar, dosyayı okuyup doğrular ve
//! bütün ayarları değiştirir (bozuk dosyada mevcut ayarlar korunur).

use carlui_core::config::{self, Config};
use windows::core::{w, PWSTR};
use windows::Win32::UI::Controls::Dialogs::*;
use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;

fn dialog(save: bool) -> Option<std::path::PathBuf> {
    let mut buf = [0u16; 1024];
    if save {
        for (i, c) in t!("CarlUI ayarları.toml", "CarlUI settings.toml").encode_utf16().enumerate() {
            buf[i] = c;
        }
    }
    unsafe {
        let mut ofn = OPENFILENAMEW {
            lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: GetActiveWindow(),
            lpstrFilter: if carlui_core::lang::is_tr() { w!("Ayar dosyası (*.toml)\0*.toml\0Tüm dosyalar\0*.*\0\0") } else { w!("Settings file (*.toml)\0*.toml\0All files\0*.*\0\0") },
            lpstrFile: PWSTR(buf.as_mut_ptr()),
            nMaxFile: buf.len() as u32,
            lpstrDefExt: w!("toml"),
            ..Default::default()
        };
        let ok = if save {
            ofn.Flags = OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
            ofn.lpstrTitle = if carlui_core::lang::is_tr() { w!("Ayarları dışa aktar") } else { w!("Export settings") };
            GetSaveFileNameW(&mut ofn).as_bool()
        } else {
            ofn.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
            ofn.lpstrTitle = if carlui_core::lang::is_tr() { w!("Ayarları içe aktar") } else { w!("Import settings") };
            GetOpenFileNameW(&mut ofn).as_bool()
        };
        if !ok {
            return None;
        }
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]).into())
}

/// Ayarları seçilen dosyaya yazar; iptalde false. Hata olursa açıklamasını döner.
pub fn export(cfg: &Config) -> Result<bool, String> {
    let Some(path) = dialog(true) else { return Ok(false) };
    let text = config::to_text(cfg).ok_or_else(|| t!("Ayarlar metne çevrilemedi.", "Could not serialize the settings.").to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    carlui_core::info!("ayarlar dışa aktarıldı: {}", path.display());
    Ok(true)
}

/// Seçilen dosyadaki ayarları `cfg`'ye yükler. İptalde ya da hatada `cfg` değişmez.
pub fn import(cfg: &mut Config) -> Result<bool, String> {
    let Some(path) = dialog(false) else { return Ok(false) };
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    let new = config::from_text(&text).map_err(|e| t!(format!("Dosya okunamadı: {e}"), format!("Could not read the file: {e}")))?;
    *cfg = new;
    carlui_core::lang::apply(&cfg.general.language);
    carlui_core::info!("ayarlar içe aktarıldı: {}", path.display());
    Ok(true)
}

/// Bildirim sesi için .wav dosyası seçtirir; iptalde None.
pub fn pick_wav() -> Option<String> {
    let mut buf = [0u16; 1024];
    unsafe {
        let mut ofn = OPENFILENAMEW {
            lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: GetActiveWindow(),
            lpstrFilter: if carlui_core::lang::is_tr() { w!("Ses dosyası (*.wav)\0*.wav\0\0") } else { w!("Sound file (*.wav)\0*.wav\0\0") },
            lpstrFile: PWSTR(buf.as_mut_ptr()),
            nMaxFile: buf.len() as u32,
            lpstrTitle: if carlui_core::lang::is_tr() { w!("Bildirim sesi") } else { w!("Notification sound") },
            Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
            ..Default::default()
        };
        if !GetOpenFileNameW(&mut ofn).as_bool() {
            return None;
        }
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}
