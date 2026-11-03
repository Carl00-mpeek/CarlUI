//! Bu modül iki sorunu çözüyor:
//! 1. "Bu pencere hangi uygulamaya ait?" — Her pencere ve sabitlenmiş öğe bir
//!    `AppId`'ye indirgenir: varsa AppUserModelID (Store uygulamaları, Chrome,
//!    Firefox, Discord bunu kullanır), yoksa exe yolu. İkisi aynı kurala göre
//!    karşılaştırıldığı için kısayoldan açılan pencere doğru ikona düşer.
//! 2. "Görev çubuğundaki sabitlenmişler hangi sırayla?" — Explorer sırayı
//!    `HKCU\...\Explorer\Taskband\Favorites` değerinde, ardışık shell öğe
//!    kimlikleri (PIDL) olarak tutar: `[00] { u32 uzunluk, PIDL, 00 }…`.
//!    Her PIDL bir shell öğesine çevrilir; kısayolsa hedefi ve kimliği okunur.

use carlui_core::config::{expand_env, Pinned};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Storage::EnhancedStorage::PKEY_AppUserModel_ID,
        System::{Com::*, Registry::*, Threading::*},
        UI::Shell::{Common::ITEMIDLIST, PropertiesSystem::*, *},
    },
};

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct AppId {
    /// Küçük harf AppUserModelID (boş olabilir).
    pub aumid: String,
    /// Küçük harf tam exe yolu (boş olabilir).
    pub exe: String,
    /// Son çare eşleştirme adı: exe'nin uzantısız adı ya da sabitlenmiş öğenin adı.
    /// Discord gibi bir "Update.exe" başlatıcısıyla açılan uygulamalar için.
    pub stem: String,
}

fn stem_of(path: &str) -> String {
    std::path::Path::new(path).file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default()
}

impl AppId {
    pub fn of_window(aumid: &str, exe: &str) -> AppId {
        AppId { aumid: aumid.to_lowercase(), exe: exe.to_lowercase(), stem: stem_of(exe) }
    }

    pub fn matches(&self, other: &AppId) -> bool {
        if !self.aumid.is_empty() && !other.aumid.is_empty() && self.aumid == other.aumid {
            return true;
        }
        if !self.exe.is_empty() && self.exe == other.exe {
            return true;
        }
        !self.stem.is_empty() && self.stem == other.stem
    }

    /// Dock içinde öğeyi kalıcı olarak tanımlayan anahtar.
    pub fn key(&self) -> String {
        if self.aumid.is_empty() { format!("exe:{}", self.exe) } else { format!("id:{}", self.aumid) }
    }

    pub fn of_pin(p: &Pinned) -> AppId {
        let exe = if p.exe.is_empty() { expand_env(&p.path) } else { expand_env(&p.exe) };
        let exe = if exe.to_lowercase().ends_with(".exe") { exe } else { String::new() };
        // Başlatıcı exe'ler (Update.exe) uygulamanın kendisi değildir; adı kullan.
        let stem = match stem_of(&exe).as_str() {
            "" | "update" | "launcher" => p.name.to_lowercase().replace(' ', ""),
            s => s.to_string(),
        };
        AppId { aumid: p.aumid.to_lowercase(), exe: exe.to_lowercase(), stem }
    }
}

fn pwstr_take(p: PWSTR) -> String {
    unsafe {
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as _));
        s
    }
}

pub fn process_exe(pid: u32) -> String {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        if ok.is_ok() { String::from_utf16_lossy(&buf[..len as usize]) } else { String::new() }
    }
}

/// Paketli (Store/MSIX) bir sürecin uygulama kimliği; paketli değilse boş.
fn process_aumid(pid: u32) -> String {
    use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let rc = GetApplicationUserModelId(h, &mut len, PWSTR(buf.as_mut_ptr()));
        let _ = CloseHandle(h);
        if rc.is_ok() && len > 1 { String::from_utf16_lossy(&buf[..len as usize - 1]) } else { String::new() }
    }
}

