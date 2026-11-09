//! Bu modül "hangi pencereler açık ve hangisi önde?" sorusunu çözüyor.
//! Açılış/kapanış, gösterme/gizleme, başlık değişimi, gizlenme (cloak; başka
//! sanal masaüstü veya askıya alınmış Store uygulaması), küçültme ve ön plan
//! olaylarına abone olur. Fare hareketi gibi çok sık gelen olaylara abone
//! olmadığı için boşta CPU harcamaz. Olay gelince dock'a tek bir mesaj atılır;
//! dock pencere listesini Alt+Tab'ın kurallarıyla yeniden okur.
//!
//! Pencere öne getirme/küçültme sahte tuş basımı olmadan, pencerenin kendi
//! sistem komutlarıyla yapılır.

use crate::apps::{process_exe_of_window, window_app_id, AppId};
use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, AtomicIsize, Ordering},
};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Dwm::*,
        UI::{Accessibility::*, WindowsAndMessaging::*},
    },
};

pub const WM_APP_WINDOWS: u32 = WM_APP + 10;
pub const WM_APP_LOCATION: u32 = WM_APP + 11;
static NOTIFY: AtomicIsize = AtomicIsize::new(0);
static PENDING: AtomicBool = AtomicBool::new(false);
static PENDING_LOC: AtomicBool = AtomicBool::new(false);

/// Konum mesajı işlendi; bir sonraki konum olayı yeni mesaj atabilir.
pub fn location_handled() {
    PENDING_LOC.store(false, Ordering::SeqCst);
}

#[derive(Clone, Debug)]
pub struct Window {
    pub hwnd: HWND,
    pub app: AppId,
}

pub struct Tracker {
    cache: HashMap<isize, AppId>,
    pub windows: Vec<Window>,
    pub foreground: HWND,
    hooks: Vec<HWINEVENTHOOK>,
    location_hooks: Vec<HWINEVENTHOOK>,
    own_pid: u32,
    /// Intellihide: öndeki sürecin konum olayları dinleniyor mu, hangi süreç.
    watching: bool,
    follow_hook: HWINEVENTHOOK,
    follow_pid: u32,
}

unsafe extern "system" fn on_event(_: HWINEVENTHOOK, event: u32, hwnd: HWND, id_object: i32, id_child: i32, _: u32, _: u32) {
    if event != EVENT_SYSTEM_FOREGROUND && (id_object != OBJID_WINDOW.0 || id_child != 0 || hwnd.is_invalid()) {
        return;
    }
    // Konum değişimleri yalnızca intellihide için: pencere listesini yeniden okumaya gerek yok.
    if event == EVENT_OBJECT_LOCATIONCHANGE || event == EVENT_SYSTEM_MOVESIZEEND {
        if !PENDING_LOC.swap(true, Ordering::SeqCst) {
            let _ = PostMessageW(HWND(NOTIFY.load(Ordering::SeqCst) as _), WM_APP_LOCATION, WPARAM(0), LPARAM(0));
        }
        return;
    }
    // Aynı anda gelen olay yığınını tek bir yenilemede birleştir.
    if !PENDING.swap(true, Ordering::SeqCst) {
        let _ = PostMessageW(HWND(NOTIFY.load(Ordering::SeqCst) as _), WM_APP_WINDOWS, WPARAM(0), LPARAM(0));
    }
}

const IGNORED_CLASSES: [&str; 7] = [
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "Windows.UI.Core.CoreWindow",
    "XamlExplorerHostIslandWindow",
    "TopLevelWindowForOverflowXamlIsland",
];

pub fn title(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn is_cloaked(hwnd: HWND) -> bool {
    let mut v = 0u32;
    unsafe {
        DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut v as *mut _ as *mut _, 4).is_ok() && v != 0
    }
}

/// Alt+Tab'da görünecek türden bir uygulama penceresi mi?
fn is_app_window(hwnd: HWND, own_pid: u32) -> bool {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || GetWindowTextLengthW(hwnd) == 0 {
            return false;
        }
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        let app_window = ex & WS_EX_APPWINDOW.0 != 0;
        if !app_window && (ex & WS_EX_TOOLWINDOW.0 != 0 || ex & WS_EX_NOACTIVATE.0 != 0) {
            return false;
        }
        if !app_window && GetWindow(hwnd, GW_OWNER).is_ok_and(|o| !o.is_invalid()) {
            return false;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == own_pid {
            return false;
        }
        let class = carlui_core::win::class_name(hwnd);
        // CarlUI'nin yüzen katmanları dock'ta görünmez; görev yöneticisi ve
        // ayarlar ise gerçek uygulama pencereleri.
        let own_app = matches!(class.as_str(), "CarlUI.TaskMgr" | "CarlUI.Settings");
        if IGNORED_CLASSES.contains(&class.as_str()) || class.starts_with("CarlUI.") && !own_app {
            return false;
        }
        !is_cloaked(hwnd)
    }
}

