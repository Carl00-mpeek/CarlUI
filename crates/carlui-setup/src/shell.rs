//! Bu modül "kurucu Windows'un kabuk ve kayıt defteri işlerini yönetici izni
//! olmadan yapsın" sorununu kullanıcıya ait yerlerle çözüyor: kayıtlar HKCU'ya,
//! kısayollar kullanıcının Başlat menüsü/masaüstü klasörüne yazılır. Silme
//! yerine Geri Dönüşüm Kutusu kullanılır; çalışan CarlUI zorla öldürülmez,
//! kendi "kapan" olayıyla kibarca kapatılır. Yönetici gerektiren tek adım
//! (sensör görevi) UAC penceresiyle kullanıcıya sorulur.

use std::{
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{Com::*, Registry::*, Threading::*},
        UI::{Shell::*, WindowsAndMessaging::*},
    },
};

pub const NO_WINDOW: u32 = 0x0800_0000;
pub const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\CarlUI";
pub const FIREFOX_KEY: &str = r"Software\Mozilla\Firefox\Extensions";
pub const EXT_ID: &str = "downloads@carlui.local";
pub const UNINSTALLER: &str = "CarlUI-Uninstall.exe";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// Varsayılan kurulum yeri: `%LOCALAPPDATA%\Programs\CarlUI` (yetki gerekmez).
pub fn default_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join(r"Programs\CarlUI")
}

pub fn known(id: &GUID) -> Option<PathBuf> {
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        s.map(PathBuf::from)
    }
}

/// Başlat menüsündeki `CarlUI` klasörü.
pub fn menu_dir() -> Option<PathBuf> {
    known(&FOLDERID_Programs).map(|p| p.join("CarlUI"))
}

pub fn desktop_link() -> Option<PathBuf> {
    known(&FOLDERID_Desktop).map(|p| p.join("CarlUI.lnk"))
}

pub enum Val<'a> {
    Str(&'a str),
    Dword(u32),
}

/// HKCU altında anahtarı açar (yoksa oluşturur) ve değerleri yazar.
pub fn reg_set(path: &str, values: &[(&str, Val)]) -> bool {
    unsafe {
        let mut key = HKEY::default();
        if RegCreateKeyExW(HKEY_CURRENT_USER, &HSTRING::from(path), 0, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_WRITE, None, &mut key, None) != ERROR_SUCCESS {
            return false;
        }
        let mut ok = true;
        for (name, v) in values {
            let n = HSTRING::from(*name);
            let r = match v {
                Val::Str(s) => {
                    let w = wide(s);
                    let bytes = std::slice::from_raw_parts(w.as_ptr() as *const u8, w.len() * 2);
                    RegSetValueExW(key, &n, 0, REG_SZ, Some(bytes))
                }
                Val::Dword(d) => RegSetValueExW(key, &n, 0, REG_DWORD, Some(&d.to_le_bytes())),
            };
            ok &= r == ERROR_SUCCESS;
        }
        let _ = RegCloseKey(key);
        ok
    }
}

pub fn reg_get(hive: HKEY, path: &str, name: &str) -> Option<String> {
    unsafe {
        let (p, n) = (HSTRING::from(path), HSTRING::from(name));
        let mut size = 0u32;
        if RegGetValueW(hive, &p, &n, RRF_RT_REG_SZ, None, None, Some(&mut size)) != ERROR_SUCCESS || size < 2 {
            return None;
        }
        let mut buf = vec![0u16; size as usize / 2 + 1];
        if RegGetValueW(hive, &p, &n, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as _), Some(&mut size)) != ERROR_SUCCESS {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }
}

pub fn reg_delete_key(path: &str) {
    unsafe {
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(path));
        let _ = RegDeleteKeyW(HKEY_CURRENT_USER, &HSTRING::from(path));
    }
}

pub fn reg_delete_value(path: &str, name: &str) {
    unsafe {
        let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(path), &HSTRING::from(name));
    }
}

pub fn key_exists(hive: HKEY, path: &str) -> bool {
    unsafe {
        let mut k = HKEY::default();
        let ok = RegOpenKeyExW(hive, &HSTRING::from(path), 0, KEY_READ, &mut k) == ERROR_SUCCESS;
        if ok {
            let _ = RegCloseKey(k);
        }
        ok
    }
}

/// CPU sıcaklığı için gereken PawnIO çekirdek sürücüsü kurulu mu?
pub fn pawnio_installed() -> bool {
    key_exists(HKEY_LOCAL_MACHINE, r"SYSTEM\CurrentControlSet\Services\PawnIO")
}

/// Kayıtlı bir tarayıcının exe yolu (App Paths).
pub fn app_path(exe: &str) -> Option<String> {
    let key = format!(r"Software\Microsoft\Windows\CurrentVersion\App Paths\{exe}");
    [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE].into_iter().find_map(|h| reg_get(h, &key, "")).map(|s| s.trim_matches('"').to_string()).filter(|p| Path::new(p).exists())
}

pub fn shortcut(lnk: &Path, target: &Path, args: &str, desc: &str) -> bool {
    unsafe {
        let Ok(link) = CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER) else { return false };
        let t = HSTRING::from(target.as_os_str());
        let dir = HSTRING::from(target.parent().unwrap_or(target).as_os_str());
        let ok = link.SetPath(&t).is_ok() && link.SetArguments(&HSTRING::from(args)).is_ok() && link.SetWorkingDirectory(&dir).is_ok() && link.SetDescription(&HSTRING::from(desc)).is_ok() && link.SetIconLocation(&t, 0).is_ok();
        let Ok(pf) = link.cast::<IPersistFile>() else { return false };
        if let Some(d) = lnk.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        ok && pf.Save(&HSTRING::from(lnk.as_os_str()), true).is_ok()
    }
}