/// Pencerenin kimliği: pencereye açıkça atanmış AUMID, yoksa paketli sürecin AUMID'si.
pub fn window_app_id(hwnd: HWND) -> String {
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, GetWindowThreadProcessId};
    let explicit = window_aumid(hwnd);
    if !explicit.is_empty() {
        return explicit;
    }
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let mut id = process_aumid(pid);
        if id.is_empty() {
            if let Ok(core) = FindWindowExW(hwnd, HWND::default(), w!("Windows.UI.Core.CoreWindow"), PCWSTR::null()) {
                let mut cpid = 0;
                GetWindowThreadProcessId(core, Some(&mut cpid));
                id = process_aumid(cpid);
            }
        }
        id
    }
}

/// Pencerenin sürecinin exe yolu. Store uygulamalarında (ApplicationFrameHost
/// içinde çalışanlar) asıl uygulamanın süreci alt pencereden bulunur.
pub fn process_exe_of_window(hwnd: HWND) -> String {
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, GetWindowThreadProcessId};
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let exe = process_exe(pid);
        if exe.to_lowercase().ends_with("applicationframehost.exe") {
            if let Ok(core) = FindWindowExW(hwnd, HWND::default(), w!("Windows.UI.Core.CoreWindow"), PCWSTR::null()) {
                let mut cpid = 0;
                GetWindowThreadProcessId(core, Some(&mut cpid));
                if cpid != 0 {
                    return process_exe(cpid);
                }
            }
        }
        exe
    }
}

/// Pencerenin asıl sürecinin kimliği (Store uygulamalarında çerçeve değil, uygulama).
pub fn process_id_of_window(hwnd: HWND) -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, GetWindowThreadProcessId};
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if process_exe(pid).to_lowercase().ends_with("applicationframehost.exe") {
            if let Ok(core) = FindWindowExW(hwnd, HWND::default(), w!("Windows.UI.Core.CoreWindow"), PCWSTR::null()) {
                let mut cpid = 0;
                GetWindowThreadProcessId(core, Some(&mut cpid));
                if cpid != 0 {
                    return cpid;
                }
            }
        }
        pid
    }
}

/// Pencerenin açıkça ayarlanmış AppUserModelID'si (yoksa boş).
pub fn window_aumid(hwnd: HWND) -> String {
    unsafe {
        let Ok(store) = SHGetPropertyStoreForWindow::<_, IPropertyStore>(hwnd) else { return String::new() };
        store.GetValue(&PKEY_AppUserModel_ID).map(|v| v.to_string()).unwrap_or_default()
    }
}

/// Exe'nin dosya açıklaması ("Mozilla Firefox" gibi); yoksa dosya adı.
fn file_description(exe: &str) -> Option<String> {
    use windows::Win32::Storage::FileSystem::*;
    unsafe {
        let path = HSTRING::from(exe);
        let size = GetFileVersionInfoSizeW(&path, None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(&path, 0, size, data.as_mut_ptr() as _).ok()?;
        let mut ptr = std::ptr::null_mut();
        let mut len = 0u32;
        if !VerQueryValueW(data.as_ptr() as _, w!("\\VarFileInfo\\Translation"), &mut ptr, &mut len).as_bool() || len < 4 {
            return None;
        }
        let lang = *(ptr as *const u16);
        let cp = *(ptr as *const u16).add(1);
        let key = HSTRING::from(format!("\\StringFileInfo\\{lang:04x}{cp:04x}\\FileDescription"));
        if !VerQueryValueW(data.as_ptr() as _, &key, &mut ptr, &mut len).as_bool() || len == 0 {
            return None;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(ptr as *const u16, len as usize));
        let s = s.trim_end_matches('\0').trim().to_string();
        (!s.is_empty()).then_some(s)
    }
}

/// Sabitlenmemiş açık bir uygulamanın dock'ta gösterilecek adı.
pub fn display_name(app: &AppId, exe_original_case: &str) -> String {
    if !app.aumid.is_empty() {
        unsafe {
            let path = HSTRING::from(format!("shell:AppsFolder\\{}", app.aumid));
            if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&path, None) {
                if let Ok(n) = item.GetDisplayName(SIGDN_NORMALDISPLAY) {
                    return pwstr_take(n);
                }
            }
        }
    }
    file_description(exe_original_case).unwrap_or_else(|| {
        std::path::Path::new(exe_original_case).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
    })
}

