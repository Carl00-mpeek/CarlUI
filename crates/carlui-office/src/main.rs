//! Carl Office (`carlui-office.exe`): Excel (xlsx/xls/csv), Word (docx),
//! PowerPoint (pptx) ve PDF dosyalarını açan, düzenleyen hafif uygulama.
//! Bu dosya "pencere, çizim döngüsü ve Windows iletileri" sorununu çözüyor:
//! CarlUI'nin diğer pencereleri gibi akrilik cam pencere, Direct2D ile çizim.
//! Tek örnek değildir: her belge kendi süreçte açılır, pencere kapanınca
//! süreç biter. Arka planda kalan hiçbir şey yoktur.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod clip;
mod dialog;
mod doc;
mod field;
mod img;
mod pdf;
mod recent;
mod rich;
mod richedit;
mod sheet;
mod slides;
mod tl;
mod ui;
mod xml;
mod zip;

use app::{Act, App, Keys, Mouse, View};
use carlui_core::{config, info, log, t};
use carlui_ui::{
    glassmenu,
    paint::{Fonts, Painter, R},
    tween,
};
use std::{cell::RefCell, path::Path};
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
        System::{Com::*, LibraryLoader::GetModuleHandleW},
        UI::{
            Controls::MARGINS,
            HiDpi::*,
            Input::KeyboardAndMouse::*,
            Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, HDROP},
            WindowsAndMessaging::*,
        },
    },
};

const CLASS: PCWSTR = w!("CarlUI.Office");
const TIMER_ANIM: usize = 2;

struct Win {
    app: App,
    rt: Option<ID2D1HwndRenderTarget>,
    factory: ID2D1Factory,
    fonts: Fonts,
    scale: f32,
    mouse: (f32, f32),
    hits: Vec<(R, Act)>,
    accent: u32,
    /// Bekleyen UTF-16 yüksek vekil (emoji gibi iki parçalı karakterler).
    surrogate: Option<u16>,
    /// Fare belge alanında basılı (sürükleme görünüme gider).
    captured: bool,
    /// Sunu kipinden önceki pencere yerleşimi (tam ekrandayken dolu).
    placement: Option<WINDOWPLACEMENT>,
}

thread_local! {
    static WIN: RefCell<Option<Win>> = const { RefCell::new(None) };
}

fn with<T>(f: impl FnOnce(&mut Win) -> T) -> Option<T> {
    WIN.with(|a| a.try_borrow_mut().ok().and_then(|mut a| a.as_mut().map(f)))
}

/// Dosyayı uzantısına göre açar.
pub fn load(path: &Path) -> std::result::Result<Box<dyn View>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    // Uzantı yanlışsa içeriğe bak (zip / PDF imzası).
    let kind = match ext.as_str() {
        "xlsx" | "xlsm" | "xls" | "csv" | "tsv" | "txt" | "docx" | "pptx" | "pdf" => ext.clone(),
        _ if bytes.starts_with(b"%PDF") => "pdf".into(),
        _ => ext.clone(),
    };
    Ok(match kind.as_str() {
        "xlsx" | "xlsm" => Box::new(sheet::view::SheetView::new(sheet::xlsx::load(&bytes)?)),
        "xls" => Box::new(sheet::view::SheetView::new(sheet::xls::load(&bytes)?)),
        "csv" | "tsv" | "txt" => Box::new(sheet::view::SheetView::new(sheet::csv::load(&bytes))),
        "docx" => Box::new(doc::view::DocView::new(doc::docx::load(&bytes)?)),
        "pptx" => Box::new(slides::view::SlideView::new(slides::pptx::load(&bytes)?)),
        "pdf" => Box::new(pdf::view::PdfView::new(bytes)?),
        _ => return Err(t!("Bu dosya türü desteklenmiyor.", "This file type isn't supported.").to_string()),
    })
}

