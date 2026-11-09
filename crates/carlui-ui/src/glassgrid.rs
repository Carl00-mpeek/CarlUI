//! Simge ızgaralı cam açılır pencere (dock'taki klasör yığınları için).
//!
//! Cam menüyle (`glassmenu`) aynı cam ve renkleri kullanır, ama öğeler alt
//! alta yazı satırı yerine yan yana büyük simge + altında adı olarak dizilir.
//! Üstte klasörün adı ve "Klasörü aç" düğmesi durur. `run_side` kendi mesaj
//! döngüsünü çalıştırır ve seçilen öğenin kimliğini döndürür: dışarı
//! tıklamak, Esc ya da odak kaybı iptal eder. Ok tuşları + Enter da çalışır.

use crate::glassmenu::{pen, rgb, text_format, Icon};
use std::cell::RefCell;
use windows::{
    core::*,
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
        UI::{HiDpi::GetDpiForWindow, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};

pub struct Cell {
    pub id: usize,
    pub text: String,
    pub icon: Icon,
}

const PAD: f32 = 10.0;
const HEAD_H: f32 = 40.0;
const CELL_W: f32 = 86.0;
const CELL_H: f32 = 92.0;
const ICON: f32 = 44.0;
const MAX_COLS: usize = 5;
const MIN_W: f32 = 300.0;
const EMPTY_H: f32 = 56.0;

struct State {
    title: String,
    action: (usize, String),
    empty: String,
    cells: Vec<Cell>,
    cols: usize,
    width: f32,
    height: f32,
    /// Izgaranın sol kenarı (ortalanır).
    grid_x: f32,
    /// "Klasörü aç" düğmesinin dikdörtgeni.
    button: D2D_RECT_F,
    scale: f32,
    /// 0: düğme, 1..: hücre (k + 1).
    hover: Option<usize>,
    result: Option<usize>,
    done: bool,
    accent: D2D1_COLOR_F,
    factory: ID2D1Factory,
    dw: IDWriteFactory,
    rt: Option<ID2D1HwndRenderTarget>,
    bitmaps: Vec<Option<ID2D1Bitmap>>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn inside(r: &D2D_RECT_F, x: f32, y: f32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

impl State {
    fn cell_rect(&self, k: usize) -> D2D_RECT_F {
        let (c, r) = ((k % self.cols) as f32, (k / self.cols) as f32);
        let left = self.grid_x + c * CELL_W;
        let top = HEAD_H + r * CELL_H;
        D2D_RECT_F { left, top, right: left + CELL_W, bottom: top + CELL_H }
    }

    fn hit(&self, x: f32, y: f32) -> Option<usize> {
        if inside(&self.button, x, y) {
            return Some(0);
        }
        (0..self.cells.len()).find(|&k| inside(&self.cell_rect(k), x, y)).map(|k| k + 1)
    }

    fn id_of(&self, h: usize) -> usize {
        if h == 0 { self.action.0 } else { self.cells[h - 1].id }
    }

    /// Ok tuşlarıyla gezinme: düğme ızgaranın üstündedir.
    fn step(&mut self, dx: i32, dy: i32) {
        let n = self.cells.len() as i32;
        let cols = self.cols as i32;
        self.hover = Some(match self.hover {
            None => {
                if n > 0 { 1 } else { 0 }
            }
            Some(0) => {
                if dy > 0 && n > 0 { 1 } else { 0 }
            }
            Some(h) => {
                let k = h as i32 - 1;
                if dy < 0 && k < cols {
                    0
                } else {
                    let next = (k + dx + dy * cols).clamp(0, n - 1);
                    next as usize + 1
                }
            }
        });
    }

    fn paint(&mut self, hwnd: HWND) -> Result<()> {
        let s = self.scale;
        let (w, h) = (self.width, self.height);
        if self.rt.is_none() {
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            };
            let hp = D2D1_HWND_RENDER_TARGET_PROPERTIES { hwnd, pixelSize: D2D_SIZE_U { width: (w * s).ceil() as u32, height: (h * s).ceil() as u32 }, presentOptions: D2D1_PRESENT_OPTIONS_NONE };
            let rt = unsafe { self.factory.CreateHwndRenderTarget(&props, &hp)? };
            unsafe { rt.SetTransform(&windows::Foundation::Numerics::Matrix3x2 { M11: s, M12: 0.0, M21: 0.0, M22: s, M31: 0.0, M32: 0.0 }) };
            self.bitmaps = self
                .cells
                .iter()
                .map(|c| match &c.icon {
                    Icon::Image(img) => unsafe { rt.CreateBitmapFromWicBitmap(img, None).ok() },
                    _ => None,
                })
                .collect();
            self.rt = Some(rt);
        }
        let rt = self.rt.clone().unwrap();
        let title = text_format(&self.dw, 14.0, DWRITE_FONT_WEIGHT_SEMI_BOLD)?;
        let button = text_format(&self.dw, 12.5, DWRITE_FONT_WEIGHT_NORMAL)?;
        let name = text_format(&self.dw, 11.5, DWRITE_FONT_WEIGHT_NORMAL)?;
        let glyphs = unsafe {
            self.dw
                .CreateTextFormat(w!("Segoe Fluent Icons"), None, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, 30.0, w!("tr-tr"))
                .or_else(|_| self.dw.CreateTextFormat(w!("Segoe MDL2 Assets"), None, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, 30.0, w!("tr-tr")))?
        };
        unsafe {
            glyphs.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            glyphs.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            button.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            // Ad iki satıra kadar, ortalı; sığmayan "…" ile biter.
            name.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            name.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)?;
            name.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP)?;
            if let Ok(sign) = self.dw.CreateEllipsisTrimmingSign(&name) {
                let trim = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
                let _ = name.SetTrimming(&trim, &sign);
            }
        }
        unsafe {
            rt.BeginDraw();
            rt.Clear(Some(&crate::tone::backdrop()));
            let brush = |c: D2D1_COLOR_F| rt.CreateSolidColorBrush(&c, None);
            let all = D2D_RECT_F { left: 0.0, top: 0.0, right: w, bottom: h };
            rt.FillRectangle(&all, &brush(rgb(0x1A1A1E, 0.38))?);
            let shine = rt.CreateGradientStopCollection(
                &[D2D1_GRADIENT_STOP { position: 0.0, color: rgb(0xFFFFFF, 0.07) }, D2D1_GRADIENT_STOP { position: 1.0, color: rgb(0xFFFFFF, 0.0) }],
                D2D1_GAMMA_2_2,
                D2D1_EXTEND_MODE_CLAMP,
            )?;
            let grad = rt.CreateLinearGradientBrush(&D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: D2D_POINT_2F { x: 0.0, y: 0.0 }, endPoint: D2D_POINT_2F { x: 0.0, y: 90.0 } }, None, &shine)?;
            rt.FillRectangle(&D2D_RECT_F { bottom: 90.0f32.min(h), ..all }, &grad);
            let white = brush(pen(0xFFFFFF, 0.93))?;
            let dim = brush(pen(0xFFFFFF, 0.6))?;
            let accent = brush(self.accent)?;
            let hover = brush(rgb(0xFFFFFF, 0.11))?;
            let pill = brush(rgb(0xFFFFFF, 0.06))?;
            let line = brush(rgb(0xFFFFFF, 0.10))?;
            let text = |s: &str, r: D2D_RECT_F, fmt: &IDWriteTextFormat, b: &ID2D1SolidColorBrush| {
                let t: Vec<u16> = s.encode_utf16().collect();
                rt.DrawText(&t, fmt, &r, b, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
            };

            // Başlık ve düğme.
            let b = self.button;
            text(&self.title, D2D_RECT_F { left: PAD + 4.0, top: 0.0, right: b.left - 8.0, bottom: HEAD_H }, &title, &accent);
            let rr = D2D1_ROUNDED_RECT { rect: b, radiusX: (b.bottom - b.top) / 2.0, radiusY: (b.bottom - b.top) / 2.0 };
            rt.FillRoundedRectangle(&rr, if self.hover == Some(0) { &hover } else { &pill });
            text(&self.action.1, b, &button, &white);
            rt.FillRectangle(&D2D_RECT_F { left: PAD, top: HEAD_H - 0.5, right: w - PAD, bottom: HEAD_H + 0.5 }, &line);

            if self.cells.is_empty() {
                text(&self.empty, D2D_RECT_F { left: PAD, top: HEAD_H, right: w - PAD, bottom: h - PAD / 2.0 }, &button, &dim);
            }
            for (k, c) in self.cells.iter().enumerate() {
                let r = self.cell_rect(k);
                if self.hover == Some(k + 1) {
                    let rr = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: r.left + 3.0, top: r.top + 3.0, right: r.right - 3.0, bottom: r.bottom - 3.0 }, radiusX: 8.0, radiusY: 8.0 };
                    rt.FillRoundedRectangle(&rr, &hover);
                }
                let cx = (r.left + r.right) / 2.0;
                let icon = D2D_RECT_F { left: cx - ICON / 2.0, top: r.top + 8.0, right: cx + ICON / 2.0, bottom: r.top + 8.0 + ICON };
                match (&c.icon, self.bitmaps.get(k)) {
                    (Icon::Image(_), Some(Some(bmp))) => rt.DrawBitmap(bmp, Some(&icon), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None),
                    (Icon::Glyph(g, col), _) => {
                        let b = col.and_then(|c| brush(pen(c, 1.0)).ok());
                        let t: Vec<u16> = g.to_string().encode_utf16().collect();
                        rt.DrawText(&t, &glyphs, &icon, b.as_ref().unwrap_or(&white), D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                    }
                    _ => {
                        // Simgesi gelmeyen (yavaş sürücü) dosya için sade bir sayfa.
                        let t: Vec<u16> = '\u{E8A5}'.to_string().encode_utf16().collect();
                        rt.DrawText(&t, &glyphs, &icon, &dim, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                    }
                }
                text(&c.text, D2D_RECT_F { left: r.left + 5.0, top: icon.bottom + 5.0, right: r.right - 5.0, bottom: r.bottom - 4.0 }, &name, &white);
            }
            if rt.EndDraw(None, None).is_err() {
                self.rt = None;
            }
        }
        Ok(())
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let xy = || ((lp.0 & 0xFFFF) as i16 as f32, ((lp.0 >> 16) & 0xFFFF) as i16 as f32);
    let mut handled = true;
    STATE.with(|st| {
        let Ok(mut g) = st.try_borrow_mut() else { handled = false; return };
        let Some(s) = g.as_mut() else { handled = false; return };
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                BeginPaint(hwnd, &mut ps);
                let _ = s.paint(hwnd);
                let _ = EndPaint(hwnd, &ps);
            }
            WM_MOUSEMOVE => {
                let (x, y) = xy();
                let h = s.hit(x / s.scale, y / s.scale);
                if h != s.hover {
                    s.hover = h;
                    let _ = InvalidateRect(hwnd, None, false);
                }
                let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut tme);
            }
            0x02A3 /* WM_MOUSELEAVE */ => {
                s.hover = None;
                let _ = InvalidateRect(hwnd, None, false);
            }
            WM_LBUTTONUP => {
                let (x, y) = xy();
                if let Some(h) = s.hit(x / s.scale, y / s.scale) {
                    s.result = Some(s.id_of(h));
                    s.done = true;
                }
            }
            WM_KEYDOWN => {
                let mv = match VIRTUAL_KEY(wp.0 as u16) {
                    VK_ESCAPE => {
                        s.done = true;
                        None
                    }
                    VK_LEFT => Some((-1, 0)),
                    VK_RIGHT => Some((1, 0)),
                    VK_UP => Some((0, -1)),
                    VK_DOWN => Some((0, 1)),
                    VK_RETURN | VK_SPACE => {
                        if let Some(h) = s.hover {
                            s.result = Some(s.id_of(h));
                            s.done = true;
                        }
                        None
                    }
                    _ => None,
                };
                if let Some((dx, dy)) = mv {
                    s.step(dx, dy);
                    let _ = InvalidateRect(hwnd, None, false);
                }
            }
            WM_ACTIVATE if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE => s.done = true,
            _ => handled = false,
        }
    });
    if handled { LRESULT(0) } else { DefWindowProcW(hwnd, msg, wp, lp) }
}

