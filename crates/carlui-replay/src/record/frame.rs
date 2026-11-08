//! Alanın çevresindeki ince kırmızı çerçeve ve durdurma kısayolu. Çerçeve alanın
//! dışında durur, tıklamaları geçirir, odak almaz ve kayda girmez. Kısayol bu
//! iş parçacığının mesaj kuyruğuna gelir.

use super::{Area, Shared};
use carlui_core::keys::Combo;
use std::sync::{atomic::Ordering, Arc};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::*,
    },
};

const EDGE: i32 = 2;
/// BGR sırasıyla kırmızı.
const RED: u32 = 0x0030_3BFF;
const HOTKEY: i32 = 1;

unsafe extern "system" fn proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    DefWindowProcW(h, m, w, l)
}

unsafe fn window(a: Area) -> Result<HWND> {
    let inst: HINSTANCE = GetModuleHandleW(None)?.into();
    let class = w!("CarlUI.RecordFrame");
    let wc = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: inst, lpszClassName: class, hbrBackground: CreateSolidBrush(COLORREF(RED)), ..Default::default() };
    RegisterClassW(&wc);
    let (w, h) = (a.w as i32 + EDGE * 2, a.h as i32 + EDGE * 2);
    let ex = WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_LAYERED;
    let hwnd = CreateWindowExW(ex, class, w!(""), WS_POPUP, a.x - EDGE, a.y - EDGE, w, h, None, None, inst, None)?;
    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
    let outer = CreateRectRgn(0, 0, w, h);
    let inner = CreateRectRgn(EDGE, EDGE, w - EDGE, h - EDGE);
    CombineRgn(outer, outer, inner, RGN_DIFF);
    let _ = DeleteObject(inner);
    // Bölgenin sahibi artık pencere.
    SetWindowRgn(hwnd, outer, true);
    let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    Ok(hwnd)
}

unsafe fn listen(c: Option<Combo>) {
    let Some(c) = c else { return };
    if let Err(e) = carlui_core::hotkey::register(HWND::default(), HOTKEY, c) {
        warn!("alan kaydı kısayolu kaydedilemedi: {e}");
    }
}

pub fn run(area: Area, combo: Option<Combo>, shared: Arc<Shared>) {
    unsafe {
        let hwnd = match window(area) {
            Ok(h) => Some(h),
            Err(e) => {
                warn!("kayıt çerçevesi açılamadı: {e}");
                None
            }
        };
        listen(combo);
        // Kuyruk boşken de durma bayrağı görülsün.
        let timer = SetTimer(None, 0, 200, None);
        let mut msg = MSG::default();
        while !shared.stop.load(Ordering::SeqCst) && GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_HOTKEY {
                shared.stop();
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = KillTimer(None, timer);
        carlui_core::hotkey::unregister(HWND::default(), HOTKEY);
        if let Some(h) = hwnd {
            let _ = DestroyWindow(h);
        }
    }
}
