//! Karşılama penceresi: ayarlar penceresinden küçük, sabit boyutlu, aynı cam
//! zemin. Kendi sınıfı (`CarlUI.Welcome`; "CarlUI." öneki ana sürecin tam
//! ekran algısını şaşırtmasın) ve tek örnek kilidi vardır. Açılınca öne
//! zorlanmaz: yalnızca normal gösterilir (öne gelme iznini başlatan süreç verir).

use super::{model::Nav, Go, Welcome};
use carlui_core::config::{self, Config};
use carlui_ui::{paint::Fonts, tween};
use std::{
    cell::RefCell,
    sync::{Arc, Mutex},
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
        System::{Com::*, LibraryLoader::GetModuleHandleW, Threading::CreateMutexW},
        UI::{Controls::MARGINS, HiDpi::*, Input::KeyboardAndMouse::*, Shell::ShellExecuteW, WindowsAndMessaging::*},
    },
};

const CLASS: PCWSTR = w!("CarlUI.Welcome");
const WM_APP_RELOAD: u32 = WM_APP + 1;
const WM_MOUSELEAVE: u32 = 0x02A3;
const TIMER_ANIM: usize = 2;
/// Pencerenin içerik boyutu (DIP).
const SIZE: (f32, f32) = (760.0, 560.0);

thread_local! {
    static WEL: RefCell<Option<Welcome>> = const { RefCell::new(None) };
}

fn with<T>(f: impl FnOnce(&mut Welcome) -> T) -> Option<T> {
    WEL.with(|a| a.try_borrow_mut().ok().and_then(|mut a| a.as_mut().map(f)))
}

pub(super) fn close(hwnd: HWND) {
    unsafe {
        let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
    }
}

