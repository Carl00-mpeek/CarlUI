//! Bu modül "bildirime ya da şarkıya tıklayınca kaynağına git" sorununu
//! çözüyor: uygulama kimliğine (AUMID ya da exe adı) sahip açık bir pencere
//! varsa öne getirilir, yoksa uygulama `shell:AppsFolder` üzerinden açılır.

use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Storage::EnhancedStorage::PKEY_AppUserModel_ID,
        System::Threading::*,
        UI::{
            Shell::{PropertiesSystem::*, *},
            WindowsAndMessaging::*,
        },
    },
};

/// Pencerenin açıkça ayarlanmış AppUserModelID'si (yoksa boş).
pub fn window_aumid(hwnd: HWND) -> String {
    unsafe {
        let Ok(store) = SHGetPropertyStoreForWindow::<_, IPropertyStore>(hwnd) else { return String::new() };
        store.GetValue(&PKEY_AppUserModel_ID).map(|v| v.to_string()).unwrap_or_default()
    }
}

fn process_id_string(pid: u32) -> (String, String) {
    use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return Default::default() };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let exe = if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok() {
            String::from_utf16_lossy(&buf[..len as usize])
        } else {
            String::new()
        };
        let mut abuf = [0u16; 512];
        let mut alen = abuf.len() as u32;
        let aumid = if GetApplicationUserModelId(h, &mut alen, PWSTR(abuf.as_mut_ptr())).is_ok() && alen > 1 {
            String::from_utf16_lossy(&abuf[..alen as usize - 1])
        } else {
            String::new()
        };
        let _ = CloseHandle(h);
        (exe, aumid)
    }
}

/// Alt+Tab'da görünen bir uygulama penceresi mi?
fn is_app_window(hwnd: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || !GetWindow(hwnd, GW_OWNER).unwrap_or_default().is_invalid() {
            return false;
        }
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TOOLWINDOW.0 != 0 {
            return false;
        }
        let mut cloaked = 0u32;
        let _ = windows::Win32::Graphics::Dwm::DwmGetWindowAttribute(
            hwnd,
            windows::Win32::Graphics::Dwm::DWMWA_CLOAKED,
            &mut cloaked as *mut _ as *mut _,
            4,
        );
        cloaked == 0 && GetWindowTextLengthW(hwnd) > 0
    }
}

/// Uygulamanın en öndeki penceresi. `id` bir AUMID ya da exe adıdır.
pub fn find_window(id: &str) -> Option<HWND> {
    if id.is_empty() {
        return None;
    }
    struct Ctx<'a> {
        id: String,
        stem: String,
        found: Option<HWND>,
        _p: std::marker::PhantomData<&'a ()>,
    }
    unsafe extern "system" fn cb(hwnd: HWND, lp: LPARAM) -> BOOL {
        let ctx = &mut *(lp.0 as *mut Ctx);
        if !is_app_window(hwnd) {
            return TRUE;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let explicit = window_aumid(hwnd).to_lowercase();
        let (exe, aumid) = process_id_string(pid);
        let exe = exe.to_lowercase();
        let s = crate::audio::stem(&exe);
        let hit = (!explicit.is_empty() && explicit == ctx.id)
            || (!aumid.is_empty() && aumid.to_lowercase() == ctx.id)
            || (!ctx.stem.is_empty() && s == ctx.stem)
            || (s.len() >= 4 && ctx.id.contains(&s));
        if hit {
            ctx.found = Some(hwnd);
            return FALSE;
        }
        TRUE
    }
    let lower = id.to_lowercase();
    let stem = match lower.strip_suffix(".exe") {
        Some(n) => crate::audio::stem(n),
        None => match lower.as_str() {
            "308046b0af4a39cb" => "firefox".into(),
            "msedge" | "chrome" | "brave" | "opera" | "vivaldi" => lower.clone(),
            _ => String::new(),
        },
    };
    let mut ctx = Ctx { id: lower, stem, found: None, _p: std::marker::PhantomData };
    unsafe {
        // EnumWindows z-sırasıyla gezer: ilk bulunan en öndeki penceredir.
        let _ = EnumWindows(Some(cb), LPARAM(&mut ctx as *mut _ as isize));
    }
    ctx.found
}

pub fn bring_to_front(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindowAsync(hwnd, SW_RESTORE);
        }
        if !SetForegroundWindow(hwnd).as_bool() {
            SwitchToThisWindow(hwnd, true);
        }
    }
}

/// Uygulamayı öne getirir; açık penceresi yoksa başlatır. Başardıysa true.
pub fn open_app(id: &str) -> bool {
    if let Some(h) = find_window(id) {
        bring_to_front(h);
        return true;
    }
    if id.is_empty() || id.to_lowercase().ends_with(".exe") && !id.contains('\\') {
        return false;
    }
    let target = if id.contains('\\') { id.to_string() } else { format!("shell:AppsFolder\\{id}") };
    unsafe {
        let r = ShellExecuteW(HWND::default(), w!("open"), &HSTRING::from(target), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
        r.0 as isize > 32
    }
}