/// Kısayolun (.lnk) hedef exe'si ve argümanları.
fn resolve_lnk(path: &str) -> Option<(String, String)> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        link.cast::<IPersistFile>().ok()?.Load(&HSTRING::from(path), STGM_READ).ok()?;
        let mut target = [0u16; 1024];
        link.GetPath(&mut target, std::ptr::null_mut(), 0).ok()?;
        let mut args = [0u16; 2048];
        let _ = link.GetArguments(&mut args);
        let s = |b: &[u16]| String::from_utf16_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]);
        Some((s(&target), s(&args)))
    }
}

fn item_to_pin(item: &IShellItem) -> Option<Pinned> {
    unsafe {
        let name = pwstr_take(item.GetDisplayName(SIGDN_NORMALDISPLAY).ok()?);
        let parsing = pwstr_take(item.GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING).ok()?);
        let aumid = item
            .cast::<IShellItem2>()
            .ok()
            .and_then(|i2| i2.GetString(&PKEY_AppUserModel_ID).ok())
            .map(pwstr_take)
            .unwrap_or_default();
        let lower = parsing.to_lowercase();
        let (path, exe) = if lower.ends_with(".lnk") {
            let exe = resolve_lnk(&parsing).map(|(t, _)| t).unwrap_or_default();
            (parsing, exe)
        } else if lower.ends_with(".exe") {
            (parsing.clone(), parsing)
        } else if !aumid.is_empty() {
            // Store / paketli uygulama: Uygulamalar klasöründen açılır.
            (format!("shell:AppsFolder\\{aumid}"), String::new())
        } else {
            return None;
        };
        Some(Pinned { name, path, args: String::new(), icon: String::new(), aumid, exe })
    }
}

/// Windows görev çubuğuna sabitlenmiş uygulamaları görev çubuğundaki sırayla okur.
pub fn import_taskbar_pins() -> Vec<Pinned> {
    let mut out = Vec::new();
    unsafe {
        let key = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Taskband");
        let mut size = 0u32;
        if RegGetValueW(HKEY_CURRENT_USER, key, w!("Favorites"), RRF_RT_REG_BINARY, None, None, Some(&mut size)).is_err() {
            return out;
        }
        let mut buf = vec![0u8; size as usize];
        if RegGetValueW(HKEY_CURRENT_USER, key, w!("Favorites"), RRF_RT_REG_BINARY, None, Some(buf.as_mut_ptr() as _), Some(&mut size)).is_err() {
            return out;
        }
        let mut pos = 1usize;
        while pos + 4 <= buf.len() {
            let n = u32::from_le_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]) as usize;
            let start = pos + 4;
            if n < 2 || start + n > buf.len() {
                break;
            }
            // PIDL'in kendi uzunluk zinciri kayıt sınırı içinde bitmeli; bitmiyorsa bozuk kayıt.
            let mut q = start;
            let mut valid = false;
            while q + 2 <= start + n {
                let cb = u16::from_le_bytes([buf[q], buf[q + 1]]) as usize;
                if cb == 0 {
                    valid = true;
                    break;
                }
                q += cb;
            }
            if valid {
                let pidl = buf[start..].as_ptr() as *const ITEMIDLIST;
                if let Ok(item) = SHCreateItemFromIDList::<IShellItem>(pidl) {
                    if let Some(p) = item_to_pin(&item) {
                        out.push(p);
                    }
                }
            }
            pos = start + n + 1;
        }
    }
    out
}
