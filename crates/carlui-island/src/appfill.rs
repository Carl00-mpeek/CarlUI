//! Masaüstü uygulamalarının giriş ekranında "Vault" düğmesi (Carl Vault ile
//! uygulamada giriş). Kendi iş parçacığında küçük, odak almayan bir pencere:
//! tıklamak öndeki uygulamadan odağı almaz (kullanıcının tıkladığı kutu odakta
//! kalır). Tıklanınca kasaya `Request::AppFill` gider; kasa kapalıysa açılır.
//!
//! Düğme ne zaman görünür (öndeki pencereye her 250 ms bakılır):
//! - Oyun, tam ekran, tarayıcı, CarlUI, masaüstü/görev çubuğu: hiçbir zaman.
//! - Yerel pencere (Win32/WPF/UWP/Qt): UI Automation ile görünür bir parola
//!   kutusu bulunursa, kutunun sağında. Tarama yalnızca ekranın çoğunu
//!   kaplamayan pencerelerde ve en çok 1,5 sn'de bir yapılır.
//! - Chromium/CEF/Electron pencereleri (Steam, Discord, Epic, Riot…) kutularını
//!   göstermez; bunlara UI Automation ile dokunulmaz (erişilebilirliği açıp
//!   uygulamayı yavaşlatabilir). Başlık giriş ekranına benziyorsa ("Steam'e
//!   Giriş Yap", "Sign in") ya da uygulama kasada bağlıyken başlık yalnızca
//!   uygulama adıysa ("Discord") pencerenin üst ortasında görünür. Kullanıcı
//!   önce kullanıcı adı kutusuna tıklar, sonra düğmeye.

use carlui_core::{
    vaultlink::{ask, linked_apps, Request},
    win,
};
use std::{
    cell::RefCell,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::{Com::*, LibraryLoader::GetModuleHandleW},
        UI::{Accessibility::*, HiDpi::GetDpiForWindow, WindowsAndMessaging::*},
    },
};

/// Ayarlardaki Carl Vault → "Uygulamalarda doldurma".
pub static ENABLED: AtomicBool = AtomicBool::new(true);

const TIMER: usize = 1;
const WM_DONE: u32 = WM_APP + 1;
const SCAN_EVERY: Duration = Duration::from_millis(1500);
/// Taraması bu kadar süren pencere bir daha taranmaz (çok büyük ağaç).
const SLOW_SCAN: Duration = Duration::from_millis(400);

/// Düğmenin hiç görünmeyeceği uygulamalar (anahtar; bkz. `win::app_key_of`).
const SKIP: [&str; 12] = ["explorer", "shellexperiencehost", "startmenuexperiencehost", "searchhost", "searchapp", "lockapp", "logonui", "consent", "credentialuibroker", "keepass", "keepassxc", "bitwarden"];
/// Giriş ekranı başlıklarındaki sözcükler (küçük harf).
const LOGIN_WORDS: [&str; 9] = ["giriş", "oturum aç", "sign in", "sign-in", "log in", "login", "anmelden", "connexion", "iniciar sesión"];

#[derive(Clone, Copy, PartialEq)]
enum Look {
    Idle,
    Busy,
    Failed(Instant),
}

struct State {
    btn: HWND,
    ua: Option<IUIAutomation>,
    /// Gösterildiği pencere.
    target: HWND,
    look: Look,
    /// Son yerel tarama: pencere, zaman, bulunan kutu (ekran koordinatı).
    scanned: (HWND, Option<Instant>, Option<RECT>),
    slow: Vec<isize>,
    chromium: (HWND, bool),
    linked: (Vec<String>, Option<Instant>),
    size: (i32, i32),
    scale: f32,
}

thread_local! {
    static ST: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    ST.with(|s| s.borrow_mut().as_mut().map(f))
}

