//! Bu modül "yalnızca şarkıyı çalan uygulamanın sesini değiştir" sorununu
//! Windows ses karıştırıcısının uygulama başına oturumlarıyla çözüyor (Ses
//! Karıştırıcısı'ndaki uygulama kaydırıcılarının aynısı). Medya oturumunun
//! kimliği ("Spotify.exe", "Chrome", Store AUMID…) ses oturumunun sürecine
//! eşlenir. Tarayıcıda ses sekme başına değil tarayıcı başına ayarlanabilir;
//! YouTube çalıyorsa tarayıcının sesi değişir, sistem sesi değişmez.

use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Media::Audio::*,
        System::{Com::*, Threading::*},
        UI::WindowsAndMessaging::*,
    },
};

fn process_exe(pid: u32) -> String {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        if ok.is_ok() { String::from_utf16_lossy(&buf[..len as usize]) } else { String::new() }
    }
}

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

pub fn stem(path: &str) -> String {
    std::path::Path::new(path).file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Medya kimliğinden beklenen exe adı (uzantısız, küçük harf). Bilinen tarayıcılar
/// kısa kimlik kullanır; Firefox kurulum yoluna göre üretilmiş bir karma kullanır.
fn expected_stem(app: &str) -> Option<String> {
    let id = app.to_lowercase();
    if let Some(name) = id.strip_suffix(".exe") {
        return Some(stem(name).rsplit('\\').next().unwrap_or(name).to_string());
    }
    let known = [("chrome", "chrome"), ("msedge", "msedge"), ("brave", "brave"), ("opera", "opera"), ("vivaldi", "vivaldi"), ("firefox", "firefox")];
    known.iter().find(|(k, _)| id == *k).map(|(_, v)| v.to_string())
}

/// Bu kimliği taşıyan açık pencerelerin exe adları (Firefox gibi kimliğini
/// pencereye yazan uygulamalar için).
fn stems_of_windows_with_id(app: &str) -> Vec<String> {
    struct Ctx<'a> {
        id: &'a str,
        out: Vec<String>,
    }
    unsafe extern "system" fn cb(hwnd: HWND, lp: LPARAM) -> BOOL {
        let ctx = &mut *(lp.0 as *mut Ctx);
        if IsWindowVisible(hwnd).as_bool() && crate::activate::window_aumid(hwnd).eq_ignore_ascii_case(ctx.id) {
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let s = stem(&process_exe(pid));
            if !s.is_empty() && !ctx.out.contains(&s) {
                ctx.out.push(s);
            }
        }
        TRUE
    }
    let mut ctx = Ctx { id: app, out: Vec::new() };
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut ctx as *mut _ as isize));
    }
    ctx.out
}

/// Medya uygulamasına ait ses oturumlarının ses kontrolleri.
pub fn app_volumes(app: &str) -> Vec<ISimpleAudioVolume> {
    if app.is_empty() {
        return Vec::new();
    }
    let id = app.to_lowercase();
    let mut stems: Vec<String> = expected_stem(app).into_iter().collect();
    let mut found = Vec::new();
    let pass = |stems: &[String], found: &mut Vec<ISimpleAudioVolume>| unsafe {
        let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return };
        let Ok(dev) = en.GetDefaultAudioEndpoint(eRender, eMultimedia) else { return };
        let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { return };
        let Ok(list) = mgr.GetSessionEnumerator() else { return };
        let n = list.GetCount().unwrap_or(0);
        for i in 0..n {
            let Ok(ctl) = list.GetSession(i) else { continue };
            let Ok(ctl2) = ctl.cast::<IAudioSessionControl2>() else { continue };
            let pid = ctl2.GetProcessId().unwrap_or(0);
            if pid == 0 {
                continue;
            }
            let exe = process_exe(pid);
            let s = stem(&exe);
            let hit = stems.contains(&s)
                || (!s.is_empty() && s.len() >= 4 && id.contains(&s))
                || process_aumid(pid).eq_ignore_ascii_case(app);
            if hit {
                if let Ok(v) = ctl.cast::<ISimpleAudioVolume>() {
                    found.push(v);
                }
            }
        }
    };
    pass(&stems, &mut found);
    if found.is_empty() {
        stems = stems_of_windows_with_id(app);
        if !stems.is_empty() {
            pass(&stems, &mut found);
        }
    }
    found
}

pub fn get(vols: &[ISimpleAudioVolume]) -> Option<f32> {
    vols.first().and_then(|v| unsafe { v.GetMasterVolume().ok() })
}

pub fn set(vols: &[ISimpleAudioVolume], v: f32) {
    for vol in vols {
        unsafe {
            let _ = vol.SetMasterVolume(v.clamp(0.0, 1.0), std::ptr::null());
            if v > 0.0 {
                let _ = vol.SetMute(false, std::ptr::null());
            }
        }
    }
}
