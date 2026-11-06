//! Carl Vault penceresi (`carlui-vault.exe`). Ayarlar ve görev yöneticisiyle
//! aynı akrilik cam pencere. Tek örnek çalışır: ikinci kez açılınca mevcut
//! pencere öne gelir. Kasa şu durumlarda kendiliğinden kilitlenir: belirlenen
//! süre boyunca klavye/fare kullanılmazsa, bilgisayar uykuya geçerken ve
//! Windows oturumu kilitlenirken. Pencere kapanınca süreç biter; ana anahtar
//! bellekle birlikte silinir. Ana süreç (`carlui.exe`) kısayolla bu exe'yi açar.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod dialog;
mod inject;
mod ui;

use app::{App, View};
use carlui_core::{config, config::Config, info, log};
use carlui_ui::{
    glassmenu,
    paint::{Align, Fonts, Painter, R, TEXT},
    tween,
};
use std::{
    cell::RefCell,
    sync::{mpsc, Arc, Mutex},
    time::Instant,
};
use windows::{
    core::*,
    Foundation::Numerics::Matrix3x2,
    Win32::{
        Foundation::*,
        Graphics::{
            Direct2D::{Common::*, *},
            Dwm::*,
            Dxgi::Common::*,
            Gdi::*,
        },
        System::{
            Com::*,
            LibraryLoader::GetModuleHandleW,
            RemoteDesktop::{WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION},
            Threading::CreateMutexW,
        },
        UI::{Controls::MARGINS, HiDpi::*, WindowsAndMessaging::*},
    },
};

const CLASS: PCWSTR = w!("CarlUI.Vault");
const WM_APP_RELOAD: u32 = WM_APP + 1;
/// Adadan yapıştırma isteği geldi (`LINK` kuyruğunda).
const WM_APP_LINK: u32 = WM_APP + 2;
const TIMER_ANIM: usize = 2;
const TIMER_IDLE: usize = 4;
const WTS_SESSION_LOCK: usize = 7;
const PBT_APMSUSPEND: usize = 4;

struct Win {
    app: App,
    rt: Option<ID2D1HwndRenderTarget>,
    factory: ID2D1Factory,
    fonts: Fonts,
    scale: f32,
    mouse: (f32, f32),
    hits: Vec<(R, app::Act)>,
    accent: u32,
    reload: Arc<Mutex<Option<Config>>>,
}

thread_local! {
    static WIN: RefCell<Option<Win>> = const { RefCell::new(None) };
}

/// Boru iş parçacığından arayüz iş parçacığına: istek, istekçinin süreci, yanıt kanalı.
type LinkJob = (carlui_core::vaultlink::Request, u32, mpsc::Sender<carlui_core::vaultlink::Reply>);
static LINK: Mutex<Vec<LinkJob>> = Mutex::new(Vec::new());

/// Adanın isteklerini dinler; her isteği arayüz iş parçacığında `App::link` yanıtlar.
fn start_link(hwnd: HWND) {
    let h = hwnd.0 as isize;
    carlui_core::vaultlink::serve(move |req, pid| {
        let (tx, rx) = mpsc::channel();
        if let Ok(mut q) = LINK.lock() {
            q.push((req, pid, tx));
        }
        unsafe {
            let _ = PostMessageW(HWND(h as _), WM_APP_LINK, WPARAM(0), LPARAM(0));
        }
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .unwrap_or_else(|_| carlui_core::vaultlink::Reply::no(carlui_core::t!("Kasa yanıt vermedi.", "The vault didn't answer.")))
    });
}

fn with<T>(f: impl FnOnce(&mut Win) -> T) -> Option<T> {
    WIN.with(|a| a.try_borrow_mut().ok().and_then(|mut a| a.as_mut().map(f)))
}

fn capture_affinity(hwnd: HWND, cfg: &Config) {
    unsafe {
        let _ = SetWindowDisplayAffinity(hwnd, if cfg.vault.hide_from_capture { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE });
    }
}