pub fn start() {
    let _ = std::thread::Builder::new().name("appfill".into()).spawn(|| unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let Ok(inst) = GetModuleHandleW(None) else { return };
        let class = w!("CarlUI.VaultAppButton");
        let wc = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: inst.into(), lpszClassName: class, hCursor: LoadCursorW(None, IDC_HAND).unwrap_or_default(), ..Default::default() };
        RegisterClassW(&wc);
        let Ok(btn) = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED, class, w!("Vault"), WS_POPUP, 0, 0, 10, 10, None, None, inst, None) else {
            carlui_core::warn!("uygulama düğmesi penceresi açılamadı");
            return;
        };
        let _ = SetLayeredWindowAttributes(btn, COLORREF(0), 245, LWA_ALPHA);
        let ua: Option<IUIAutomation> = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok();
        if let Some(u2) = ua.as_ref().and_then(|u| u.cast::<IUIAutomation2>().ok()) {
            let _ = u2.SetConnectionTimeout(1000);
            let _ = u2.SetTransactionTimeout(1000);
        }
        ST.with(|s| {
            *s.borrow_mut() = Some(State {
                btn,
                ua,
                target: HWND::default(),
                look: Look::Idle,
                scanned: (HWND::default(), None, None),
                slow: Vec::new(),
                chromium: (HWND::default(), false),
                linked: (Vec::new(), None),
                size: (0, 0),
                scale: 1.0,
            })
        });
        SetTimer(btn, TIMER, 250, None);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}

fn title(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn rect(hwnd: HWND) -> RECT {
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut r);
    }
    r
}

/// Başlık giriş ekranına benziyor mu (Chromium pencereleri için tek ipucu).
fn login_title(title: &str, key: &str, linked: &[String]) -> bool {
    let t = title.trim().to_lowercase();
    if LOGIN_WORDS.iter().any(|w| t.contains(w)) {
        return true;
    }
    // Discord'un giriş ekranının başlığı yalnızca "Discord"; içerideyken "… - Discord".
    // Steam'in ana penceresi de "Steam" olduğundan Steam yalnızca sözcükle.
    let bare = t == key || t == win::app_display(key).to_lowercase();
    bare && key != "steam" && (key == "discord" || linked.iter().any(|k| k == key))
}

impl State {
    fn linked(&mut self) -> &[String] {
        if self.linked.1.is_none_or(|t| t.elapsed() > Duration::from_secs(5)) {
            self.linked = (linked_apps(), Some(Instant::now()));
        }
        &self.linked.0
    }

    /// Yerel penceredeki görünür parola kutusu (ekran koordinatı).
    fn password_field(&mut self, hwnd: HWND) -> Option<RECT> {
        let fresh = self.scanned.0 == hwnd && self.scanned.1.is_some_and(|t| t.elapsed() < SCAN_EVERY);
        if fresh {
            return self.scanned.2;
        }
        if self.slow.contains(&(hwnd.0 as isize)) {
            return None;
        }
        let started = Instant::now();
        let found = self.ua.as_ref().and_then(|ua| unsafe {
            let pass = ua.CreatePropertyCondition(UIA_IsPasswordPropertyId, &VARIANT::from(true)).ok()?;
            let shown = ua.CreatePropertyCondition(UIA_IsOffscreenPropertyId, &VARIANT::from(false)).ok()?;
            let cond = ua.CreateAndCondition(&pass, &shown).ok()?;
            let el = ua.ElementFromHandle(hwnd).ok()?.FindFirst(TreeScope_Descendants, &cond).ok()?;
            if !el.CurrentIsEnabled().ok()?.as_bool() {
                return None;
            }
            let r = el.CurrentBoundingRectangle().ok()?;
            (r.right > r.left && r.bottom > r.top).then_some(r)
        });
        if started.elapsed() > SLOW_SCAN {
            carlui_core::info!("uygulama düğmesi: {} taraması yavaş, bu pencere atlanacak", win::class_name(hwnd));
            if self.slow.len() > 64 {
                self.slow.clear();
            }
            self.slow.push(hwnd.0 as isize);
        }
        self.scanned = (hwnd, Some(Instant::now()), found);
        found
    }