impl Win {
    fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(app::hwnd(), None, false);
        }
    }

    fn hit_at(&self, pt: (f32, f32)) -> Option<Act> {
        self.hits.iter().rev().filter(|(r, a)| r.contains(pt) && !matches!(a, Act::Scroll)).map(|(_, a)| a.clone()).next()
    }

    /// Sunu kipi: pencereyi çerçevesiz olarak bulunduğu ekranı kaplatır; çıkınca eski yerine koyar.
    fn set_full(&mut self, on: bool) {
        let hwnd = app::hwnd();
        unsafe {
            if on {
                let mut wp = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
                let _ = GetWindowPlacement(hwnd, &mut wp);
                self.placement = Some(wp);
                let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
                let _ = GetMonitorInfoW(mon, &mut mi);
                let r = mi.rcMonitor;
                SetWindowLongPtrW(hwnd, GWL_STYLE, (WS_POPUP | WS_VISIBLE).0 as isize);
                let _ = SetWindowPos(hwnd, HWND_TOP, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_FRAMECHANGED | SWP_SHOWWINDOW);
            } else {
                SetWindowLongPtrW(hwnd, GWL_STYLE, (WS_OVERLAPPEDWINDOW | WS_VISIBLE).0 as isize);
                if let Some(wp) = self.placement.take() {
                    let _ = SetWindowPlacement(hwnd, &wp);
                }
                let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED);
            }
        }
        self.invalidate();
    }

    fn paint(&mut self) -> Result<()> {
        let hwnd = app::hwnd();
        let mut rc = RECT::default();
        unsafe { GetClientRect(hwnd, &mut rc)? };
        let (pw, ph) = ((rc.right - rc.left).max(1) as u32, (rc.bottom - rc.top).max(1) as u32);
        if self.rt.is_none() {
            let props = D2D1_RENDER_TARGET_PROPERTIES { pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED }, dpiX: 96.0, dpiY: 96.0, ..Default::default() };
            let hp = D2D1_HWND_RENDER_TARGET_PROPERTIES { hwnd, pixelSize: D2D_SIZE_U { width: pw, height: ph }, presentOptions: D2D1_PRESENT_OPTIONS_NONE };
            let rt = unsafe { self.factory.CreateHwndRenderTarget(&props, &hp)? };
            unsafe { rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE) };
            self.rt = Some(rt);
            if let Some(v) = self.app.view.as_mut() {
                v.reset_device();
            }
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
        unsafe {
            rt.BeginDraw();
            rt.SetTransform(&Matrix3x2 { M11: s, M12: 0.0, M21: 0.0, M22: s, M31: 0.0, M32: 0.0 });
            rt.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
        }
        {
            let base: ID2D1RenderTarget = rt.cast()?;
            let mut p = Painter { rt: &base, fonts: &self.fonts, mouse: self.mouse, hits: &mut hits, accent: self.accent };
            ui::paint(&mut self.app, &mut p, w, h, s);
        }
        self.hits = hits;
        self.app.sync_title();
        let full = self.app.view.as_ref().is_some_and(|v| v.fullscreen());
        if full != self.placement.is_some() {
            self.set_full(full);
        }
        unsafe {
            if rt.EndDraw(None, None).is_err() {
                self.rt = None;
            }
            let toast = app::notice().is_some_and(|(_, at)| at.elapsed().as_millis() < 2600);
            let view = self.app.view.as_ref();
            if tween::busy() || toast {
                SetTimer(hwnd, TIMER_ANIM, 16, None);
            } else if view.is_some_and(|v| v.ticking()) {
                SetTimer(hwnd, TIMER_ANIM, 60, None);
            } else if view.is_some_and(|v| v.editing()) {
                SetTimer(hwnd, TIMER_ANIM, 530, None);
            }
        }
        Ok(())
    }
}

fn xy(lp: LPARAM) -> (f32, f32) {
    ((lp.0 & 0xFFFF) as i16 as f32, ((lp.0 >> 16) & 0xFFFF) as i16 as f32)
}

