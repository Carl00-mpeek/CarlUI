//! Bu modül "ekrandaki bir rengin kodunu öğrenmek için ayrı program açmak"
//! sorununu çözüyor: kısayola (`shortcuts.color_picker`, varsayılan kapalı)
//! basınca tüm ekranlar dondurulur, imlecin yanında büyüteç ve renk kodu
//! görünür. Tıklama ya da Enter rengi ayardaki biçimde (HEX/RGB/HSL) panoya
//! kopyalar; Tab biçimi değiştirir, ok tuşları imleci bir piksel kaydırır,
//! Esc ya da sağ tık vazgeçer. Dondurulmuş kare GDI ile çizilir; yalnızca
//! büyütecin eski ve yeni yeri yeniden boyanır.

use crate::{color, note};
use carlui_core::config::{self, ColorFormat};
use std::cell::RefCell;
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};

/// Büyüteçte görünen kaynak piksel (tek sayı: ortası seçilen piksel).
const CELLS: i32 = 15;
const ZOOM: i32 = 9;

struct Picker {
    /// Sanal ekranın sol üstü (fiziksel piksel).
    vx: i32,
    vy: i32,
    w: i32,
    h: i32,
    mem: HDC,
    bmp: HBITMAP,
    bits: *const u8,
    cursor: POINT,
    last: RECT,
    format: ColorFormat,
    picked: Option<(u8, u8, u8)>,
    font: HFONT,
}

thread_local! {
    static P: RefCell<Option<Picker>> = const { RefCell::new(None) };
}

impl Picker {
    fn pixel(&self, x: i32, y: i32) -> (u8, u8, u8) {
        let (x, y) = ((x - self.vx).clamp(0, self.w - 1), (y - self.vy).clamp(0, self.h - 1));
        let i = ((y * self.w + x) * 4) as usize;
        unsafe { (*self.bits.add(i + 2), *self.bits.add(i + 1), *self.bits.add(i)) }
    }

