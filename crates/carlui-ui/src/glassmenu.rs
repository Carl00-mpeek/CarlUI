//! Bu modül "Windows'un standart menüsü dock'un cam görünümüne uymuyor, öğe
//! simgesi ve renkli başlık gösteremiyor" sorununu çözüyor. Menü kendi
//! penceremizde çizilir:
//! - Arka plan Windows 11'in kendi akrilik camıdır (DWM sistem arka planı),
//!   üstüne hafif koyu bir ton ve ince bir parlak kenar çizilir.
//! - Kategori başlıkları Windows'ta seçili vurgu renginin açık tonundadır.
//! - Öğeler uygulama logosu (WIC bitmap) ya da Fluent simgesi taşıyabilir.
//! `run` kendi mesaj döngüsünü çalıştırır ve seçilen öğenin kimliğini döndürür
//! (TrackPopupMenu gibi): dışarı tıklamak, Esc ya da odak kaybı iptal eder.
//! Ok tuşları + Enter ile klavyeden de kullanılır.

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
            Imaging::IWICFormatConverter,
        },
        System::{LibraryLoader::GetModuleHandleW, Registry::*},
        UI::{HiDpi::GetDpiForWindow, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};

pub enum Icon {
    None,
    /// Segoe Fluent Icons glifi ve isteğe bağlı renk (0xRRGGBB).
    Glyph(char, Option<u32>),
    Image(IWICFormatConverter),
}

pub enum Row {
    /// Vurgu renginde küçük başlık.
    Header(String),
    /// Soluk, tıklanamaz satır (uygulama adı gibi), simgeli.
    Label(String, Icon),
    Item { id: usize, text: String, icon: Icon },
    Sep,
}

const PAD: f32 = 5.0;
const ITEM_H: f32 = 32.0;
const HEAD_H: f32 = 28.0;
const SEP_H: f32 = 9.0;
const ICON: f32 = 18.0;
const MIN_W: f32 = 240.0;
const MAX_W: f32 = 420.0;

struct State {
    rows: Vec<Row>,
    /// Satırların dikey konumları (mantıksal piksel).
    tops: Vec<f32>,
    width: f32,
    scale: f32,
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

fn rgb(c: u32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: ((c >> 16) & 0xFF) as f32 / 255.0, g: ((c >> 8) & 0xFF) as f32 / 255.0, b: (c & 0xFF) as f32 / 255.0, a }
}