/// Tam ayarlar penceresini açar (dock'taki "Ayarlar" ile aynı yol).
pub(super) fn launch_settings() {
    let exe = carlui_core::paths::sibling_exe("carlui-settings.exe");
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        ShellExecuteW(HWND::default(), w!("open"), &HSTRING::from(exe.as_os_str()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

impl Welcome {
    fn paint(&mut self) -> Result<()> {
        let mut rc = RECT::default();
        unsafe { GetClientRect(self.hwnd, &mut rc)? };
        let (pw, ph) = ((rc.right - rc.left).max(1) as u32, (rc.bottom - rc.top).max(1) as u32);
        if self.rt.is_none() {
            let props = D2D1_RENDER_TARGET_PROPERTIES { pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED }, dpiX: 96.0, dpiY: 96.0, ..Default::default() };
            let hp = D2D1_HWND_RENDER_TARGET_PROPERTIES { hwnd: self.hwnd, pixelSize: D2D_SIZE_U { width: pw, height: ph }, presentOptions: D2D1_PRESENT_OPTIONS_NONE };
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
            let mut p = super::W { rt: &base, fonts: &self.fonts, mouse: self.mouse, hits: &mut hits, accent: self.accent() };
            super::draw::frame(self, &mut p, w, h);
        }
        self.hits = hits;
        unsafe {
            if rt.EndDraw(None, None).is_err() {
                self.rt = None;
            }
            if tween::busy() {
                SetTimer(self.hwnd, TIMER_ANIM, 16, None);
            }
        }
        Ok(())
    }

    /// Klavye: Enter birincil düğme, Esc kapat (işaret zaten yazıldı),
    /// sol/sağ ok geri/ileri. Yalnızca bu pencere öndeyken; genel kısayol değil.
    fn key(&mut self, vk: u16) -> bool {
        let nav = match VIRTUAL_KEY(vk) {
            VK_RETURN | VK_RIGHT => Nav::Next,
            VK_LEFT => Nav::Back,
            VK_ESCAPE => Nav::Skip,
            _ => return false,
        };
        self.act(Go::Nav(nav));
        true
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
            let _ = KillTimer(hwnd, wp.0);
            with(|a| a.invalidate());
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            with(|a| {
                let (x, y) = xy(lp);
                let pt = (x / a.scale, y / a.scale);
                let changed = carlui_ui::paint::hover_changed(&a.hits, a.mouse, pt);
                a.mouse = pt;
                let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut tme);
                if changed {
                    a.invalidate();
                }
            });
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with(|a| {
                a.mouse = (-1.0, -1.0);
                a.invalidate();
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            // `act` pencereyi kapatabilir; kapanış mesajı kuyruğa gider, burada çalışmaz.
            with(|a| {
                let (x, y) = xy(lp);
                a.mouse = (x / a.scale, y / a.scale);
                if let Some(g) = a.hit_at(a.mouse) {
                    a.act(g);
                }
            });
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if with(|a| a.key(wp.0 as u16)) == Some(true) {
                LRESULT(0)
            } else {
                DefWindowProcW(hwnd, msg, wp, lp)
            }
        }
        WM_APP_RELOAD => {
            with(|a| {
                if let Some(c) = a.reload.lock().ok().and_then(|mut s| s.take()) {
                    a.cfg = c;
                    carlui_ui::tone::follow_window(&a.cfg.theme, a.hwnd);
                    a.invalidate();
                }
            });
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
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// `--welcome` ile çağrılır; pencere kapanınca döner.
pub fn run() -> Result<()> {
    let cfg = config::load();
    carlui_ui::tone::follow(&cfg.theme);
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let _m = CreateMutexW(None, true, w!("Local\\CarlUI.Welcome"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // Zaten açık (ör. Ayarlar'dan yeniden istendi): onu göster.
            if let Ok(h) = FindWindowW(CLASS, None) {
                let _ = ShowWindow(h, SW_SHOWNORMAL);
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
        let dpi = GetDpiForSystem().max(96);
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let mut rc = RECT { left: 0, top: 0, right: (SIZE.0 * dpi as f32 / 96.0) as i32, bottom: (SIZE.1 * dpi as f32 / 96.0) as i32 };
        let _ = AdjustWindowRectExForDpi(&mut rc, style, false, WINDOW_EX_STYLE(0), dpi);
        let mut wa = RECT::default();
        if SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut wa as *mut _ as _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)).is_err() {
            wa = RECT { left: 0, top: 0, right: GetSystemMetrics(SM_CXSCREEN), bottom: GetSystemMetrics(SM_CYSCREEN) };
        }
        let (aw, ah) = (wa.right - wa.left, wa.bottom - wa.top);
        let (ww, wh) = ((rc.right - rc.left).min(aw - 16), (rc.bottom - rc.top).min(ah - 16));
        let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), CLASS, &HSTRING::from(carlui_core::t!("CarlUI'ye hoş geldin", "Welcome to CarlUI")), style, wa.left + (aw - ww) / 2, wa.top + (ah - wh) / 2, ww, wh, None, None, inst, None)?;
        let on = i32::from(!carlui_ui::tone::light());
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &on as *const _ as _, 4);
        carlui_ui::tone::caption(hwnd);
        let backdrop = DWMSBT_TRANSIENTWINDOW;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, &backdrop as *const _ as _, 4);
        let _ = DwmExtendFrameIntoClientArea(hwnd, &MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 });

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
        // İşaret hemen yazılır: kapatmak, atlamak ya da çökme akışı bir daha açmaz.
        let cfg = if cfg.general.welcomed { cfg } else { config::update(super::model::mark).unwrap_or(cfg) };
        let app = Welcome {
            hwnd,
            rt: None,
            factory: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?,
            fonts: Fonts::new()?,
            scale: GetDpiForWindow(hwnd).max(96) as f32 / 96.0,
            mouse: (-1.0, -1.0),
            hits: Vec::new(),
            cfg,
            step: super::model::Step::Look,
            touched: [false; 5],
            reload,
        };
        tween::enable();
        WEL.with(|a| *a.borrow_mut() = Some(app));
        // Öne zorlanmaz (SetForegroundWindow yok); izin varsa Windows kendisi öne alır.
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}