fn keys() -> Keys {
    // AltGr = Ctrl+Alt: Türkçe klavyede @, €, { gibi karakterler kısayol sayılmasın.
    unsafe {
        let alt = GetKeyState(VK_MENU.0 as i32) < 0;
        Keys { ctrl: GetKeyState(VK_CONTROL.0 as i32) < 0 && !alt, shift: GetKeyState(VK_SHIFT.0 as i32) < 0 }
    }
}

fn mouse_event(hwnd: HWND, m: Mouse, lp: LPARAM) {
    with(|a| {
        let (x, y) = xy(lp);
        a.mouse = (x / a.scale, y / a.scale);
        let k = keys();
        match m {
            Mouse::Down | Mouse::Double | Mouse::Right => {
                app::caret_reset();
                match a.hit_at(a.mouse) {
                    Some(Act::Content) | None => {
                        if let Some(v) = a.app.view.as_mut() {
                            if m != Mouse::Right {
                                a.captured = true;
                                unsafe {
                                    SetCapture(hwnd);
                                }
                            }
                            v.mouse(m, a.mouse, &k);
                        }
                    }
                    Some(act) if m != Mouse::Right => a.app.act(act),
                    Some(_) => {}
                }
            }
            Mouse::Move => {
                if a.captured {
                    if let Some(v) = a.app.view.as_mut() {
                        v.mouse(Mouse::Move, a.mouse, &k);
                    }
                }
            }
            Mouse::Up => {
                if a.captured {
                    a.captured = false;
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                    if let Some(v) = a.app.view.as_mut() {
                        v.mouse(Mouse::Up, a.mouse, &k);
                    }
                }
            }
        }
        a.invalidate();
    });
}