/// Windows vurgu renginin koyu zemin için açık tonu (Ayarlar > Kişiselleştirme > Renkler).
pub fn accent() -> u32 {
    unsafe {
        let mut buf = [0u8; 32];
        let mut len = buf.len() as u32;
        if RegGetValueW(HKEY_CURRENT_USER, w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Accent"), w!("AccentPalette"), RRF_RT_REG_BINARY, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut len)).is_ok() && len >= 8 {
            // Sıra: Açık3, Açık2, Açık1, Vurgu, …; her biri R G B A.
            let p = &buf[4..8];
            return (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32;
        }
    }
    0x60CDFF
}

fn text_format(dw: &IDWriteFactory, size: f32, weight: DWRITE_FONT_WEIGHT) -> Result<IDWriteTextFormat> {
    unsafe {
        let f = dw.CreateTextFormat(w!("Segoe UI Variable Text"), None, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("tr-tr")).or_else(|_| dw.CreateTextFormat(w!("Segoe UI"), None, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("tr-tr")))?;
        f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        let trim = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
        let _ = f.SetTrimming(&trim, None);
        Ok(f)
    }
}

fn text_width(dw: &IDWriteFactory, fmt: &IDWriteTextFormat, s: &str) -> f32 {
    let w: Vec<u16> = s.encode_utf16().collect();
    unsafe {
        dw.CreateTextLayout(&w, fmt, 2000.0, 100.0)
            .and_then(|l| {
                let mut m = DWRITE_TEXT_METRICS::default();
                l.GetMetrics(&mut m).map(|_| m.widthIncludingTrailingWhitespace)
            })
            .unwrap_or(s.chars().count() as f32 * 7.5)
    }
}

impl State {
    fn height(&self) -> f32 {
        self.tops.last().copied().unwrap_or(0.0) + PAD
    }

    fn row_h(r: &Row) -> f32 {
        match r {
            Row::Header(_) => HEAD_H,
            Row::Sep => SEP_H,
            _ => ITEM_H,
        }
    }

    fn hit(&self, y: f32) -> Option<usize> {
        (0..self.rows.len()).find(|&i| matches!(self.rows[i], Row::Item { .. }) && y >= self.tops[i] && y < self.tops[i + 1])
    }

    fn step(&mut self, dir: i32) {
        let items: Vec<usize> = (0..self.rows.len()).filter(|&i| matches!(self.rows[i], Row::Item { .. })).collect();
        if items.is_empty() {
            return;
        }
        let pos = self.hover.and_then(|h| items.iter().position(|&i| i == h));
        let next = match (pos, dir) {
            (None, d) if d > 0 => 0,
            (None, _) => items.len() - 1,
            (Some(p), d) => (p as i32 + d).rem_euclid(items.len() as i32) as usize,
        };
        self.hover = Some(items[next]);
    }

    fn paint(&mut self, hwnd: HWND) -> Result<()> {
        let s = self.scale;
        let (w, h) = ((self.width * s).ceil() as u32, (self.height() * s).ceil() as u32);
        if self.rt.is_none() {
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            };
            let hp = D2D1_HWND_RENDER_TARGET_PROPERTIES { hwnd, pixelSize: D2D_SIZE_U { width: w, height: h }, presentOptions: D2D1_PRESENT_OPTIONS_NONE };
            let rt = unsafe { self.factory.CreateHwndRenderTarget(&props, &hp)? };
            unsafe { rt.SetTransform(&windows::Foundation::Numerics::Matrix3x2 { M11: s, M12: 0.0, M21: 0.0, M22: s, M31: 0.0, M32: 0.0 }) };
            self.bitmaps = self
                .rows
                .iter()
                .map(|r| match r {
                    Row::Item { icon: Icon::Image(img), .. } | Row::Label(_, Icon::Image(img)) => unsafe { rt.CreateBitmapFromWicBitmap(img, None).ok() },
                    _ => None,
                })
                .collect();
            self.rt = Some(rt);
        }
        let rt = self.rt.clone().unwrap();
        let text = text_format(&self.dw, 13.5, DWRITE_FONT_WEIGHT_NORMAL)?;
        let head = text_format(&self.dw, 12.0, DWRITE_FONT_WEIGHT_SEMI_BOLD)?;
        let glyphs = unsafe {
            self.dw
                .CreateTextFormat(w!("Segoe Fluent Icons"), None, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, 15.0, w!("tr-tr"))
                .or_else(|_| self.dw.CreateTextFormat(w!("Segoe MDL2 Assets"), None, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, 15.0, w!("tr-tr")))?
        };
        unsafe {
            glyphs.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            glyphs.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        }
        let w = self.width;
        let ht = self.height();
        unsafe {
            rt.BeginDraw();
            rt.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            let brush = |c: D2D1_COLOR_F| rt.CreateSolidColorBrush(&c, None);
            // Cam: akriliğin üstüne hafif koyu ton, üstte ince parlama.
            let all = D2D_RECT_F { left: 0.0, top: 0.0, right: w, bottom: ht };
            rt.FillRectangle(&all, &brush(rgb(0x1A1A1E, 0.38))?);
            let shine = rt.CreateGradientStopCollection(
                &[D2D1_GRADIENT_STOP { position: 0.0, color: rgb(0xFFFFFF, 0.07) }, D2D1_GRADIENT_STOP { position: 1.0, color: rgb(0xFFFFFF, 0.0) }],
                D2D1_GAMMA_2_2,
                D2D1_EXTEND_MODE_CLAMP,
            )?;
            let grad = rt.CreateLinearGradientBrush(&D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: D2D_POINT_2F { x: 0.0, y: 0.0 }, endPoint: D2D_POINT_2F { x: 0.0, y: 90.0 } }, None, &shine)?;
            rt.FillRectangle(&D2D_RECT_F { bottom: 90.0f32.min(ht), ..all }, &grad);
            let white = brush(rgb(0xFFFFFF, 0.93))?;
            let dim = brush(rgb(0xFFFFFF, 0.55))?;
            let accent = brush(self.accent)?;
            let line = brush(rgb(0xFFFFFF, 0.10))?;
            let hover = brush(rgb(0xFFFFFF, 0.10))?;
            for (i, r) in self.rows.iter().enumerate() {
                let (y0, y1) = (self.tops[i], self.tops[i + 1]);
                let mid = (y0 + y1) / 2.0;
                let draw_text = |s: &str, x: f32, fmt: &IDWriteTextFormat, b: &ID2D1SolidColorBrush| {
                    let t: Vec<u16> = s.encode_utf16().collect();
                    rt.DrawText(&t, fmt, &D2D_RECT_F { left: x, top: y0, right: w - 12.0, bottom: y1 }, b, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
                };
                let draw_icon = |icon: &Icon| match icon {
                    Icon::Glyph(c, col) => {
                        let t: Vec<u16> = c.to_string().encode_utf16().collect();
                        let b = col.map(|c| brush(rgb(c, 1.0)).ok()).flatten();
                        rt.DrawText(&t, &glyphs, &D2D_RECT_F { left: 12.0, top: y0, right: 12.0 + ICON + 6.0, bottom: y1 }, b.as_ref().unwrap_or(&white), D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                    }
                    Icon::Image(_) => {
                        if let Some(Some(bmp)) = self.bitmaps.get(i) {
                            let x = 15.0;
                            rt.DrawBitmap(bmp, Some(&D2D_RECT_F { left: x, top: mid - ICON / 2.0, right: x + ICON, bottom: mid + ICON / 2.0 }), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
                        }
                    }
                    Icon::None => {}
                };
                match r {
                    Row::Header(t) => draw_text(t, 14.0, &head, &accent),
                    Row::Sep => rt.FillRectangle(&D2D_RECT_F { left: 10.0, top: mid - 0.5, right: w - 10.0, bottom: mid + 0.5 }, &line),
                    Row::Label(t, icon) => {
                        draw_icon(icon);
                        draw_text(t, 44.0, &text, &dim);
                    }
                    Row::Item { text: t, icon, .. } => {
                        if self.hover == Some(i) {
                            let rr = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: PAD, top: y0 + 1.0, right: w - PAD, bottom: y1 - 1.0 }, radiusX: 5.0, radiusY: 5.0 };
                            rt.FillRoundedRectangle(&rr, &hover);
                        }
                        draw_icon(icon);
                        draw_text(t, 44.0, &text, &white);
                    }
                }
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
                let (_, y) = xy();
                let h = s.hit(y / s.scale);
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
                let (_, y) = xy();
                if let Some(i) = s.hit(y / s.scale) {
                    if let Row::Item { id, .. } = s.rows[i] {
                        s.result = Some(id);
                        s.done = true;
                    }
                }
            }
            WM_KEYDOWN => match VIRTUAL_KEY(wp.0 as u16) {
                VK_ESCAPE => s.done = true,
                VK_DOWN => {
                    s.step(1);
                    let _ = InvalidateRect(hwnd, None, false);
                }
                VK_UP => {
                    s.step(-1);
                    let _ = InvalidateRect(hwnd, None, false);
                }
                VK_RETURN | VK_SPACE => {
                    if let Some(Row::Item { id, .. }) = s.hover.map(|h| &s.rows[h]) {
                        s.result = Some(*id);
                        s.done = true;
                    }
                }
                _ => {}
            },
            WM_ACTIVATE if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE => s.done = true,
            WM_MOUSEACTIVATE => {
                handled = false;
            }
            _ => handled = false,
        }
    });
    if handled { LRESULT(0) } else { DefWindowProcW(hwnd, msg, wp, lp) }
}

