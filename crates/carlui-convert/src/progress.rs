//! İlerleme penceresi: dock'un hemen üstünde küçük bir cam kart. Dosya adı,
//! sıra (2/5), yüzde ve çubuk; sağ üstteki X işi durdurur. İş bitince sonucu
//! birkaç saniye gösterip kapanır; tıklanırsa çıkan dosya Gezgin'de seçilir.
//! Çizim iş parçacığı yalnızca paylaşılan durumu okur; dönüştürme ayrı iş
//! parçacığında yürür ve her değişiklikte pencereye mesaj atar.

use std::{
    sync::{
        atomic::{AtomicBool, AtomicIsize, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};
use windows::{
    core::*,
    Foundation::Numerics::Matrix3x2,
    Win32::{
        Foundation::*,
        Graphics::{
            Direct2D::{Common::*, *},
            DirectWrite::*,
            Dwm::*,
            Dxgi::Common::*,
            Gdi::*,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{HiDpi::GetDpiForWindow, WindowsAndMessaging::*},
    },
};

pub const WM_APP_UPDATE: u32 = WM_APP + 1;
const TIMER_CLOSE: usize = 1;
const W: f32 = 400.0;
const H: f32 = 92.0;

#[derive(Default, Clone)]
pub struct State {
    pub title: String,
    pub file: String,
    pub frac: f32,
    pub done: bool,
    pub failed: bool,
    /// Bitince tıklanırsa Gezgin'de seçilecek dosya.
    pub reveal: Option<String>,
    /// `reveal` Gezgin'de seçilmek yerine doğrudan açılsın.
    pub open: bool,
}

pub struct Shared {
    pub state: Mutex<State>,
    pub cancel: Arc<AtomicBool>,
    hwnd: AtomicIsize,
}

impl Shared {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { state: Mutex::default(), cancel: Arc::default(), hwnd: AtomicIsize::new(0) })
    }

    pub fn update(&self, f: impl FnOnce(&mut State)) {
        if let Ok(mut s) = self.state.lock() {
            f(&mut s);
        }
        unsafe {
            let _ = PostMessageW(HWND(self.hwnd.load(Ordering::SeqCst) as _), WM_APP_UPDATE, WPARAM(0), LPARAM(0));
        }
    }
}

struct Win {
    shared: Arc<Shared>,
    rt: Option<ID2D1HwndRenderTarget>,
    factory: ID2D1Factory,
    dw: IDWriteFactory,
    scale: f32,
    accent: u32,
    hover_x: bool,
    closing: Option<Instant>,
}

thread_local! {
    static WIN: std::cell::RefCell<Option<Win>> = const { std::cell::RefCell::new(None) };
}

fn col(c: u32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: ((c >> 16) & 0xFF) as f32 / 255.0, g: ((c >> 8) & 0xFF) as f32 / 255.0, b: (c & 0xFF) as f32 / 255.0, a }
}

fn close_rect() -> D2D_RECT_F {
    D2D_RECT_F { left: W - 40.0, top: 8.0, right: W - 10.0, bottom: 38.0 }
}

