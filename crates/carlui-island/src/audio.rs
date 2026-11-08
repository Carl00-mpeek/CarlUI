//! Bu modül "yalnızca şarkıyı çalan uygulamanın sesini değiştir" sorununu
//! Windows ses karıştırıcısının uygulama başına oturumlarıyla çözüyor (Ses
//! Karıştırıcısı'ndaki uygulama kaydırıcılarının aynısı). Medya oturumunun
//! kimliği ("Spotify.exe", "Chrome", Store AUMID…) ses oturumunun sürecine
//! eşlenir. Tarayıcıda ses sekme başına değil tarayıcı başına ayarlanabilir;
//! YouTube çalıyorsa tarayıcının sesi değişir, sistem sesi değişmez.

use carlui_core::quick::{app_sessions, process_aumid, process_exe};
use windows::{
    Win32::{
        Foundation::*,
        Media::Audio::{Endpoints::IAudioEndpointVolume, *},
        System::Com::*,
        UI::WindowsAndMessaging::*,
    },
};

/// Sistem sesi kodu: yüzde, sessizdeyse `MUTED` eklenmiş hâli.
const MUTED: usize = 1000;

fn level_code(level: f32, muted: bool) -> usize {
    (level.clamp(0.0, 1.0) * 100.0).round() as usize + if muted { MUTED } else { 0 }
}

/// `watch_master` iletisinin kodunu yüzde ve sessiz bilgisine çevirir.
pub fn decode_level(code: usize) -> (u8, bool) {
    ((code % MUTED) as u8, code >= MUTED)
}

fn default_endpoint(en: &IMMDeviceEnumerator) -> Option<(String, IAudioEndpointVolume)> {
    unsafe {
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let p = dev.GetId().ok()?;
        let id = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as _));
        Some((id, dev.Activate(CLSCTX_ALL, None).ok()?))
    }
}

/// Sistem sesini izler, değişince `msg` iletisini gönderir. Varsayılan çıkış
/// aygıtı değişince yeni aygıtın sesi "değişim" sayılmaz.
pub fn watch_master(hwnd: isize, msg: u32) {
    std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return };
        let (mut ep, mut id, mut last, mut tick) = (None, String::new(), None, 0u32);
        loop {
            if tick % 20 == 0 {
                match default_endpoint(&en) {
                    Some((new_id, e)) => {
                        if new_id != id {
                            id = new_id;
                            last = None;
                        }
                        ep = Some(e);
                    }
                    None => {
                        ep = None;
                        id.clear();
                        last = None;
                    }
                }
            }
            tick = tick.wrapping_add(1);
            if let Some(e) = &ep {
                if let (Ok(v), Ok(m)) = (e.GetMasterVolumeLevelScalar(), e.GetMute()) {
                    let code = level_code(v, m.as_bool());
                    if last.is_some_and(|l| l != code) {
                        let _ = PostMessageW(HWND(hwnd as _), msg, WPARAM(code), LPARAM(0));
                    }
                    last = Some(code);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
    });
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
    let pass = |stems: &[String], found: &mut Vec<ISimpleAudioVolume>| {
        found.extend(app_sessions(|pid| {
            let s = stem(&process_exe(pid));
            stems.contains(&s) || (!s.is_empty() && s.len() >= 4 && id.contains(&s)) || process_aumid(pid).eq_ignore_ascii_case(app)
        }));
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

#[cfg(test)]
mod level_tests {
    use super::*;

    #[test]
    fn level_round_trip() {
        assert_eq!(decode_level(level_code(0.0, false)), (0, false));
        assert_eq!(decode_level(level_code(0.424, false)), (42, false));
        assert_eq!(decode_level(level_code(1.0, false)), (100, false));
        assert_eq!(decode_level(level_code(1.0, true)), (100, true));
        assert_eq!(decode_level(level_code(0.0, true)), (0, true));
    }
}
