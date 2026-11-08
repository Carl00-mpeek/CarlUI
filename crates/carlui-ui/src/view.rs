//! Bu modül "her küçük araç için pencere, çizim ve girdi kodunu yeniden
//! yazmak" sorununu çözüyor: araçlar (yeniden adlandırma, disk analizörü)
//! yalnızca `View` özelliğini uygular; pencere, koyu akrilik zemin, DPI,
//! Direct2D çizimi, tıklama/klavye/tekerlek yönlendirmesi buradadır. Çizim
//! Ayarlar penceresindeki anlık modlu `Painter` ile yapılır.
//!
//! `noactivate` pencereler (alan kaydı hapı) tıklanınca bile etkinleşmez:
//! öndeki oyun odağı kaybetmez, tam ekrandan düşmez.

use crate::{
    glassmenu,
    paint::{Fonts, Painter, R},
    tween,
};
use std::{any::Any, cell::RefCell};
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
        System::LibraryLoader::GetModuleHandleW,
        UI::{Controls::MARGINS, HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};

const WM_MOUSELEAVE: u32 = 0x02A3;

/// Zamanlayıcı ya da başka iş parçacığı pencereyi bu mesajla uyandırır (`View::tick`).
pub const WM_APP_TICK: u32 = WM_APP + 7;

pub struct Ctx {
    pub hwnd: HWND,
    pub close: bool,
}

pub fn ctrl() -> bool {
    unsafe { GetKeyState(VK_CONTROL.0 as i32) < 0 }
}

pub fn shift() -> bool {
    unsafe { GetKeyState(VK_SHIFT.0 as i32) < 0 }
}

pub trait View: 'static {
    type A: Clone + 'static;
    fn paint(&mut self, p: &mut Painter<Self::A>, w: f32, h: f32);
    fn click(&mut self, a: Self::A, ctx: &mut Ctx);
    fn right_click(&mut self, _a: Self::A, _ctx: &mut Ctx) {}
    /// Tuş basıldı; `true` = işlendi.
    fn key(&mut self, _vk: u16, _ctx: &mut Ctx) -> bool {
        false
    }
    fn char(&mut self, _c: char, _ctx: &mut Ctx) {}
    fn wheel(&mut self, _delta: f32, _at: (f32, f32), _ctx: &mut Ctx) {}
    /// Sol tuş bir alanın üstünde basıldı; `true` = sürükleme başlasın
    /// (bırakınca `click` yerine `release` çağrılır).
    fn press(&mut self, _a: &Self::A, _at: (f32, f32), _ctx: &mut Ctx) -> bool {
        false
    }
    fn drag(&mut self, _at: (f32, f32), _ctx: &mut Ctx) {}
    fn release(&mut self, _at: (f32, f32), _ctx: &mut Ctx) {}
    /// `poke` ile uyandırıldı ya da zamanlayıcı.
    fn tick(&mut self, _ctx: &mut Ctx) {}
    /// Çizimden sonra hâlâ canlandırma sürüyor mu (sürekli yeniden çizim).
    fn animating(&self) -> bool {
        false
    }
    /// Pencere oluşturuldu (gösterilmeden hemen önce).
    fn opened(&mut self, _hwnd: HWND) {}
}

#[derive(Default)]
pub struct Opts {
    pub title: String,
    pub w: f32,
    pub h: f32,
    /// Ekran konumu (fiziksel piksel, sol üst); None = ortada.
    pub at: Option<(i32, i32)>,
    pub resizable: bool,
    /// Görev çubuğunda görünmeyen küçük araç penceresi; odak kaybolunca kapanır.
    pub popup: bool,
    /// Hiç etkinleşmeyen, her zaman üstte duran kaplama (odak öndeki
    /// uygulamada kalır); `popup` ile birlikte kullanılır, odak kaybında kapanmaz.
    pub noactivate: bool,
    /// > 0 ise bu aralıkla `View::tick` çağrılır (ms).
    pub timer_ms: u32,
}

struct Host<V: View> {
    view: V,
    rt: Option<ID2D1HwndRenderTarget>,
    factory: ID2D1Factory,
    fonts: Fonts,
    scale: f32,
    mouse: (f32, f32),
    hits: Vec<(R, V::A)>,
    accent: u32,
    popup: bool,
    noactivate: bool,
    dragging: bool,
}

thread_local! {
    static HOST: RefCell<Option<Box<dyn Any>>> = const { RefCell::new(None) };
}

fn with<V: View, T>(f: impl FnOnce(&mut Host<V>) -> T) -> Option<T> {
    HOST.with(|h| {
        let mut b = h.try_borrow_mut().ok()?;
        let host = b.as_mut()?.downcast_mut::<Host<V>>()?;
        Some(f(host))
    })
}