impl Win {
    fn text(&self, rt: &ID2D1RenderTarget, s: &str, r: D2D_RECT_F, size: f32, bold: bool, c: u32, a: f32, right: bool, icon: bool) {
        unsafe {
            let fam = if icon { w!("Segoe Fluent Icons") } else { w!("Segoe UI Variable Text") };
            let Ok(f) = self.dw.CreateTextFormat(fam, None, if bold { DWRITE_FONT_WEIGHT_SEMI_BOLD } else { DWRITE_FONT_WEIGHT_NORMAL }, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("tr-tr")) else { return };
            let _ = f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            let _ = f.SetTextAlignment(if right { DWRITE_TEXT_ALIGNMENT_TRAILING } else if icon { DWRITE_TEXT_ALIGNMENT_CENTER } else { DWRITE_TEXT_ALIGNMENT_LEADING });
            let trim = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
            let sign = self.dw.CreateEllipsisTrimmingSign(&f).ok();
            let _ = f.SetTrimming(&trim, sign.as_ref());
            let (c, a) = carlui_ui::tone::ink(c, a);
            let Ok(b) = rt.CreateSolidColorBrush(&col(c, a), None) else { return };
            let w: Vec<u16> = s.encode_utf16().collect();
            rt.DrawText(&w, &f, &r, &b, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
        }
    }

    fn round(&self, rt: &ID2D1RenderTarget, r: D2D_RECT_F, rad: f32, c: u32, a: f32) {
        unsafe {
            let (c, a) = carlui_ui::tone::fill(c, a);
            if let Ok(b) = rt.CreateSolidColorBrush(&col(c, a), None) {
                rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r, radiusX: rad, radiusY: rad }, &b);
            }
        }
    }

    fn paint(&mut self, hwnd: HWND) -> Result<()> {
        let mut rc = RECT::default();
        unsafe { GetClientRect(hwnd, &mut rc)? };
        if self.rt.is_none() {
            let props = D2D1_RENDER_TARGET_PROPERTIES { pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED }, dpiX: 96.0, dpiY: 96.0, ..Default::default() };
            let hp = D2D1_HWND_RENDER_TARGET_PROPERTIES { hwnd, pixelSize: D2D_SIZE_U { width: rc.right as u32, height: rc.bottom as u32 }, presentOptions: D2D1_PRESENT_OPTIONS_NONE };
            let rt = unsafe { self.factory.CreateHwndRenderTarget(&props, &hp)? };
            unsafe { rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE) };
            self.rt = Some(rt);
        }
        let hrt = self.rt.clone().unwrap();
        let rt: ID2D1RenderTarget = hrt.cast()?;
        let st = self.shared.state.lock().map(|s| s.clone()).unwrap_or_default();
        let s = self.scale;
        unsafe {
            rt.BeginDraw();
            rt.SetTransform(&Matrix3x2 { M11: s, M12: 0.0, M21: 0.0, M22: s, M31: 0.0, M32: 0.0 });
            rt.Clear(Some(&carlui_ui::tone::backdrop()));
        }
        self.round(&rt, D2D_RECT_F { left: 0.0, top: 0.0, right: W, bottom: H }, 0.0, 0x16161A, 0.40);
        let (g, gc) = if st.failed {
            ('\u{E783}', 0xFF6B6B)
        } else if st.done {
            ('\u{E73E}', 0x6CCB5F)
        } else {
            ('\u{E895}', self.accent)
        };
        self.text(&rt, &g.to_string(), D2D_RECT_F { left: 14.0, top: 10.0, right: 40.0, bottom: 36.0 }, 16.0, false, gc, 1.0, false, true);
        self.text(&rt, &st.title, D2D_RECT_F { left: 46.0, top: 10.0, right: W - 96.0, bottom: 36.0 }, 13.5, true, 0xFFFFFF, 0.95, false, false);
        if !st.done {
            self.text(&rt, &carlui_core::lang::pct(st.frac * 100.0), D2D_RECT_F { left: W - 100.0, top: 10.0, right: W - 46.0, bottom: 36.0 }, 13.0, false, 0xFFFFFF, 0.75, true, false);
        }
        if self.hover_x {
            self.round(&rt, close_rect(), 5.0, 0xFFFFFF, 0.10);
        }
        self.text(&rt, "\u{E711}", close_rect(), 11.0, false, 0xFFFFFF, 0.8, false, true);
        self.text(&rt, &st.file, D2D_RECT_F { left: 46.0, top: 38.0, right: W - 16.0, bottom: 60.0 }, 12.0, false, 0xFFFFFF, 0.6, false, false);
        let bar = D2D_RECT_F { left: 46.0, top: 68.0, right: W - 16.0, bottom: 74.0 };
        self.round(&rt, bar, 3.0, 0xFFFFFF, 0.10);
        let frac = if st.done { 1.0 } else { st.frac.clamp(0.0, 1.0) };
        if frac > 0.0 {
            let fill = D2D_RECT_F { right: bar.left + (bar.right - bar.left) * frac, ..bar };
            self.round(&rt, fill, 3.0, gc, 0.95);
        }
        unsafe {
            if hrt.EndDraw(None, None).is_err() {
                self.rt = None;
            }
        }
        Ok(())
    }
}