    fn scale_at(p: POINT) -> f32 {
        let mon = unsafe { MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST) };
        let (mut dx, mut dy) = (96u32, 96u32);
        unsafe {
            let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        }
        dx.max(96) as f32 / 96.0
    }

    /// Büyütecin ve altındaki etiketin istemci koordinatlarındaki yeri.
    fn lens_rect(&self) -> (RECT, i32) {
        let s = Self::scale_at(self.cursor);
        let cell = ((ZOOM as f32) * s).round() as i32;
        let size = cell * CELLS;
        let label = (30.0 * s) as i32;
        let gap = (22.0 * s) as i32;
        let (cx, cy) = (self.cursor.x - self.vx, self.cursor.y - self.vy);
        // Monitörün içinde kalsın: sağa/alta sığmazsa sola/üste aç.
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        unsafe {
            let _ = GetMonitorInfoW(MonitorFromPoint(self.cursor, MONITOR_DEFAULTTONEAREST), &mut mi);
        }
        let m = mi.rcMonitor;
        let mut x = cx + gap;
        let mut y = cy + gap;
        if x + size > m.right - self.vx {
            x = cx - gap - size;
        }
        if y + size + label > m.bottom - self.vy {
            y = cy - gap - size - label;
        }
        (RECT { left: x, top: y, right: x + size, bottom: y + size + label }, cell)
    }

    fn draw_lens(&self, dc: HDC, ox: i32, oy: i32) {
        let (r, cell) = self.lens_rect();
        let size = cell * CELLS;
        let (x, y) = (r.left - ox, r.top - oy);
        let half = CELLS / 2;
        unsafe {
            SetStretchBltMode(dc, COLORONCOLOR);
            let _ = StretchBlt(dc, x, y, size, size, self.mem, self.cursor.x - self.vx - half, self.cursor.y - self.vy - half, CELLS, CELLS, SRCCOPY);
            // Izgara ve ortadaki pikselin çerçevesi.
            let grid = CreatePen(PS_SOLID, 1, COLORREF(0x404040));
            let old = SelectObject(dc, grid);
            for i in 1..CELLS {
                let _ = MoveToEx(dc, x + i * cell, y, None);
                let _ = LineTo(dc, x + i * cell, y + size);
                let _ = MoveToEx(dc, x, y + i * cell, None);
                let _ = LineTo(dc, x + size, y + i * cell);
            }
            let (px, py) = (x + half * cell, y + half * cell);
            let (cr, cg, cb) = self.pixel(self.cursor.x, self.cursor.y);
            let lum = 0.299 * cr as f32 + 0.587 * cg as f32 + 0.114 * cb as f32;
            let mark = CreatePen(PS_SOLID, 2, COLORREF(if lum > 140.0 { 0x000000 } else { 0xFFFFFF }));
            SelectObject(dc, mark);
            let nb = SelectObject(dc, GetStockObject(NULL_BRUSH));
            let _ = Rectangle(dc, px, py, px + cell + 1, py + cell + 1);
            // Dış çerçeve.
            let frame = CreatePen(PS_SOLID, 1, COLORREF(0x707070));
            SelectObject(dc, frame);
            let _ = Rectangle(dc, x - 1, y - 1, x + size + 1, r.bottom - oy + 1);
            SelectObject(dc, nb);
            SelectObject(dc, old);
            let _ = DeleteObject(grid);
            let _ = DeleteObject(mark);
            let _ = DeleteObject(frame);
            // Etiket: renk örneği + kod.
            let lr = RECT { left: x, top: y + size, right: x + size, bottom: r.bottom - oy };
            let bg = CreateSolidBrush(COLORREF(0x1A1616));
            FillRect(dc, &lr, bg);
            let _ = DeleteObject(bg);
            let pad = (lr.bottom - lr.top) / 5;
            let sw = RECT { left: lr.left + pad, top: lr.top + pad, right: lr.left + (lr.bottom - lr.top) - pad, bottom: lr.bottom - pad };
            let swb = CreateSolidBrush(COLORREF(cr as u32 | (cg as u32) << 8 | (cb as u32) << 16));
            FillRect(dc, &sw, swb);
            let _ = DeleteObject(swb);
            let of = SelectObject(dc, self.font);
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, COLORREF(0xF0F0F0));
            let mut tr = RECT { left: sw.right + pad, ..lr };
            let mut text: Vec<u16> = color::format((cr, cg, cb), self.format).encode_utf16().collect();
            DrawTextW(dc, &mut text, &mut tr, DT_SINGLELINE | DT_VCENTER | DT_LEFT | DT_NOPREFIX);
            SelectObject(dc, of);
        }
    }

    fn paint(&self, hwnd: HWND) {
        unsafe {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut ps);
            let r = ps.rcPaint;
            let (w, h) = ((r.right - r.left).max(1), (r.bottom - r.top).max(1));
            // Titremesin diye boyanacak bölge önce arka tamponda hazırlanır.
            let back = CreateCompatibleDC(dc);
            let bb = CreateCompatibleBitmap(dc, w, h);
            let old = SelectObject(back, bb);
            let _ = BitBlt(back, 0, 0, w, h, self.mem, r.left, r.top, SRCCOPY);
            self.draw_lens(back, r.left, r.top);
            let _ = BitBlt(dc, r.left, r.top, w, h, back, 0, 0, SRCCOPY);
            SelectObject(back, old);
            let _ = DeleteObject(bb);
            let _ = DeleteDC(back);
            let _ = EndPaint(hwnd, &ps);
        }
    }

    fn moved(&mut self, hwnd: HWND, p: POINT) {
        if p.x == self.cursor.x && p.y == self.cursor.y {
            return;
        }
        self.cursor = p;
        let (now, _) = self.lens_rect();
        unsafe {
            let grow = |r: RECT| RECT { left: r.left - 3, top: r.top - 3, right: r.right + 3, bottom: r.bottom + 3 };
            let _ = InvalidateRect(hwnd, Some(&grow(self.last)), false);
            let _ = InvalidateRect(hwnd, Some(&grow(now)), false);
        }
        self.last = now;
    }
}