fn app_of(hwnd: HWND) -> AppId {
    AppId::of_window(&window_app_id(hwnd), &process_exe_of_window(hwnd))
}

impl Tracker {
    pub fn new(notify: HWND) -> Self {
        NOTIFY.store(notify.0 as isize, Ordering::SeqCst);
        let ranges = [
            (EVENT_OBJECT_CREATE, EVENT_OBJECT_HIDE),
            (EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_NAMECHANGE),
            (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
            (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
            (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
        ];
        let hooks = ranges
            .iter()
            .map(|&(a, b)| unsafe { SetWinEventHook(a, b, None, Some(on_event), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS) })
            .collect();
        let mut t = Self {
            cache: HashMap::new(),
            windows: Vec::new(),
            foreground: HWND::default(),
            hooks,
            location_hooks: Vec::new(),
            own_pid: std::process::id(),
            watching: false,
            follow_hook: HWINEVENTHOOK::default(),
            follow_pid: 0,
        };
        t.refresh();
        t
    }

    /// Intellihide açıkken pencere taşıma/boyutlandırma olaylarını da dinle.
    /// (Konum olayları sık geldiği için yalnızca gerektiğinde açılır.)
    pub fn watch_locations(&mut self, on: bool) {
        for h in self.location_hooks.drain(..) {
            unsafe {
                let _ = UnhookWinEvent(h);
            }
        }
        self.unhook_follow();
        self.watching = on;
        if on {
            // Sürükleyip bırakma nadirdir: tüm sistemde dinlenebilir.
            let h = unsafe { SetWinEventHook(EVENT_SYSTEM_MOVESIZEEND, EVENT_SYSTEM_MOVESIZEEND, None, Some(on_event), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS) };
            self.location_hooks.push(h);
            self.follow_foreground();
        }
    }

    /// Konum değişimini yalnızca öndeki uygulamanın sürecinde dinler. Tüm
    /// sistemi dinlemek imleç, yazı imleci ve her pencere kıpırtısını dock'a
    /// taşıyordu (boşta bile sürekli uyanma). Taşınan/büyütülen pencere zaten
    /// öndekidir; ön plan değişince (liste yenilenirken) kanca taşınır.
    fn follow_foreground(&mut self) {
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
        }
        if pid == self.follow_pid {
            return;
        }
        self.unhook_follow();
        self.follow_pid = pid;
        if pid != 0 && pid != self.own_pid {
            self.follow_hook = unsafe { SetWinEventHook(EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE, None, Some(on_event), pid, 0, WINEVENT_OUTOFCONTEXT) };
        }
    }

    fn unhook_follow(&mut self) {
        if !self.follow_hook.is_invalid() {
            unsafe {
                let _ = UnhookWinEvent(self.follow_hook);
            }
        }
        self.follow_hook = HWINEVENTHOOK::default();
        self.follow_pid = 0;
    }

    /// Pencere listesini yeniden okur. Mevcut pencerelerin sırası korunur, yeniler sona eklenir.
    pub fn refresh(&mut self) {
        PENDING.store(false, Ordering::SeqCst);
        let mut found: Vec<HWND> = Vec::new();
        unsafe extern "system" fn cb(h: HWND, data: LPARAM) -> BOOL {
            (*(data.0 as *mut Vec<HWND>)).push(h);
            TRUE
        }
        unsafe {
            let _ = EnumWindows(Some(cb), LPARAM(&mut found as *mut _ as isize));
        }
        let live: Vec<HWND> = found.into_iter().filter(|&h| is_app_window(h, self.own_pid)).collect();

        let mut next: Vec<Window> = self
            .windows
            .iter()
            .filter(|w| live.contains(&w.hwnd))
            .cloned()
            .collect();
        for &h in &live {
            if !next.iter().any(|w| w.hwnd == h) {
                let app = self.cache.entry(h.0 as isize).or_insert_with(|| app_of(h)).clone();
                next.push(Window { hwnd: h, app });
            }
        }
        self.cache.retain(|k, _| live.iter().any(|h| h.0 as isize == *k));
        self.windows = next;
        self.foreground = unsafe { GetForegroundWindow() };
        if self.watching {
            self.follow_foreground();
        }
    }
}

impl Drop for Tracker {
    fn drop(&mut self) {
        for &h in self.hooks.iter().chain(&self.location_hooks) {
            unsafe {
                let _ = UnhookWinEvent(h);
            }
        }
        self.unhook_follow();
    }
}

pub fn is_minimized(hwnd: HWND) -> bool {
    unsafe { IsIconic(hwnd).as_bool() }
}

/// Pencereyi öne getirir (küçültülmüşse geri yükler). Dock tıklamayı aldığı için
/// Windows, son girdiyi alan süreç olarak ön plan değiştirmemize izin verir.
pub fn activate(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            // Küçültülmüş pencere: Alt+Tab'ın yolu. Yönetici olarak çalışan
            // pencereye (çoğu oyun istemcisi) ShowWindow ulaşmayabiliyor; eskiden
            // pencere öne geçip simge durumunda kalıyordu. Ağır yük altındaki
            // oyun geri yüklemeyi geç işlese de istek kuyrukta bekler.
            SwitchToThisWindow(hwnd, true);
            let _ = ShowWindowAsync(hwnd, SW_RESTORE);
        }
        // Pencerenin açık bir iletişim kutusu varsa o öne gelsin (giriş ekranı vb.).
        let popup = GetLastActivePopup(hwnd);
        let target = if !popup.is_invalid() && popup != hwnd && IsWindowVisible(popup).as_bool() { popup } else { hwnd };
        if GetForegroundWindow() != target {
            carlui_core::win::force_foreground(target);
        }
        if GetForegroundWindow() != target && GetForegroundWindow() != hwnd {
            SwitchToThisWindow(hwnd, true);
        }
    }
}