impl<V: View> Host<V> {
    fn paint(&mut self, hwnd: HWND) -> Result<()> {
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
        let mut hits = std::mem::take(&mut self.hits);
        hits.clear();
        tween::frame();
        unsafe {
            rt.BeginDraw();
            rt.SetTransform(&Matrix3x2 { M11: s, M12: 0.0, M21: 0.0, M22: s, M31: 0.0, M32: 0.0 });
            rt.Clear(Some(&crate::tone::backdrop()));
        }
        {
            let base: ID2D1RenderTarget = rt.cast()?;
            let mut p = Painter { rt: &base, fonts: &self.fonts, mouse: self.mouse, hits: &mut hits, accent: self.accent };
            let (w, h) = (pw as f32 / s, ph as f32 / s);
            p.fill(R::new(0.0, 0.0, w, h), 0x101014, 0.30);
            self.view.paint(&mut p, w, h);
        }
        self.hits = hits;
        let r = unsafe { rt.EndDraw(None, None) };
        if r.is_err() {
            self.rt = None;
        }
        if tween::busy() || self.view.animating() {
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }
        Ok(())
    }

    fn hit_at(&self, pt: (f32, f32)) -> Option<V::A> {
        self.hits.iter().rev().find(|(r, _)| r.contains(pt)).map(|(_, a)| a.clone())
    }

    fn pt(&self, lp: LPARAM) -> (f32, f32) {
        let x = (lp.0 & 0xFFFF) as i16 as f32;
        let y = ((lp.0 >> 16) & 0xFFFF) as i16 as f32;
        (x / self.scale, y / self.scale)
    }
}

fn after(hwnd: HWND, ctx: Ctx) {
    unsafe {
        if ctx.close {
            let _ = DestroyWindow(hwnd);
        } else {
            let _ = InvalidateRect(hwnd, None, false);
        }
    }
}

