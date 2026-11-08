//! Önizleme kartındaki önceki / oynat / sonraki düğmeleri.
//!
//! Windows görev çubuğunun küçük resim düğmeleri uygulamanın `ITaskbarList3`
//! ile görev çubuğuna verdiği düğmelerdir, başka süreç okuyamaz. Bu yüzden
//! aynı denetim sistemin medya oturumlarından (SMTC) alınır.
//!
//! Oturum sorguları arka planda çalışır: önizleme son görülen listeyle hemen
//! açılır, liste değişirse `WM_APP_MEDIA` gelir.

use crate::apps::AppId;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use windows::{
    Media::Control::{
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    },
    Win32::{
        Foundation::*,
        System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED},
        UI::WindowsAndMessaging::*,
    },
};

pub const WM_APP_MEDIA: u32 = WM_APP + 12;

/// Komuttan sonra uygulamanın durumunu güncellemesi için tanınan süre.
const SETTLE_MS: u64 = 500;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub app: String,
    pub playing: bool,
    pub can_prev: bool,
    pub can_next: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    PlayPause,
    Next,
    Prev,
}

static SESSIONS: Mutex<Vec<Session>> = Mutex::new(Vec::new());
static SCANNING: AtomicBool = AtomicBool::new(false);

/// Oturum kimliği bu uygulamanın mı? Masaüstü uygulamaları kimlik olarak çoğu
/// zaman exe adını verir ("Spotify.exe"), paketli olanlar AUMID'i.
fn belongs(id: &AppId, session: &str) -> bool {
    let s = session.to_lowercase();
    if s.is_empty() {
        return false;
    }
    if s == id.aumid {
        return true;
    }
    if id.stem.is_empty() {
        return false;
    }
    s.strip_suffix(".exe").unwrap_or(&s) == id.stem || (s.contains('!') && s.contains(&id.stem))
}

/// Uygulamanın son görülen oturumu; beklemez.
pub fn find(id: &AppId) -> Option<Session> {
    SESSIONS.lock().ok()?.iter().find(|s| belongs(id, &s.app)).cloned()
}

fn scan() -> Vec<Session> {
    let Ok(mgr) = Manager::RequestAsync().and_then(|op| op.get()) else { return Vec::new() };
    let Ok(list) = mgr.GetSessions() else { return Vec::new() };
    list.into_iter()
        .filter_map(|s| {
            let info = s.GetPlaybackInfo().ok()?;
            let status = info.PlaybackStatus().ok()?;
            if status != Status::Playing && status != Status::Paused {
                return None;
            }
            let c = info.Controls().ok()?;
            Some(Session {
                app: s.SourceAppUserModelId().ok()?.to_string(),
                playing: status == Status::Playing,
                can_prev: c.IsPreviousEnabled().unwrap_or(false),
                can_next: c.IsNextEnabled().unwrap_or(false),
            })
        })
        .collect()
}

fn store(list: Vec<Session>, notify: isize, always: bool) {
    let Ok(mut cur) = SESSIONS.lock() else { return };
    let changed = *cur != list;
    *cur = list;
    drop(cur);
    if changed || always {
        unsafe {
            let _ = PostMessageW(HWND(notify as _), WM_APP_MEDIA, WPARAM(0), LPARAM(0));
        }
    }
}

/// Oturum listesini arka planda yeniler. Sürmekte olan bir tarama varsa onu bekler.
pub fn refresh(notify: HWND) {
    if SCANNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let notify = notify.0 as isize;
    std::thread::spawn(move || {
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        store(scan(), notify, false);
        SCANNING.store(false, Ordering::SeqCst);
    });
}

/// Komutu yalnızca o uygulamanın oturumuna gönderir, sonra gerçek durumu bildirir:
/// düğme simgesi tıklayınca hemen değiştiği için komut tutmadıysa geri dönsün.
pub fn send(app: String, cmd: Cmd, notify: HWND) {
    let notify = notify.0 as isize;
    std::thread::spawn(move || {
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        let Ok(mgr) = Manager::RequestAsync().and_then(|op| op.get()) else { return };
        let session = mgr
            .GetSessions()
            .ok()
            .and_then(|v| v.into_iter().find(|s| s.SourceAppUserModelId().map(|i| i.to_string() == app).unwrap_or(false)));
        if let Some(s) = session {
            let r = match cmd {
                Cmd::PlayPause => s.TryTogglePlayPauseAsync().and_then(|op| op.get()),
                Cmd::Next => s.TrySkipNextAsync().and_then(|op| op.get()),
                Cmd::Prev => s.TrySkipPreviousAsync().and_then(|op| op.get()),
            };
            match r {
                Ok(true) => {}
                Ok(false) => carlui_core::warn!("medya komutu reddedildi: {cmd:?} ({app})"),
                Err(e) => carlui_core::warn!("medya komutu başarısız: {cmd:?}: {e}"),
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(SETTLE_MS));
        store(scan(), notify, true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(aumid: &str, exe: &str) -> AppId {
        AppId::of_window(aumid, exe)
    }

    #[test]
    fn eslesme() {
        let spotify = app("", r"C:\Users\x\AppData\Roaming\Spotify\Spotify.exe");
        assert!(belongs(&spotify, "Spotify.exe"));
        assert!(belongs(&spotify, "SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify"));
        assert!(!belongs(&spotify, "chrome.exe"));
        assert!(!belongs(&spotify, ""));

        let chrome = app("", r"C:\Program Files\Google\Chrome\Application\chrome.exe");
        assert!(belongs(&chrome, "Chrome"));

        let firefox = app("308046B0AF4A39CB", r"C:\Program Files\Mozilla Firefox\firefox.exe");
        assert!(belongs(&firefox, "308046B0AF4A39CB"));

        // Oturum kimliği boş uygulamayla eşleşmez, ad parçası yetmez.
        assert!(!belongs(&app("", ""), "Spotify.exe"));
        assert!(!belongs(&app("", r"C:\x\spot.exe"), "Spotify.exe"));
    }
}