fn with<T>(f: impl FnOnce(&mut Win) -> T) -> Option<T> {
    WIN.with(|w| w.try_borrow_mut().ok().and_then(|mut w| w.as_mut().map(f)))
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let pt = || {
        let s = with(|w| w.scale).unwrap_or(1.0);
        ((lp.0 & 0xFFFF) as i16 as f32 / s, ((lp.0 >> 16) & 0xFFFF) as i16 as f32 / s)
    };
    let in_x = |p: (f32, f32)| {
        let r = close_rect();
        p.0 >= r.left && p.0 < r.right && p.1 >= r.top && p.1 < r.bottom
    };
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            BeginPaint(hwnd, &mut ps);
            with(|w| w.paint(hwnd).ok());
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_APP_UPDATE => {
            let done = with(|w| w.shared.state.lock().map(|s| s.done).unwrap_or(false)).unwrap_or(false);
            if done && with(|w| w.closing.is_none()).unwrap_or(false) {
                with(|w| w.closing = Some(Instant::now()));
                SetTimer(hwnd, TIMER_CLOSE, 4000, None);
            }
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let p = pt();
            let mut tme = windows::Win32::UI::Input::KeyboardAndMouse::TRACKMOUSEEVENT { cbSize: 16 + 8, dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
            tme.cbSize = std::mem::size_of_val(&tme) as u32;
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::TrackMouseEvent(&mut tme);
            if with(|w| std::mem::replace(&mut w.hover_x, in_x(p)) != in_x(p)).unwrap_or(false) {
                let _ = InvalidateRect(hwnd, None, false);
            }
            // Bitmişken üzerine gelinirse hemen kapanmasın.
            if with(|w| w.closing.is_some()).unwrap_or(false) {
                SetTimer(hwnd, TIMER_CLOSE, 4000, None);
            }
            LRESULT(0)
        }
        0x02A3 => {
            // WM_MOUSELEAVE
            with(|w| w.hover_x = false);
            let _ = InvalidateRect(hwnd, None, false);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let p = pt();
            let (done, reveal, open) = with(|w| w.shared.state.lock().map(|s| (s.done, s.reveal.clone(), s.open)).unwrap_or_default()).unwrap_or_default();
            if in_x(p) {
                if done {
                    let _ = DestroyWindow(hwnd);
                } else {
                    with(|w| w.shared.cancel.store(true, Ordering::SeqCst));
                }
            } else if done {
                if let Some(r) = reveal {
                    if open {
                        crate::open(&r);
                    } else {
                        crate::reveal(&r);
                    }
                }
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_TIMER if wp.0 == TIMER_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Pencereyi açar ve kapanana dek mesaj döngüsünü yürütür.
pub fn run(shared: Arc<Shared>, accent: u32, start: impl FnOnce()) -> Result<()> {
    unsafe {
        let inst: HINSTANCE = GetModuleHandleW(None)?.into();
        let wc = WNDCLASSEXW { cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32, lpfnWndProc: Some(proc), hInstance: inst, hCursor: LoadCursorW(None, IDC_ARROW)?, lpszClassName: w!("CarlUI.Convert"), ..Default::default() };
        RegisterClassExW(&wc);
        let mut work = RECT::default();
        let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut work as *mut _ as _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
        let hwnd = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, w!("CarlUI.Convert"), w!("CarlUI Converter"), WS_POPUP, 0, 0, 10, 10, None, None, inst, None)?;
        let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        let (pw, ph) = ((W * scale) as i32, (H * scale) as i32);
        // Dock ekranın altında, iş alanı çoğu zaman tam ekran: dock'un üstüne koy.
        let x = (work.left + work.right - pw) / 2;
        let y = work.bottom - ph - (96.0 * scale) as i32;
        let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y, pw, ph, SWP_NOACTIVATE);
        let on = i32::from(!carlui_ui::tone::light());
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &on as *const _ as _, 4);
        let round = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &round as *const _ as _, 4);
        let backdrop = DWMSBT_TRANSIENTWINDOW;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, &backdrop as *const _ as _, 4);
        let border: u32 = if carlui_ui::tone::light() { 0x00E4DCD8 } else { 0x00505050 };
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, &border as *const _ as _, 4);
        let _ = DwmExtendFrameIntoClientArea(hwnd, &windows::Win32::UI::Controls::MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 });
        shared.hwnd.store(hwnd.0 as isize, Ordering::SeqCst);
        WIN.with(|w| {
            *w.borrow_mut() = Some(Win {
                shared: shared.clone(),
                rt: None,
                factory: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).unwrap(),
                dw: DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).unwrap(),
                scale,
                accent: carlui_ui::tone::accent(accent),
                hover_x: false,
                closing: None,
            })
        });
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        start();
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        // Pencere kapandıysa iş sürüyorsa da durdur.
        shared.cancel.store(true, Ordering::SeqCst);
    }
    Ok(())
}