impl Win {
    fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(self.app.hwnd, None, false);
        }
    }

    fn hit_at(&self, pt: (f32, f32)) -> Option<app::Act> {
        self.hits.iter().rev().filter(|(r, a)| r.contains(pt) && !matches!(a, app::Act::Scroll)).map(|(_, a)| a.clone()).next()
    }

    fn paint(&mut self) -> Result<()> {
        let hwnd = self.app.hwnd;
        let mut rc = RECT::default();
        unsafe { GetClientRect(hwnd, &mut rc)? };
        let (pw, ph) = ((rc.right - rc.left).max(1) as u32, (rc.bottom - rc.top).max(1) as u32);
        if self.rt.is_none() {
            let props = D2D1_RENDER_TARGET_PROPERTIES { pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED }, dpiX: 96.0, dpiY: 96.0, ..Default::default() };
            let hp = D2D1_HWND_RENDER_TARGET_PROPERTIES { hwnd, pixelSize: D2D_SIZE_U { width: pw, height: ph }, presentOptions: D2D1_PRESENT_OPTIONS_NONE };
            let rt = unsafe { self.factory.CreateHwndRenderTarget(&props, &hp)? };
            unsafe { rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE) };
            self.rt = Some(rt);
        }
        let rt = self.rt.clone().unwrap();
        unsafe {
            let size = rt.GetPixelSize();
            if size.width != pw || size.height != ph {
                rt.Resize(&D2D_SIZE_U { width: pw, height: ph })?;
            }
        }
        let s = self.scale;
        let (w, h) = (pw as f32 / s, ph as f32 / s);
        let mut hits = std::mem::take(&mut self.hits);
        hits.clear();
        tween::frame();
        carlui_ui::paint::hover_frame();
        unsafe {
            rt.BeginDraw();
            rt.SetTransform(&Matrix3x2 { M11: s, M12: 0.0, M21: 0.0, M22: s, M31: 0.0, M32: 0.0 });
            rt.Clear(Some(&carlui_ui::tone::backdrop()));
        }
        {
            let base: ID2D1RenderTarget = rt.cast()?;
            let mut p = Painter { rt: &base, fonts: &self.fonts, mouse: self.mouse, hits: &mut hits, accent: self.accent };
            let over = ui::paint(&self.app, &mut p, w, h);
            self.app.max_scroll = over;
            if self.app.scroll > over {
                self.app.scroll = over;
            }

            // Bilgi: alttan kayarak gelir, 2,5 sn sonra gider.
            let shown = self.app.notice.as_ref().is_some_and(|(_, at)| at.elapsed().as_millis() < 2500);
            let t = tween::to(tween::key("toast"), if shown { 1.0 } else { 0.0 }, 14.0);
            if t > 0.01 {
                if let Some((m, _)) = &self.app.notice {
                    let tw = p.fonts.width(m, 13.0, false) + 40.0;
                    let r = R::new((w - tw) / 2.0, h - 52.0 + 24.0 * (1.0 - t), tw, 34.0);
                    p.round(r, 17.0, 0x202024, 0.96 * t);
                    p.outline(r, 17.0, 0xFFFFFF, 0.12 * t);
                    p.glyph('\u{E73E}', R::new(r.x + 12.0, r.y, 16.0, r.h), 12.0, self.accent, t);
                    p.text(m, R::new(r.x + 30.0, r.y, r.w - 38.0, r.h), 13.0, false, TEXT, 0.95 * t, Align::Left);
                }
            }
        }
        self.hits = hits;
        unsafe {
            if rt.EndDraw(None, None).is_err() {
                self.rt = None;
            }
            let toast = self.app.notice.as_ref().is_some_and(|(_, at)| at.elapsed().as_millis() < 2600);
            if tween::busy() || toast {
                SetTimer(hwnd, TIMER_ANIM, 16, None);
            } else if self.app.focus.is_some() || self.app.wait_left().is_some() {
                // İmleç yanıp sönsün / bekleme süresi saysın.
                SetTimer(hwnd, TIMER_ANIM, 530, None);
            } else if self.app.ticking() {
                // 2FA kodu ve geri sayımı.
                SetTimer(hwnd, TIMER_ANIM, 1000, None);
            }
        }
        Ok(())
    }
}

