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
//! - Chromium/CEF/Electron pencereleri (Steam, Discord, Epic, Riot…) önce
//!   başlıklarına göre süzülür: başlık giriş ekranına benziyorsa ("Steam'e
//!   Giriş Yap", "Sign in") ya da yalnızca uygulama adıysa ("Discord",
//!   Discord'un giriş ekranında " - Discord"; diğer uygulamalarda kasada
//!   bağlıyken) ya da sayfa adı yerine adres yazıyorsa ("discord.com/app?_=…
//!   - Discord": Discord giriş sayfasına başlık vermez). Yalnızca bu
//!   pencerelerde UI Automation'a parola kutusu sorulur (Chromium
//!   erişilebilirlik ağacını ilk istekte kurar; uygulamayı yavaşlatmamak için
//!   başka Chromium pencerelerine sorulmaz). Kutu bulunursa düğme yanında;
//!   bulunamazsa başlık giriş ekranını açıkça söylüyorsa üst ortada, adres
//!   başlığında hiç görünmez (yükleniyor ekranı olabilir).

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
    /// En az bir kez taranmış pencereler.
    warm: Vec<isize>,
    /// Son günlüğe yazılan Chromium adayı (pencere, başlık kesin mi, kutu var mı).
    noted: Option<(isize, bool, bool)>,
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
                warm: Vec::new(),
                noted: None,
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
    // Discord başlığı "<sayfa> - Discord" biçiminde yazar; giriş ekranında sayfa adı
    // boş olduğundan başlık " - Discord" olur (eski sürümlerde yalnızca "Discord").
    // İçerideyken "Friends - Discord" gibi. PTB/Canary: "Discord PTB" → `discordptb`.
    // Steam'in ana penceresi de "Steam" olduğundan Steam yalnızca sözcükle.
    let b = t.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '-' | '–' | '—' | '|' | '•' | '·' | ':'));
    let squashed: String = b.chars().filter(|c| !c.is_whitespace()).collect();
    let bare = !b.is_empty() && (b == key || squashed == key || b == win::app_display(key).to_lowercase());
    bare && key != "steam" && (key.starts_with("discord") || linked.iter().any(|k| k == key))
}