fn with<T>(f: impl FnOnce(&mut Picker) -> T) -> Option<T> {
    P.with(|p| p.try_borrow_mut().ok()?.as_mut().map(f))
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            with(|p| p.paint(hwnd));
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEMOVE => {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            with(|p| p.moved(hwnd, pt));
            LRESULT(0)
        }
        WM_SETCURSOR => {
            if let Ok(c) = LoadCursorW(None, IDC_CROSS) {
                SetCursor(c);
            }
            LRESULT(1)
        }
        WM_LBUTTONDOWN => {
            with(|p| p.picked = Some(p.pixel(p.cursor.x, p.cursor.y)));
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_RBUTTONDOWN => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vk = VIRTUAL_KEY(wp.0 as u16);
            let step = |dx: i32, dy: i32| {
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);
                let _ = SetCursorPos(pt.x + dx, pt.y + dy);
            };
            let n = if GetKeyState(VK_SHIFT.0 as i32) < 0 { 10 } else { 1 };
            match vk {
                VK_ESCAPE => {
                    let _ = DestroyWindow(hwnd);
                }
                VK_RETURN | VK_SPACE => {
                    with(|p| p.picked = Some(p.pixel(p.cursor.x, p.cursor.y)));
                    let _ = DestroyWindow(hwnd);
                }
                VK_TAB => {
                    with(|p| {
                        p.format = color::next(p.format);
                        let _ = InvalidateRect(hwnd, Some(&p.last), false);
                    });
                }
                VK_LEFT => step(-n, 0),
                VK_RIGHT => step(n, 0),
                VK_UP => step(0, -n),
                VK_DOWN => step(0, n),
                _ => {}
            }
            LRESULT(0)
        }
        WM_ACTIVATE if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE => {
            // Başka bir pencere öne geçti (ör. Alt+Tab): seçici kapanır.
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

pub fn run() -> Result<()> {
    let cfg = config::load();
    unsafe {
        let (vx, vy) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
        let (w, h) = (GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1), GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1));
        // Ekranı dondur: tüm monitörlerin anlık kopyası.
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(screen);
        let bi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, ..Default::default() }, ..Default::default() };
        let mut bits = std::ptr::null_mut();
        let bmp = CreateDIBSection(screen, &bi, DIB_RGB_COLORS, &mut bits, None, 0)?;
        SelectObject(mem, bmp);
        BitBlt(mem, 0, 0, w, h, screen, vx, vy, SRCCOPY | CAPTUREBLT)?;
        ReleaseDC(None, screen);

        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let s = Picker::scale_at(cursor);
        let font = CreateFontW((-15.0 * s) as i32, 0, 0, 0, 600, 0, 0, 0, 0, 0, 0, CLEARTYPE_QUALITY.0 as u32, 0, w!("Segoe UI"));
        let picker = Picker { vx, vy, w, h, mem, bmp, bits: bits as *const u8, cursor, last: RECT::default(), format: cfg.tools.color_format, picked: None, font };
        let last = picker.lens_rect().0;
        P.with(|p| *p.borrow_mut() = Some(Picker { last, ..picker }));

        let inst: HINSTANCE = GetModuleHandleW(None)?.into();
        let class = w!("CarlUI.ColorPicker");
        let wc = WNDCLASSEXW { cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32, lpfnWndProc: Some(wnd_proc), hInstance: inst, lpszClassName: class, ..Default::default() };
        RegisterClassExW(&wc);
        let hwnd = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW, class, w!("CarlUI renk seçici"), WS_POPUP, vx, vy, w, h, None, None, inst, None)?;
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(hwnd);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let done = P.with(|p| p.borrow_mut().take());
        if let Some(p) = done {
            let _ = DeleteDC(p.mem);
            let _ = DeleteObject(p.bmp);
            let _ = DeleteObject(p.font);
            if let Some(rgb) = p.picked {
                let text = color::format(rgb, p.format);
                carlui_clip::write::set(HWND::default(), carlui_clip::write::Data::Text(&text));
                note::island(t!("Renk seçici", "Color picker"), &text, t!("Panoya kopyalandı", "Copied to clipboard"));
                carlui_core::info!("renk seçildi: {text}");
            }
        }
    }
    Ok(())
}
