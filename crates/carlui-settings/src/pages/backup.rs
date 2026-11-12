//! Bu bölüm "ayarları başka bilgisayara taşıma / yedekleme" sorununu şu
//! yöntemle çözüyor: Genel sayfasındaki iki düğme. Dışa aktar, seçilen klasörün
//! içine yedek klasörü açar (`backup_set`); içe aktar, bir .toml dosyasını okuyup
//! doğrular ve bütün ayarları değiştirir (bozuk dosyada mevcut ayarlar korunur).

use carlui_core::config::{self, Config};
use super::backup_set;
use windows::core::{w, HSTRING, PWSTR};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Controls::Dialogs::*;
use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
use windows::Win32::UI::Shell::{FileOpenDialog, IFileOpenDialog, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, SIGDN_FILESYSPATH};

/// Yedeğin konacağı klasörü seçtirir; iptalde None.
fn pick_folder() -> Option<std::path::PathBuf> {
    unsafe {
        let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let _ = dlg.SetOptions(FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST);
        let _ = dlg.SetTitle(&HSTRING::from(t!("Yedeğin kaydedileceği klasörü seç", "Choose where to save the backup")));
        dlg.Show(GetActiveWindow()).ok()?;
        let item = dlg.GetResult().ok()?;
        let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        s.map(Into::into)
    }
}

/// Yedek klasöründeki ya da tek başına bir .toml dosyası seçtirir; iptalde None.
fn dialog() -> Option<std::path::PathBuf> {
    let mut buf = [0u16; 1024];
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
        ofn.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
        ofn.lpstrTitle = if carlui_core::lang::is_tr() { w!("Ayarları içe aktar") } else { w!("Import settings") };
        if !GetOpenFileNameW(&mut ofn).as_bool() {
            return None;
        }
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]).into())
}

/// Bugünün tarihi, `YYYY-MM-DD` (yerel saat).
fn today() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay)
}

/// Seçilen klasörün içine `CarlUI-yedek-<tarih>` açıp yedeği yazar (ayarlar,
/// görünümler, düzenler, hayvan paketleri). İptalde `Ok(None)`; başarıda kullanıcıya
/// gösterilecek mesaj (klasör yolu dahil). Hata olursa açıklamasını döner.
pub fn export(cfg: &Config) -> Result<Option<String>, String> {
    let Some(base) = pick_folder() else { return Ok(None) };
    let text = config::to_text(cfg).ok_or_else(|| t!("Ayarlar metne çevrilemedi.", "Could not serialize the settings.").to_string())?;
    let cfg_dir = carlui_core::paths::config_file().parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let folder = backup_set::export(&base, &today(), &text, &cfg_dir, &carlui_core::paths::data_dir()).map_err(|e| format!("{}: {e}", base.display()))?;
    carlui_core::info!("ayarlar dışa aktarıldı: {}", folder.display());
    Ok(Some(t!(format!("Ayarlar dışa aktarıldı: {}", folder.display()), format!("Settings exported to: {}", folder.display()))))
}

/// Seçilen dosyadaki ayarları `cfg`'ye yükler. İptalde ya da hatada `cfg` değişmez.
/// Başarıda kullanıcıya gösterilecek mesajı döner; ek parçalar yüklenemediyse mesaj bunu söyler.
pub fn import(cfg: &mut Config) -> Result<Option<String>, String> {
    let Some(path) = dialog() else { return Ok(None) };
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    let new = config::from_text(&text).map_err(|e| t!(format!("Dosya okunamadı: {e}"), format!("Could not read the file: {e}")))?;
    *cfg = new;
    // Yedek klasöründeki config.toml ise yanındaki görünüm, düzen ve hayvan paketleri de dönülür.
    let mut failed: Vec<String> = Vec::new();
    if backup_set::is_backup_config(&path) {
        if let Some(folder) = path.parent() {
            let cfg_dir = carlui_core::paths::config_file().parent().map(|p| p.to_path_buf()).unwrap_or_default();
            failed = backup_set::restore(folder, &cfg_dir, &carlui_core::paths::data_dir()).1;
            if !failed.is_empty() {
                carlui_core::warn!("yedekten ek parçalar yüklenemedi: {}", failed.join(", "));
            }
        }
    }
    carlui_core::lang::apply(&cfg.general.language);
    carlui_core::info!("ayarlar içe aktarıldı: {}", path.display());
    let list = failed.join(", ");
    Ok(Some(if failed.is_empty() {
        t!("Ayarlar içe aktarıldı.", "Settings imported.").to_string()
    } else {
        t!(format!("Ayarlar geri yüklendi, ancak şu parçalar yüklenemedi: {list}"), format!("Settings restored, but these parts could not be loaded: {list}"))
    }))
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