/// "<sayfa> - <Uygulama>" başlığında sayfa adı bir adres mi
/// ("discord.com/app?_=1795366828405 - Discord"). Sayfa kendine başlık
/// vermediğinde Chromium adresi yazar: Discord'un giriş ekranı böyle görünür
/// ama yükleniyor ekranı da öyle; bu yüzden yalnızca parola kutusu bulunursa
/// giriş ekranı sayılır.
fn url_title(title: &str) -> bool {
    let t = title.trim().to_lowercase();
    let page = t.rsplit_once(" - ").map_or(t.as_str(), |(p, _)| p).trim();
    !page.is_empty() && !page.contains(char::is_whitespace) && page.contains('.') && page.contains('/')
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
        // Chromium erişilebilirlik ağacını ilk istekte kurar: o tarama yavaş ve boş
        // olabilir. İlk taramada pencere atlanacaklara konmaz ve boş sonuç kısa tutulur.
        let first = !self.warm.contains(&(hwnd.0 as isize));
        if first {
            if self.warm.len() > 64 {
                self.warm.clear();
            }
            self.warm.push(hwnd.0 as isize);
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
        if started.elapsed() > SLOW_SCAN && !first {
            carlui_core::info!("uygulama düğmesi: {} taraması yavaş, bu pencere atlanacak", win::class_name(hwnd));
            if self.slow.len() > 64 {
                self.slow.clear();
            }
            self.slow.push(hwnd.0 as isize);
        }
        // İlk tarama boşsa 300 ms sonra yeniden bakılır (1,5 sn yerine).
        let at = if first && found.is_none() { Instant::now().checked_sub(SCAN_EVERY - Duration::from_millis(300)) } else { None };
        self.scanned = (hwnd, Some(at.unwrap_or_else(Instant::now)), found);
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
        let beside = |f: RECT| {
            let y = f.top + (f.bottom - f.top - bh) / 2;
            let x = if f.right + 6 + bw <= wr.right { f.right + 6 } else { (f.right - bw - 4).max(f.left) };
            (x, y)
        };
        if self.chromium.1 {
            let t = title(fg);
            let linked = self.linked().to_vec();
            let sure = login_title(&t, &key, &linked);
            if !sure && !url_title(&t) {
                return None;
            }
            // Giriş ekranına benzeyen pencere: kutuyu sor. Chromium ağacı ilk istekte
            // kurulur; ilk tarama boş dönebilir, sonraki taramada kutu bulunur.
            let field = self.password_field(fg).filter(|f| f.left >= wr.left && f.right <= wr.right && f.top >= wr.top && f.bottom <= wr.bottom);
            let note = (fg.0 as isize, sure, field.is_some());
            if self.noted != Some(note) {
                self.noted = Some(note);
                carlui_core::info!("uygulama düğmesi: {key} giriş ekranı adayı ({t:?}): başlık kesin={sure}, parola kutusu={}", field.is_some());
            }
            if let Some(f) = field {
                return Some(beside(f));
            }
            // Başlık adresse (yükleniyor ekranı da olabilir) kutu olmadan gösterilmez.
            return sure.then(|| (wr.left + (wr.right - wr.left - bw) / 2, wr.top + (8.0 * self.scale) as i32));
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
        self.password_field(fg).map(beside)
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
    use super::{login_title, url_title};

    /// Canlı pencerede adım adım: `CARLUI_HWND=<ondalık> cargo test -p carlui-island live_window -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_window() {
        use super::*;
        let h: isize = std::env::var("CARLUI_HWND").unwrap().parse().unwrap();
        let hwnd = HWND(h as _);
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let ua: Option<IUIAutomation> = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }.ok();
        let mut s = State { btn: HWND::default(), ua, target: HWND::default(), look: Look::Idle, scanned: (HWND::default(), None, None), slow: Vec::new(), warm: Vec::new(), noted: None, chromium: (HWND::default(), false), linked: (Vec::new(), None), size: (0, 0), scale: 1.0 };
        let t = title(hwnd);
        let exe = win::window_exe(hwnd);
        println!("title={t:?} exe={exe} key={} chromium={} game={} game_front={}", win::app_key_of(&exe), win::is_chromium_window(hwnd), win::window_like_game(hwnd), win::game_in_front());
        println!("login_title={} url_title={}", login_title(&t, &win::app_key_of(&exe), &[]), url_title(&t));
        for i in 0..4 {
            s.scanned.1 = None;
            let started = Instant::now();
            println!("scan {i}: {:?} {:?}", s.password_field(hwnd).map(|r| (r.left, r.top, r.right, r.bottom)), started.elapsed());
            std::thread::sleep(Duration::from_millis(300));
        }
        s.scanned.1 = None;
        println!("place={:?}", s.place(hwnd));
        unsafe {
            let ua = s.ua.as_ref().unwrap();
            let edits = ua.CreatePropertyCondition(UIA_ControlTypePropertyId, &VARIANT::from(UIA_EditControlTypeId.0)).unwrap();
            let list = ua.ElementFromHandle(hwnd).unwrap().FindAll(TreeScope_Descendants, &edits).unwrap();
            for i in 0..list.Length().unwrap() {
                let e = list.GetElement(i).unwrap();
                println!("edit {i}: {:?} pw={:?} off={:?}", e.CurrentName().map(|n| n.to_string()), e.CurrentIsPassword().map(|b| b.as_bool()), e.CurrentIsOffscreen().map(|b| b.as_bool()));
            }
            // Gerçek Win32 parola kutusu: koşul çalışıyor mu.
            let host = CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), w!("pwtest"), WS_OVERLAPPEDWINDOW | WS_VISIBLE, 100, 100, 300, 200, None, None, None, None).unwrap();
            let _pw = CreateWindowExW(WINDOW_EX_STYLE(0), w!("EDIT"), w!(""), WS_CHILD | WS_VISIBLE | WINDOW_STYLE(0x20), 10, 10, 200, 24, host, None, None, None).unwrap();
            println!("win32 password: {:?}", s.password_field(host).map(|r| (r.left, r.top, r.right, r.bottom)));
            let _ = DestroyWindow(host);
        }
    }

    #[test]
    fn url_titles() {
        assert!(url_title("discord.com/app?_=1795366828405 - Discord"), "Discord giriş ekranı");
        assert!(url_title("https://discord.com/login - Discord"));
        assert!(!url_title("Friends - Discord"));
        assert!(!url_title("#genel | Sunucu - Discord"));
        assert!(!url_title(" - Discord"));
        assert!(!url_title("Steam"));
    }

    #[test]
    fn login_titles() {
        let none: Vec<String> = Vec::new();
        assert!(login_title("Steam'e Giriş Yap", "steam", &none));
        assert!(login_title("Sign in to Steam", "steam", &none));
        assert!(!login_title("Steam", "steam", &["steam".into()]), "Steam ana penceresi");
        assert!(login_title("Discord", "discord", &none));
        assert!(login_title(" - Discord", "discord", &none), "güncel Discord giriş ekranı");
        assert!(login_title(" - Discord PTB", "discordptb", &none));
        assert!(!login_title("Friends - Discord", "discord", &none));
        assert!(!login_title("#genel | Sunucu - Discord", "discord", &none));
        assert!(!login_title(" - ", "discord", &none));
        assert!(!login_title(" - Steam", "steam", &["steam".into()]));
        assert!(!login_title("Riot Client", "riotclientux", &none));
        assert!(login_title("Riotclientux", "riotclientux", &["riotclientux".into()]));
        assert!(!login_title("Untitled - Notepad", "notepad", &none));
    }
}
