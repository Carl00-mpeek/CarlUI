//! Bu modül oyundan atılmaların kimden geldiğini günlüğe yazar. Ön plan
//! değişimini dinler; yalnızca öndeki pencere bir oyun / tam ekran pencereyken
//! odak başka yere geçerse tek satır yazar: yeni pencerenin exe adı, pid'i,
//! sınıfı, monitörü ve CarlUI'ye ait olup olmadığı; önceki oyunun exe'si ve
//! sınıfı. Pencere başlığı yazılmaz (gizlilik). Oyun yokken hiçbir şey yazılmaz.
//! `ForegroundStaging` gibi geçiş yüzeyleri kendi başına yazılmaz; ardından
//! gelen gerçek pencerenin satırında "arada" olarak anılır.

use carlui_core::{info, win};
use std::cell::RefCell;
use windows::Win32::{
    Foundation::HWND,
    UI::{Accessibility::*, WindowsAndMessaging::*},
};

/// Odak geçişinde araya giren kabuk yüzeyleri (Windows'un geçiş sahnesi, Alt+Tab).
const TRANSIENT: [&str; 3] = ["ForegroundStaging", "XamlExplorerHostIslandWindow", "MultitaskingViewFrame"];

/// Son gerçek ön plan penceresi.
struct Front {
    hwnd: isize,
    exe: String,
    class: String,
    monitor: String,
    /// Oyun / tam ekran penceresi miydi?
    game: bool,
}

#[derive(Default)]
struct State {
    last: Option<Front>,
    /// Son gerçek pencereden beri görülen geçiş yüzeyleri.
    via: Vec<&'static str>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}

/// Dinlemeyi kurar (mesaj döngüsü olan iş parçacığında çağrılmalı). Kendi
/// sürecimizin pencereleri de görülsün diye `WINEVENT_SKIPOWNPROCESS` yok.
pub fn install() -> HWINEVENTHOOK {
    let fg = unsafe { GetForegroundWindow() };
    STATE.with(|s| s.borrow_mut().last = Some(describe(fg)));
    unsafe { SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(on_foreground), 0, 0, WINEVENT_OUTOFCONTEXT) }
}

/// Tam ekran izleyicisi öndeki pencereyi tam ekran buldu: oyun bayrağını tazele
/// (oyun açılırken pencere ilk ön plana geçtiğinde henüz tam boy olmayabilir).
pub fn note_fullscreen() {
    let fg = unsafe { GetForegroundWindow() }.0 as isize;
    STATE.with(|s| {
        if let Some(l) = s.borrow_mut().last.as_mut().filter(|l| l.hwnd == fg) {
            l.game = true;
        }
    });
}

unsafe extern "system" fn on_foreground(_: HWINEVENTHOOK, _: u32, hwnd: HWND, _: i32, _: i32, _: u32, _: u32) {
    let hwnd = if hwnd.is_invalid() { GetForegroundWindow() } else { hwnd };
    if hwnd.is_invalid() {
        return;
    }
    let class = win::class_name(hwnd);
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if let Some(t) = TRANSIENT.iter().find(|t| **t == class) {
            if !s.via.contains(t) {
                s.via.push(t);
            }
            return;
        }
        let via = std::mem::take(&mut s.via);
        let prev = s.last.take();
        let now = describe_with(hwnd, class);
        if let Some(p) = prev.filter(|p| p.hwnd != now.hwnd) {
            // Oyun odağı kaybedince çoğu zaman küçülür; bayrak kaçmışsa şimdiki hâline de bak.
            let was_game = p.game || win::window_like_game(HWND(p.hwnd as _));
            if was_game {
                log_leave(hwnd, &now, &p, &via);
            }
        }
        s.last = Some(now);
    });
}

fn log_leave(hwnd: HWND, now: &Front, prev: &Front, via: &[&str]) {
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    let carlui = if pid == std::process::id() || exe_file(&now.exe).starts_with("carlui") || now.class.starts_with("CarlUI.") { ", CarlUI süreci" } else { "" };
    let via = if via.is_empty() { String::new() } else { format!(" · arada: {}", via.join(", ")) };
    info!(
        "odak oyundan gitti → {} (pid {pid}, sınıf {}, monitör {}{carlui}) · önceki: {} ({}, monitör {}){via}",
        exe_file(&now.exe),
        now.class,
        now.monitor,
        exe_file(&prev.exe),
        prev.class,
        prev.monitor,
    );
}

fn describe(hwnd: HWND) -> Front {
    describe_with(hwnd, win::class_name(hwnd))
}

fn describe_with(hwnd: HWND, class: String) -> Front {
    Front { hwnd: hwnd.0 as isize, exe: win::window_exe(hwnd), monitor: win::monitor_name(hwnd), game: win::window_like_game(hwnd), class }
}

/// Exe yolundan yalnızca dosya adı; erişilemeyen (yönetici) süreçte "?".
fn exe_file(path: &str) -> &str {
    match path.rsplit('\\').next() {
        Some(n) if !n.is_empty() => n,
        _ => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_file_takes_name() {
        assert_eq!(exe_file(r"c:\games\cs2\cs2.exe"), "cs2.exe");
        assert_eq!(exe_file(""), "?");
        assert_eq!(exe_file("chrome.exe"), "chrome.exe");
    }
}