fn key_down(vk: u16) -> bool {
    with(|a| {
        let k = keys();
        app::caret_reset();
        let done = match (vk, k.ctrl, k.shift) {
            (0x53, true, false) => {
                a.app.act(Act::Save);
                true
            }
            (0x53, true, true) => {
                a.app.act(Act::SaveAs);
                true
            }
            (0x4F, true, _) => {
                a.app.act(Act::Open);
                true
            }
            (0x4E, true, _) => {
                let kind = a.app.view.as_ref().map(|v| v.ext());
                a.app.act(if kind == Some("docx") { Act::NewDoc } else { Act::NewSheet });
                true
            }
            _ => a.app.view.as_mut().is_some_and(|v| v.key(vk, &k)),
        };
        a.invalidate();
        done
    })
    .unwrap_or(false)
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
                a.mouse = (x / a.scale, y / a.scale);
                if !a.captured {
                    a.invalidate();
                }
            });
            mouse_event(hwnd, Mouse::Move, lp);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            mouse_event(hwnd, Mouse::Down, lp);
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            mouse_event(hwnd, Mouse::Double, lp);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            mouse_event(hwnd, Mouse::Up, lp);
            LRESULT(0)
        }
        WM_RBUTTONDOWN => {
            mouse_event(hwnd, Mouse::Right, lp);
            LRESULT(0)
        }
        WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
            with(|a| {
                let mut d = (wp.0 >> 16) as i16 as f32 / 120.0;
                let mut k = keys();
                if msg == WM_MOUSEHWHEEL {
                    // Yatay tekerlek: sağa = ileri.
                    d = -d;
                    k.shift = true;
                }
                if let Some(v) = a.app.view.as_mut() {
                    v.wheel(d, &k);
                } else {
                    a.app.act(Act::Scroll);
                }
                a.invalidate();
            });
            LRESULT(0)
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            if msg == WM_KEYDOWN && key_down(wp.0 as u16) {
                LRESULT(0)
            } else {
                DefWindowProcW(hwnd, msg, wp, lp)
            }
        }
        WM_CHAR => {
            let u = wp.0 as u16;
            let ch = with(|a| {
                if (0xD800..0xDC00).contains(&u) {
                    a.surrogate = Some(u);
                    return None;
                }
                if (0xDC00..0xE000).contains(&u) {
                    let hi = a.surrogate.take()?;
                    return char::decode_utf16([hi, u]).next()?.ok();
                }
                char::from_u32(u as u32)
            })
            .flatten();
            // Denetim karakterleri (Enter, Tab, geri silme, Ctrl+harf) tuş olarak işlendi.
            if let Some(c) = ch.filter(|c| !c.is_control()) {
                with(|a| {
                    if let Some(v) = a.app.view.as_mut() {
                        v.char(c);
                    }
                    app::caret_reset();
                    a.invalidate();
                });
            }
            LRESULT(0)
        }
        WM_DROPFILES => {
            let drop = HDROP(wp.0 as _);
            let n = DragQueryFileW(drop, u32::MAX, None);
            let mut files = Vec::new();
            for i in 0..n {
                let mut buf = vec![0u16; 1024];
                let l = DragQueryFileW(drop, i, Some(&mut buf)) as usize;
                files.push(std::path::PathBuf::from(String::from_utf16_lossy(&buf[..l])));
            }
            DragFinish(drop);
            with(|a| {
                for f in &files {
                    a.app.open(f);
                }
                a.invalidate();
            });
            LRESULT(0)
        }
        WM_CLOSE => {
            if with(|a| a.app.confirm_close()).unwrap_or(true) {
                let _ = DestroyWindow(hwnd);
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
            mm.ptMinTrackSize = POINT { x: 720, y: 480 };
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn main() -> Result<()> {
    log::init("office");
    let _ = config::load();
    let arg = std::env::args_os().nth(1);
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let inst: HINSTANCE = GetModuleHandleW(None)?.into();
        let icon = LoadIconW(inst, PCWSTR(1 as _)).or_else(|_| LoadIconW(None, IDI_APPLICATION))?;
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: Some(wnd_proc),
            hInstance: inst,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hIcon: icon,
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let dpi = GetDpiForSystem().max(96) as f32 / 96.0;
        let sw = GetSystemMetrics(SM_CXSCREEN);
        let sh = GetSystemMetrics(SM_CYSCREEN);
        let (ww, wh) = (((1180.0 * dpi) as i32).min(sw - 40), ((800.0 * dpi) as i32).min(sh - 60));
        let hwnd = CreateWindowExW(WS_EX_ACCEPTFILES, CLASS, w!("Carl Office"), WS_OVERLAPPEDWINDOW, (sw - ww) / 2, (sh - wh) / 2, ww, wh, None, None, inst, None)?;
        let on: i32 = 1;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &on as *const _ as _, 4);
        let backdrop = DWMSBT_TRANSIENTWINDOW;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, &backdrop as *const _ as _, 4);
        let _ = DwmExtendFrameIntoClientArea(hwnd, &MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 });
        DragAcceptFiles(hwnd, true);

        let win = Win {
            app: App::new(hwnd),
            rt: None,
            factory: D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?,
            fonts: Fonts::new()?,
            scale: GetDpiForWindow(hwnd).max(96) as f32 / 96.0,
            mouse: (-1.0, -1.0),
            hits: Vec::new(),
            accent: glassmenu::accent(),
            surrogate: None,
            captured: false,
            placement: None,
        };
        tween::enable();
        WIN.with(|a| *a.borrow_mut() = Some(win));
        match arg.as_ref().map(|a| a.to_string_lossy().into_owned()) {
            Some(a) if a == "--new-sheet" => {
                with(|w| w.app.act(Act::NewSheet));
            }
            Some(a) if a == "--new-doc" => {
                with(|w| w.app.act(Act::NewDoc));
            }
            Some(a) => {
                with(|w| w.app.open(Path::new(&a)));
            }
            None => {}
        }
        info!("Office penceresi açıldı");
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

/// Testler dil ayarını paylaşır; aynı anda koşanlar birbirini bozmasın.
#[cfg(test)]
pub fn test_lang(l: &str) -> std::sync::MutexGuard<'static, ()> {
    static M: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let g = M.lock().unwrap_or_else(|e| e.into_inner());
    carlui_core::lang::apply(l);
    g
}