/// Uygulamanın kendi küçültme komutu (animasyonu ve durumu uygulama yönetir).
pub fn minimize(hwnd: HWND) {
    unsafe {
        let _ = PostMessageW(hwnd, WM_SYSCOMMAND, WPARAM(SC_MINIMIZE as usize), LPARAM(0));
    }
}

pub fn close(hwnd: HWND) {
    unsafe {
        let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
    }
}

/// Pencerenin büyük ikonu (shell ikonu bulunamazsa son çare).
pub fn window_icon(hwnd: HWND) -> HICON {
    unsafe {
        let mut res = 0usize;
        let _ = SendMessageTimeoutW(hwnd, WM_GETICON, WPARAM(ICON_BIG as usize), LPARAM(0), SMTO_ABORTIFHUNG, 100, Some(&mut res));
        if res == 0 {
            res = GetClassLongPtrW(hwnd, GCLP_HICON);
        }
        HICON(res as _)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::w;

    fn pump(ms: u64) {
        let end = std::time::Instant::now() + std::time::Duration::from_millis(ms);
        while std::time::Instant::now() < end {
            unsafe {
                let mut m = MSG::default();
                while PeekMessageW(&mut m, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&m);
                    DispatchMessageW(&m);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// Elle: `cargo test -p carlui-dock uc_pencere -- --ignored`. Aynı uygulamanın
    /// üç penceresi (kendi test pencerelerimiz, 2. ekranda) küçültülmüşken her
    /// biri ayrı ayrı öne gelip açılmalı.
    #[test]
    #[ignore]
    fn uc_pencere_kucultulmusken_acilir() {
        unsafe {
            let wins: Vec<HWND> = (0..3)
                .map(|i| CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), w!("CarlUI test"), WS_OVERLAPPEDWINDOW | WS_VISIBLE, 2000 + i * 260, 200, 240, 160, None, None, None, None).unwrap())
                .collect();
            pump(200);
            for &h in &wins {
                let _ = ShowWindow(h, SW_MINIMIZE);
            }
            pump(300);
            for (i, &h) in wins.iter().enumerate() {
                assert!(IsIconic(h).as_bool());
                activate(h);
                pump(400);
                assert!(!IsIconic(h).as_bool(), "{i}. pencere açılmadı");
                assert_eq!(GetForegroundWindow(), h, "{i}. pencere öne gelmedi");
                let _ = ShowWindow(h, SW_MINIMIZE);
                pump(300);
            }
            for h in wins {
                let _ = DestroyWindow(h);
            }
        }
    }
}