    /// Düğme bu pencerede nerede görünmeli (sol üst köşe, ekran koordinatı).
    fn place(&mut self, fg: HWND) -> Option<(i32, i32)> {
        unsafe {
            if fg.is_invalid() || !IsWindowVisible(fg).as_bool() || IsIconic(fg).as_bool() || GetAncestor(fg, GA_ROOT) != fg {
                return None;
            }
        }
        let class = win::class_name(fg);
        if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") || class.starts_with("CarlUI.") {
            return None;
        }
        let exe = win::window_exe(fg);
        if exe.is_empty() || win::is_browser(&exe) {
            return None;
        }
        let key = win::app_key_of(&exe);
        if key.is_empty() || key.starts_with("carlui") || SKIP.contains(&key.as_str()) {
            return None;
        }
        if win::game_in_front() || win::window_like_game(fg) {
            return None;
        }
        let dpi = unsafe { GetDpiForWindow(fg) };
        self.scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
        let (bw, bh) = ((86.0 * self.scale) as i32, (28.0 * self.scale) as i32);
        let wr = rect(fg);
        if self.chromium.0 != fg {
            self.chromium = (fg, win::is_chromium_window(fg));
        }
        if self.chromium.1 {
            let t = title(fg);
            let linked = self.linked().to_vec();
            if !login_title(&t, &key, &linked) {
                return None;
            }
            return Some((wr.left + (wr.right - wr.left - bw) / 2, wr.top + (8.0 * self.scale) as i32));
        }
        // Yerel: ekranın çoğunu kaplayan pencereler (IDE, ofis…) taranmaz.
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        unsafe {
            let _ = GetMonitorInfoW(MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST), &mut mi);
        }
        let area = |r: RECT| ((r.right - r.left).max(0) as i64) * ((r.bottom - r.top).max(0) as i64);
        if unsafe { IsZoomed(fg) }.as_bool() || area(wr) * 10 > area(mi.rcWork) * 6 {
            return None;
        }
        let f = self.password_field(fg)?;
        let y = f.top + (f.bottom - f.top - bh) / 2;
        let x = if f.right + 6 + bw <= wr.right { f.right + 6 } else { (f.right - bw - 4).max(f.left) };
        Some((x, y))
    }

    fn tick(&mut self) {
        if let Look::Failed(at) = self.look {
            if at.elapsed() > Duration::from_millis(2500) {
                self.look = Look::Idle;
                unsafe {
                    let _ = InvalidateRect(self.btn, None, true);
                }
            }
        }
        if self.look == Look::Busy {
            return;
        }
        let fg = unsafe { GetForegroundWindow() };
        if fg == self.btn {
            return;
        }
        let spot = if ENABLED.load(Ordering::Relaxed) { self.place(fg) } else { None };
        unsafe {
            match spot {
                Some((x, y)) => {
                    self.target = fg;
                    let (w, h) = ((86.0 * self.scale) as i32, (28.0 * self.scale) as i32);
                    if self.size != (w, h) {
                        self.size = (w, h);
                        let rgn = CreateRoundRectRgn(0, 0, w + 1, h + 1, h / 2 * 2 / 2 + 8, h / 2 * 2 / 2 + 8);
                        let _ = SetWindowRgn(self.btn, rgn, true);
                    }
                    let _ = SetWindowPos(self.btn, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
                }
                None => {
                    if IsWindowVisible(self.btn).as_bool() {
                        let _ = ShowWindow(self.btn, SW_HIDE);
                    }
                    self.target = HWND::default();
                }
            }
        }
    }

    fn click(&mut self) {
        if self.look == Look::Busy || self.target.is_invalid() {
            return;
        }
        self.look = Look::Busy;
        unsafe {
            let _ = InvalidateRect(self.btn, None, true);
        }
        let target = self.target.0 as isize;
        let btn = self.btn.0 as isize;
        let _ = std::thread::Builder::new().name("appfill-ask".into()).spawn(move || {
            let req = Request::AppFill { hwnd: target };
            let mut reply = ask(&req);
            if reply.is_err() {
                let exe = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("carlui-vault.exe")));
                if exe.is_some_and(|e| std::process::Command::new(e).spawn().is_ok()) {
                    for _ in 0..30 {
                        std::thread::sleep(Duration::from_millis(250));
                        reply = ask(&req);
                        if reply.is_ok() {
                            break;
                        }
                    }
                }
            }
            let ok = match &reply {
                Ok(r) if r.ok => true,
                Ok(r) => {
                    carlui_core::warn!("uygulama düğmesi: kasa reddetti: {}", r.message);
                    false
                }
                Err(e) => {
                    carlui_core::warn!("uygulama düğmesi: kasaya ulaşılamadı: {e}");
                    false
                }
            };
            unsafe {
                let _ = PostMessageW(HWND(btn as _), WM_DONE, WPARAM(ok as usize), LPARAM(0));
            }
        });
    }

    fn paint(&self, hwnd: HWND) {
        unsafe {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut ps);
            let mut r = RECT::default();
            let _ = GetClientRect(hwnd, &mut r);
            let failed = matches!(self.look, Look::Failed(_));
            let bg = CreateSolidBrush(COLORREF(if failed { 0x002A2A6E } else { 0x00261E1C }));
            FillRect(dc, &r, bg);
            let _ = DeleteObject(bg);
            let edge = CreateSolidBrush(COLORREF(0x00FFC24C));
            let rgn = CreateRoundRectRgn(0, 0, r.right + 1, r.bottom + 1, 16, 16);
            let _ = FrameRgn(dc, rgn, edge, 1, 1);
            let _ = DeleteObject(rgn);
            let _ = DeleteObject(edge);
            SetBkMode(dc, TRANSPARENT);
            let px = |v: f32| -(v * self.scale) as i32;
            let glyph = CreateFontW(px(13.0), 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET.0 as u32, 0, 0, CLEARTYPE_QUALITY.0 as u32, 0, w!("Segoe MDL2 Assets"));
            let text = CreateFontW(px(13.0), 0, 0, 0, 600, 0, 0, 0, DEFAULT_CHARSET.0 as u32, 0, 0, CLEARTYPE_QUALITY.0 as u32, 0, w!("Segoe UI"));
            let pad = (10.0 * self.scale) as i32;
            let gw = (18.0 * self.scale) as i32;
            let old = SelectObject(dc, glyph);
            SetTextColor(dc, COLORREF(0x00FFC24C));
            let mut gr = RECT { left: pad, top: 0, right: pad + gw, bottom: r.bottom };
            let mut g: Vec<u16> = (if failed { "\u{E783}" } else { "\u{E72E}" }).encode_utf16().collect();
            DrawTextW(dc, &mut g, &mut gr, DT_SINGLELINE | DT_VCENTER | DT_CENTER);
            SelectObject(dc, text);
            SetTextColor(dc, COLORREF(0x00F2F2F2));
            let label = match self.look {
                Look::Busy => "…",
                Look::Failed(_) => carlui_core::t!("Olmadı", "Failed"),
                Look::Idle => "Vault",
            };
            let mut tr = RECT { left: pad + gw + (4.0 * self.scale) as i32, top: 0, right: r.right - pad / 2, bottom: r.bottom };
            let mut l: Vec<u16> = label.encode_utf16().collect();
            DrawTextW(dc, &mut l, &mut tr, DT_SINGLELINE | DT_VCENTER | DT_LEFT | DT_END_ELLIPSIS);
            SelectObject(dc, old);
            let _ = DeleteObject(glyph);
            let _ = DeleteObject(text);
            let _ = EndPaint(hwnd, &ps);
        }
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_TIMER if wp.0 == TIMER => {
            with(|s| s.tick());
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            with(|s| s.click());
            LRESULT(0)
        }
        WM_DONE => {
            with(|s| {
                s.look = if wp.0 != 0 { Look::Idle } else { Look::Failed(Instant::now()) };
                if wp.0 != 0 {
                    // Kasa öne geldi ya da yazıyor: düğme yeni öndeki pencereye göre yeniden konur.
                    let _ = ShowWindow(s.btn, SW_HIDE);
                }
                let _ = InvalidateRect(s.btn, None, true);
            });
            LRESULT(0)
        }
        WM_PAINT => {
            if with(|s| s.paint(hwnd)).is_none() {
                let mut ps = PAINTSTRUCT::default();
                BeginPaint(hwnd, &mut ps);
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

#[cfg(test)]
mod tests {
    use super::login_title;

    #[test]
    fn login_titles() {
        let none: Vec<String> = Vec::new();
        assert!(login_title("Steam'e Giriş Yap", "steam", &none));
        assert!(login_title("Sign in to Steam", "steam", &none));
        assert!(!login_title("Steam", "steam", &["steam".into()]), "Steam ana penceresi");
        assert!(login_title("Discord", "discord", &none));
        assert!(!login_title("Friends - Discord", "discord", &none));
        assert!(!login_title("Riot Client", "riotclientux", &none));
        assert!(login_title("Riotclientux", "riotclientux", &["riotclientux".into()]));
        assert!(!login_title("Untitled - Notepad", "notepad", &none));
    }
}