fn register() -> PCWSTR {
    let name = w!("CarlUI.GlassMenu");
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

/// Menüyü `at` noktasının üstüne, yatayda ortalı açar; seçilen öğenin kimliği.
/// `accent`: başlık rengi (0xRRGGBB), `None` ise renksiz (soluk beyaz).
pub fn run(owner: HWND, rows: Vec<Row>, accent: Option<u32>, at: (i32, i32)) -> Option<usize> {
    let factory: ID2D1Factory = unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok()? };
    let dw: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()? };
    let scale = unsafe { GetDpiForWindow(owner) }.max(96) as f32 / 96.0;
    let body = text_format(&dw, 13.5, DWRITE_FONT_WEIGHT_NORMAL).ok()?;
    let mut width: f32 = MIN_W;
    let mut tops = vec![PAD];
    for r in &rows {
        let tw = match r {
            Row::Item { text, .. } | Row::Label(text, _) | Row::Header(text) => text_width(&dw, &body, text) + 44.0 + 22.0,
            Row::Sep => 0.0,
        };
        width = width.max(tw);
        tops.push(tops.last().unwrap() + State::row_h(r));
    }
    let width = width.min(MAX_W).ceil();
    let st = State { rows, tops, width, scale, hover: None, result: None, done: false, accent: accent.map(|c| rgb(c, 1.0)).unwrap_or(rgb(0xFFFFFF, 0.55)), factory, dw, rt: None, bitmaps: Vec::new() };
    let (pw, ph) = ((st.width * scale).ceil() as i32, (st.height() * scale).ceil() as i32);
    // Ekrana sığdır.
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe {
        let _ = GetMonitorInfoW(MonitorFromPoint(POINT { x: at.0, y: at.1 }, MONITOR_DEFAULTTONEAREST), &mut mi);
    }
    let x = (at.0 - pw / 2).clamp(mi.rcMonitor.left + 4, mi.rcMonitor.right - pw - 4);
    let y = (at.1 - ph).max(mi.rcMonitor.top + 4);
    STATE.with(|s| *s.borrow_mut() = Some(st));
    let class = register();
    unsafe {
        let inst: HINSTANCE = GetModuleHandleW(None).unwrap_or_default().into();
        let hwnd = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW, class, w!(""), WS_POPUP, x, y, pw, ph, owner, None, inst, None).ok()?;
        let on: i32 = 1;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &on as *const _ as _, 4);
        let round = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &round as *const _ as _, 4);
        let backdrop = DWMSBT_TRANSIENTWINDOW;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, &backdrop as *const _ as _, 4);
        let border: u32 = 0x00505050;
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
            // Menü dışına tıklama: iptal (tıklama da yerine ulaşsın).
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