/// Kısayolun hedefi (kaldırırken başka bir şeye ait kısayola dokunmamak için).
pub fn link_target(lnk: &Path) -> String {
    unsafe {
        let Ok(link) = CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER) else { return String::new() };
        let Ok(pf) = link.cast::<IPersistFile>() else { return String::new() };
        if pf.Load(&HSTRING::from(lnk.as_os_str()), STGM_READ).is_err() {
            return String::new();
        }
        let mut buf = [0u16; 520];
        let _ = link.GetPath(&mut buf, std::ptr::null_mut(), 0);
        String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(0)])
    }
}

/// Dosya ya da klasörleri Geri Dönüşüm Kutusu'na gönderir.
pub fn recycle(paths: &[PathBuf]) -> bool {
    let list: Vec<&PathBuf> = paths.iter().filter(|p| p.exists()).collect();
    if list.is_empty() {
        return true;
    }
    let mut from: Vec<u16> = Vec::new();
    for p in list {
        from.extend(p.as_os_str().to_string_lossy().encode_utf16());
        from.push(0);
    }
    from.push(0);
    let mut op = SHFILEOPSTRUCTW { wFunc: FO_DELETE, pFrom: PCWSTR(from.as_ptr()), fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI).0 as u16, ..Default::default() };
    unsafe { SHFileOperationW(&mut op) == 0 && !op.fAnyOperationsAborted.as_bool() }
}

const HOST_MUTEX: PCWSTR = w!("Local\\CarlUI.Host");
const QUIT_EVENT: PCWSTR = w!("Local\\CarlUI.Quit");

pub fn carlui_running() -> bool {
    unsafe {
        match OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, HOST_MUTEX) {
            Ok(h) => {
                let _ = CloseHandle(h);
                true
            }
            Err(_) => false,
        }
    }
}

/// Çalışan CarlUI'den düzgünce kapanmasını ister (görev çubuğu geri gelir,
/// modüller kendileri çıkar). Çalışıyorduysa true.
pub fn quit_carlui(timeout: Duration) -> bool {
    if !carlui_running() {
        return false;
    }
    unsafe {
        if let Ok(ev) = OpenEventW(EVENT_MODIFY_STATE, false, QUIT_EVENT) {
            let _ = SetEvent(ev);
            let _ = CloseHandle(ev);
        }
    }
    let start = Instant::now();
    while carlui_running() && start.elapsed() < timeout {
        std::thread::sleep(Duration::from_millis(50));
    }
    true
}

pub fn run_hidden(exe: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(exe).args(args).creation_flags(NO_WINDOW).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

pub enum Elevated {
    Done,
    Declined,
    Failed(String),
}

/// Yönetici olarak çalıştırır (UAC penceresi) ve bitmesini bekler.
pub fn run_elevated(owner: HWND, exe: &Path, args: &str) -> Elevated {
    let file = HSTRING::from(exe.as_os_str());
    let params = HSTRING::from(args);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        hwnd: owner,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    unsafe {
        if let Err(e) = ShellExecuteExW(&mut info) {
            return if e.code() == ERROR_CANCELLED.to_hresult() { Elevated::Declined } else { Elevated::Failed(e.message().to_string()) };
        }
        if !info.hProcess.is_invalid() {
            WaitForSingleObject(info.hProcess, 60_000);
            let _ = CloseHandle(info.hProcess);
        }
        Elevated::Done
    }
}

pub fn open(target: &str, args: &str) {
    unsafe {
        ShellExecuteW(None, w!("open"), &HSTRING::from(target), &HSTRING::from(args), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

/// Klasör seçici. Kalıcı (modal) olduğu için uygulama durumu ödünç alınmadan çağrılır.
pub fn pick_folder(owner: HWND, start: &Path) -> Option<PathBuf> {
    unsafe {
        let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let _ = dlg.SetOptions(FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST);
        let _ = dlg.SetTitle(w!("CarlUI kurulum konumu"));
        let mut existing = start.to_path_buf();
        while !existing.exists() {
            existing = existing.parent()?.to_path_buf();
        }
        if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(existing.as_os_str()), None) {
            let _ = dlg.SetFolder(&item);
        }
        dlg.Show(owner).ok()?;
        let item = dlg.GetResult().ok()?;
        let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        let mut dir = PathBuf::from(s?);
        // Seçilen klasörün içine "CarlUI" alt klasörü açılır; başka dosyalarla karışmasın.
        if !dir.file_name().is_some_and(|n| n.eq_ignore_ascii_case("CarlUI")) {
            dir.push("CarlUI");
        }
        Some(dir)
    }
}

/// Geçici kaldırıcı kopyasının adı: `%TEMP%\CarlUI-Uninstall-<pid>.exe`.
pub fn temp_copy_name(pid: u32) -> String {
    format!("CarlUI-Uninstall-{pid}.exe")
}

/// Önceki kaldırmalardan %TEMP%'te kalan kaldırıcı kopyalarını siler.
/// Çalışan exe kendini silemez; eskiden gizli `cmd /c ping & del` ile
/// siliniyordu ama bu "gizli komut satırı" davranışı Defender'ın makine
/// öğrenmesi tespitini (Behavior:Win32/Execution.A!ml) tetikliyordu. Artık
/// kopya bir sonraki kurucu/kaldırıcı açılışında sessizce temizlenir.
pub fn sweep_temp_copies() {
    let me = std::env::current_exe().ok();
    let Ok(rd) = std::fs::read_dir(std::env::temp_dir()) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_ascii_lowercase();
        if name.starts_with("carlui-uninstall-") && name.ends_with(".exe") && me.as_deref() != Some(e.path().as_path()) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}