fn register() -> PCWSTR {
    let name = w!("CarlUI.GlassGrid");
    unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(proc),
            hInstance: GetModuleHandleW(None).unwrap_or_default().into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: name,
            ..Default::default()
        };
        RegisterClassExW(&wc);
    }
    name
}

fn text_width(dw: &IDWriteFactory, fmt: &IDWriteTextFormat, s: &str) -> f32 {
    let w: Vec<u16> = s.encode_utf16().collect();
    unsafe {
        dw.CreateTextLayout(&w, fmt, 2000.0, 100.0)
            .and_then(|l| {
                let mut m = DWRITE_TEXT_METRICS::default();
                l.GetMetrics(&mut m).map(|_| m.widthIncludingTrailingWhitespace)
            })
            .unwrap_or(s.chars().count() as f32 * 7.0)
    }
}

/// Izgarayı açar. `side` 0: `at` noktasının üstünde ortalı, 1: sağında,
/// -1: solunda (yan dock). `action`: başlıktaki düğmenin kimliği ve yazısı,
/// `empty`: hücre yoksa gösterilen yazı. `accent`: başlık rengi.
pub fn run_side(owner: HWND, title: &str, action: (usize, String), empty: &str, cells: Vec<Cell>, accent: Option<u32>, at: (i32, i32), side: i32) -> Option<usize> {
    let factory: ID2D1Factory = unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok()? };
    let dw: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()? };
    let scale = unsafe { GetDpiForWindow(owner) }.max(96) as f32 / 96.0;

    let cols = cells.len().clamp(1, MAX_COLS);
    let rows = cells.len().div_ceil(cols);
    let fmt = text_format(&dw, 12.5, DWRITE_FONT_WEIGHT_NORMAL).ok()?;
    let bw = text_width(&dw, &fmt, &action.1) + 28.0;
    let head = text_format(&dw, 14.0, DWRITE_FONT_WEIGHT_SEMI_BOLD).ok()?;
    let need = PAD + 4.0 + text_width(&dw, &head, title) + 16.0 + bw + PAD;
    let width = (cols as f32 * CELL_W + PAD * 2.0).max(MIN_W).max(need.min(480.0)).ceil();
    let height = (HEAD_H + if cells.is_empty() { EMPTY_H } else { rows as f32 * CELL_H } + PAD / 2.0).ceil();
    let grid_x = ((width - cols as f32 * CELL_W) / 2.0).floor();
    let button = D2D_RECT_F { left: width - PAD - bw, top: 8.0, right: width - PAD, bottom: HEAD_H - 8.0 };

    let st = State {
        title: title.to_string(),
        action,
        empty: empty.to_string(),
        cells,
        cols,
        width,
        height,
        grid_x,
        button,
        scale,
        hover: None,
        result: None,
        done: false,
        accent: accent.map(|c| pen(c, 1.0)).unwrap_or(pen(0xFFFFFF, 0.7)),
        factory,
        dw,
        rt: None,
        bitmaps: Vec::new(),
    };
    let (pw, ph) = ((width * scale).ceil() as i32, (height * scale).ceil() as i32);
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe {
        let _ = GetMonitorInfoW(MonitorFromPoint(POINT { x: at.0, y: at.1 }, MONITOR_DEFAULTTONEAREST), &mut mi);
    }
    let mr = mi.rcMonitor;
    let (x, y) = match side {
        0 => ((at.0 - pw / 2).clamp(mr.left + 4, (mr.right - pw - 4).max(mr.left)), (at.1 - ph).max(mr.top + 4)),
        s => {
            let x = if s > 0 { at.0 } else { at.0 - pw };
            (x.clamp(mr.left + 4, (mr.right - pw - 4).max(mr.left)), (at.1 - ph / 2).clamp(mr.top + 4, (mr.bottom - ph - 4).max(mr.top)))
        }
    };
    STATE.with(|s| *s.borrow_mut() = Some(st));
    let class = register();
    unsafe {
        let inst: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();
        let hwnd = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW, class, w!(""), WS_POPUP, x, y, pw, ph, owner, None, inst, None).ok()?;
        let on = i32::from(!crate::tone::light());
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &on as *const _ as _, 4);
        let round = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &round as *const _ as _, 4);
        let backdrop = DWMSBT_TRANSIENTWINDOW;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, &backdrop as *const _ as _, 4);
        let border: u32 = if crate::tone::light() { 0x00E4DCD8 } else { 0x00505050 };
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, &border as *const _ as _, 4);
        let _ = DwmExtendFrameIntoClientArea(hwnd, &windows::Win32::UI::Controls::MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 });
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(hwnd);
        let mut msg = MSG::default();
        while !STATE.with(|s| s.borrow().as_ref().map(|s| s.done).unwrap_or(true)) {
            if !GetMessageW(&mut msg, None, 0, 0).as_bool() {
                PostQuitMessage(msg.wParam.0 as i32);
                break;
            }
            if matches!(msg.message, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_NCLBUTTONDOWN | WM_NCRBUTTONDOWN) && msg.hwnd != hwnd {
                STATE.with(|s| s.borrow_mut().as_mut().map(|s| s.done = true));
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = DestroyWindow(hwnd);
    }
    STATE.with(|s| s.borrow_mut().take()).and_then(|s| s.result)
}