fn xy(lp: LPARAM) -> (f32, f32) {
    ((lp.0 & 0xFFFF) as i16 as f32, ((lp.0 >> 16) & 0xFFFF) as i16 as f32)
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            BeginPaint(hwnd, &mut ps);
            with(|a| a.paint().ok());
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_SIZE => {
            with(|a| a.invalidate());
            LRESULT(0)
        }
        WM_TIMER => {
            match wp.0 {
                TIMER_IDLE => {
                    with(|a| {
                        a.app.check_idle();
                        a.invalidate();
                    });
                }
                app::TIMER_CLIP => {
                    let _ = KillTimer(hwnd, wp.0);
                    with(|a| a.app.clear_clipboard());
                }
                _ => {
                    let _ = KillTimer(hwnd, wp.0);
                    with(|a| a.invalidate());
                }
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            with(|a| {
                let (x, y) = xy(lp);
                let pt = (x / a.scale, y / a.scale);
                // Yalnızca bir vurgu değişiyorsa çiz (her harekette çizmek GPU yorar).
                if carlui_ui::paint::hover_changed(&a.hits, a.mouse, pt) {
                    a.invalidate();
                }
                a.mouse = pt;
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            with(|a| {
                let (x, y) = xy(lp);
                a.mouse = (x / a.scale, y / a.scale);
                a.app.caret_since = Instant::now();
                match a.hit_at(a.mouse) {
                    Some(act) => {
                        // Uzun süren işlemler (Argon2) sırasında bekleme imleci.
                        let slow = matches!(act, app::Act::Unlock | app::Act::Create | app::Act::SetPassword | app::Act::ChangePassword);
                        if slow {
                            if let Ok(c) = LoadCursorW(None, IDC_WAIT) {
                                SetCursor(c);
                            }
                        }
                        a.app.act(act);
                    }
                    None => {
                        if matches!(a.app.view, View::List | View::Edit(_) | View::Security) {
                            a.app.focus = None;
                        }
                    }
                }
                a.invalidate();
            });
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            with(|a| {
                let d = (wp.0 >> 16) as i16 as f32 / 120.0;
                a.app.scroll = (a.app.scroll - d * 80.0).clamp(0.0, a.app.max_scroll);
                a.invalidate();
            });
            LRESULT(0)
        }
        WM_CHAR => {
            if with(|a| {
                let r = a.app.char(wp.0 as u32);
                a.invalidate();
                r
            }) == Some(true)
            {
                LRESULT(0)
            } else {
                DefWindowProcW(hwnd, msg, wp, lp)
            }
        }
        WM_POWERBROADCAST => {
            if wp.0 == PBT_APMSUSPEND {
                with(|a| {
                    if a.app.cfg.vault.lock_on_sleep {
                        a.app.lock(carlui_core::t!("uyku", "sleep"));
                        a.invalidate();
                    }
                });
            }
            LRESULT(1)
        }
        WM_WTSSESSION_CHANGE => {
            if wp.0 == WTS_SESSION_LOCK {
                with(|a| {
                    if a.app.cfg.vault.lock_on_sleep {
                        a.app.lock(carlui_core::t!("oturum kilidi", "session lock"));
                        a.invalidate();
                    }
                });
            }
            LRESULT(0)
        }
        WM_APP_RELOAD => {
            with(|a| {
                if let Some(c) = a.reload.lock().ok().and_then(|mut s| s.take()) {
                    capture_affinity(hwnd, &c);
                    a.app.reload(c);
                    a.invalidate();
                }
            });
            LRESULT(0)
        }
        WM_APP_LINK => {
            let jobs = LINK.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
            for (req, pid, tx) in jobs {
                let reply = with(|a| {
                    let r = a.app.link(req, pid);
                    a.invalidate();
                    r
                });
                if let Some(r) = reply {
                    let _ = tx.send(r);
                }
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            with(|a| {
                a.scale = (wp.0 & 0xFFFF) as f32 / 96.0;
                a.rt = None;
            });
            let r = &*(lp.0 as *const RECT);
            let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            let mm = &mut *(lp.0 as *mut MINMAXINFO);
            mm.ptMinTrackSize = POINT { x: 820, y: 600 };
            LRESULT(0)
        }
        WM_DESTROY => {
            with(|a| {
                a.app.clear_clipboard();
                a.app.lock(carlui_core::t!("kapandı", "closed"));
            });
            let _ = WTSUnRegisterSessionNotification(hwnd);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn main() -> Result<()> {
    log::init("vault");
    let cfg = config::load();
    carlui_ui::tone::follow(&cfg.theme);
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let _m = CreateMutexW(None, true, w!("Local\\CarlUI.Vault"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if let Ok(h) = FindWindowW(CLASS, None) {
                let _ = ShowWindow(h, SW_RESTORE);
                let _ = SetForegroundWindow(h);
            }
            return Ok(());
        }
        let inst: HINSTANCE = GetModuleHandleW(None)?.into();
        let icon = LoadIconW(inst, PCWSTR(1 as _)).or_else(|_| LoadIconW(None, IDI_APPLICATION))?;
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wnd_proc),
            hInstance: inst,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hIcon: icon,
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let dpi = GetDpiForSystem().max(96) as f32 / 96.0;
        let (ww, wh) = ((1000.0 * dpi) as i32, (700.0 * dpi) as i32);
        let sw = GetSystemMetrics(SM_CXSCREEN);
        let sh = GetSystemMetrics(SM_CYSCREEN);
        let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), CLASS, w!("Carl Vault"), WS_OVERLAPPEDWINDOW, (sw - ww) / 2, (sh - wh) / 2, ww, wh, None, None, inst, None)?;
        let on = i32::from(!carlui_ui::tone::light());
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &on as *const _ as _, 4);
        carlui_ui::tone::caption(hwnd);
        let backdrop = DWMSBT_TRANSIENTWINDOW;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, &backdrop as *const _ as _, 4);
        let _ = DwmExtendFrameIntoClientArea(hwnd, &MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 });
        capture_affinity(hwnd, &cfg);
        let _ = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);

        let reload: Arc<Mutex<Option<Config>>> = Arc::default();
        {
            let slot = reload.clone();
            let h = hwnd.0 as isize;
            config::watch(move |c| {
                if let Ok(mut s) = slot.lock() {
                    *s = Some(c);
                }
                let _ = PostMessageW(HWND(h as _), WM_APP_RELOAD, WPARAM(0), LPARAM(0));
            });
        }
        let win = Win {
            app: App::new(hwnd, cfg),
            rt: None,
            factory: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?,
            fonts: Fonts::new()?,
            scale: GetDpiForWindow(hwnd).max(96) as f32 / 96.0,
            mouse: (-1.0, -1.0),
            hits: Vec::new(),
            accent: glassmenu::accent(),
            reload,
        };
        tween::enable();
        WIN.with(|a| *a.borrow_mut() = Some(win));
        SetTimer(hwnd, TIMER_IDLE, 5000, None);
        start_link(hwnd);
        info!("Vault penceresi açıldı");
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}