unsafe extern "system" fn wnd_proc<V: View>(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let mut ctx = Ctx { hwnd, close: false };
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            BeginPaint(hwnd, &mut ps);
            with::<V, _>(|h| h.paint(hwnd));
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        // Açılır pencere: kalın çerçeve yalnızca akrilik zemin ve gölge için
        // vardır; istemci alanı tüm pencereyi kaplar, kenardan boyutlanmaz.
        WM_NCCALCSIZE if wp.0 != 0 && with::<V, _>(|h| h.popup).unwrap_or(false) => LRESULT(0),
        WM_NCHITTEST if with::<V, _>(|h| h.popup).unwrap_or(false) => LRESULT(HTCLIENT as isize),
        WM_SIZE => {
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_DPICHANGED => {
            with::<V, _>(|h| h.scale = (wp.0 & 0xFFFF) as f32 / 96.0);
            let r = &*(lp.0 as *const RECT);
            let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let drag = with::<V, _>(|h| {
                let pt = h.pt(lp);
                let a = h.hit_at(pt)?;
                h.dragging = h.view.press(&a, pt, &mut ctx);
                Some(h.dragging)
            })
            .flatten()
            .unwrap_or(false);
            if drag {
                SetCapture(hwnd);
            }
            after(hwnd, ctx);
            LRESULT(0)
        }
        WM_LBUTTONUP if with::<V, _>(|h| h.dragging).unwrap_or(false) => {
            with::<V, _>(|h| {
                h.dragging = false;
                let pt = h.pt(lp);
                h.view.release(pt, &mut ctx);
            });
            let _ = ReleaseCapture();
            after(hwnd, ctx);
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            with::<V, _>(|h| {
                if h.dragging {
                    h.dragging = false;
                    let pt = h.mouse;
                    h.view.release(pt, &mut ctx);
                }
            });
            after(hwnd, ctx);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            with::<V, _>(|h| {
                h.mouse = h.pt(lp);
                if h.dragging {
                    let pt = h.mouse;
                    h.view.drag(pt, &mut ctx);
                }
            });
            let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
            let _ = TrackMouseEvent(&mut tme);
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with::<V, _>(|h| h.mouse = (-1.0, -1.0));
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_LBUTTONUP | WM_RBUTTONUP => {
            with::<V, _>(|h| {
                let pt = h.pt(lp);
                if let Some(a) = h.hit_at(pt) {
                    if msg == WM_LBUTTONUP {
                        h.view.click(a, &mut ctx);
                    } else {
                        h.view.right_click(a, &mut ctx);
                    }
                }
            });
            after(hwnd, ctx);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let d = ((wp.0 >> 16) & 0xFFFF) as i16 as f32 / 120.0;
            with::<V, _>(|h| {
                let mut p = POINT { x: (lp.0 & 0xFFFF) as i16 as i32, y: ((lp.0 >> 16) & 0xFFFF) as i16 as i32 };
                let _ = ScreenToClient(hwnd, &mut p);
                let at = (p.x as f32 / h.scale, p.y as f32 / h.scale);
                h.view.wheel(d, at, &mut ctx)
            });
            after(hwnd, ctx);
            LRESULT(0)
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let done = with::<V, _>(|h| h.view.key(wp.0 as u16, &mut ctx)).unwrap_or(false);
            if !done && wp.0 as u16 == VK_ESCAPE.0 {
                ctx.close = true;
            }
            if done || ctx.close {
                after(hwnd, ctx);
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_CHAR => {
            if let Some(c) = char::from_u32(wp.0 as u32).filter(|c| !c.is_control()) {
                with::<V, _>(|h| h.view.char(c, &mut ctx));
            }
            after(hwnd, ctx);
            LRESULT(0)
        }
        WM_APP_TICK | WM_TIMER => {
            with::<V, _>(|h| h.view.tick(&mut ctx));
            after(hwnd, ctx);
            LRESULT(0)
        }
        // Kaplama: tıklama pencereyi etkinleştirmez, öndeki oyun odağı tutar.
        WM_MOUSEACTIVATE if with::<V, _>(|h| h.noactivate).unwrap_or(false) => LRESULT(MA_NOACTIVATE as isize),
        // Akrilik zemin etkin pencerelerde çizilir: çerçeveyi hep etkin göster
        // (yalnızca görünüş; pencere gerçekten etkinleşmez).
        WM_NCACTIVATE if with::<V, _>(|h| h.noactivate).unwrap_or(false) => DefWindowProcW(hwnd, msg, WPARAM(1), lp),
        WM_ACTIVATE => {
            // Küçük açılır araç: başka yere tıklanınca kapanır.
            if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE && with::<V, _>(|h| h.popup && !h.noactivate).unwrap_or(false) && lp.0 == 0 {
                let _ = DestroyWindow(hwnd);
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Pencereyi açar ve kapanana kadar mesaj döngüsünü çalıştırır; görünümü geri verir.
pub fn run<V: View>(view: V, o: Opts) -> Result<V> {
    unsafe {
        let inst: HINSTANCE = GetModuleHandleW(None)?.into();
        let class = w!("CarlUI.Tool");
        let icon = LoadIconW(inst, PCWSTR(1 as _)).or_else(|_| LoadIconW(None, IDI_APPLICATION))?;
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wnd_proc::<V>),
            hInstance: inst,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hIcon: icon,
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        // Pencere hangi monitörde açılacaksa onun DPI'ı.
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let anchor = o.at.map(|(x, y)| POINT { x, y }).unwrap_or(pt);
        let mon = MonitorFromPoint(anchor, MONITOR_DEFAULTTONEAREST);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let scale = dx.max(96) as f32 / 96.0;
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let wa = mi.rcWork;
        let (ww, wh) = ((o.w * scale) as i32, (o.h * scale) as i32);
        let (x, y) = match o.at {
            Some((x, y)) => (x.clamp(wa.left, (wa.right - ww).max(wa.left)), y.clamp(wa.top, (wa.bottom - wh).max(wa.top))),
            None => (wa.left + (wa.right - wa.left - ww) / 2, wa.top + (wa.bottom - wa.top - wh) / 2),
        };
        let (style, ex) = if o.noactivate {
            (WS_POPUP | WS_THICKFRAME, WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE)
        } else if o.popup {
            (WS_POPUP | WS_THICKFRAME, WS_EX_TOOLWINDOW | WS_EX_TOPMOST)
        } else if o.resizable {
            (WS_OVERLAPPEDWINDOW, WINDOW_EX_STYLE(0))
        } else {
            (WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX, WINDOW_EX_STYLE(0))
        };
        let host = Host::<V> {
            view,
            rt: None,
            factory: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?,
            fonts: Fonts::new()?,
            scale,
            mouse: (-1.0, -1.0),
            hits: Vec::new(),
            accent: glassmenu::accent(),
            popup: o.popup || o.noactivate,
            noactivate: o.noactivate,
            dragging: false,
        };
        HOST.with(|h| *h.borrow_mut() = Some(Box::new(host)));
        let hwnd = CreateWindowExW(ex, class, &HSTRING::from(o.title.as_str()), style, x, y, ww, wh, None, None, inst, None)?;
        let on = i32::from(!crate::tone::light());
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &on as *const _ as _, 4);
        crate::tone::caption(hwnd);
        let backdrop = DWMSBT_TRANSIENTWINDOW;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, &backdrop as *const _ as _, 4);
        let corner = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &corner as *const _ as _, 4);
        let _ = DwmExtendFrameIntoClientArea(hwnd, &MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 });
        if o.popup || o.noactivate {
            // Kenarlıksız açılır pencere: çerçeve yalnızca akrilik ve gölge için.
            let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
        if o.timer_ms > 0 {
            SetTimer(hwnd, 1, o.timer_ms, None);
        }
        with::<V, _>(|h| h.view.opened(hwnd));
        tween::enable();
        if o.noactivate {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = SendMessageW(hwnd, WM_NCACTIVATE, WPARAM(1), LPARAM(0));
        } else {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let host = HOST.with(|h| h.borrow_mut().take()).and_then(|b| b.downcast::<Host<V>>().ok()).ok_or(Error::from(E_FAIL))?;
        Ok(host.view)
    }
}
