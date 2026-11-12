//! Hayvanların tüneği: dock'un hemen üstünde, tıklamayı geçiren saydam bir
//! katman penceresi. Dock penceresinin bölgesi (SetWindowRgn) pill'le sınırlı
//! olduğu için hayvan dock'un dışına çizilemez; ayrı pencere hem çizimi hem
//! isabet testini dock'tan tamamen ayırır.
//!
//! Pencere monitör genişliğindedir ve alt kenarı pill'in üst kenarına
//! (ya da şerit hayvanın taban çizgisine) oturur; hayvanlar yalnızca görselin
//! konumu değiştirilerek yürür (pencere taşınmaz). Pencerenin altında kalan
//! kısım kırpılır; camın içinde görünmesi gereken parçalar (yılanın ön
//! sarmalı, yunusun suyun altındaki gövdesi) camın içerik katmanına ayrıca
//! eklenir.

use crate::dock::LiveView;
use carlui_ui::gfx::Gfx;
use std::sync::Once;
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::Dwm::*,
        System::{LibraryLoader::GetModuleHandleW, WinRT::Composition::ICompositorDesktopInterop},
        UI::WindowsAndMessaging::*,
    },
    UI::Composition::{Desktop::DesktopWindowTarget, *},
};

extern "system" fn perch_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

unsafe fn create_window() -> Result<HWND> {
    static CLASS: Once = Once::new();
    let instance = GetModuleHandleW(None)?;
    // Kum havuzu ayrı sınıf adı kullanır (gerçek dock'un tüneğiyle karışmasın).
    let class = if crate::sandbox::on() { w!("CarlUI.DockSandboxPet") } else { w!("CarlUI.DockPet") };
    CLASS.call_once(|| {
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(perch_proc), hInstance: instance.into(), lpszClassName: class, ..Default::default() });
    });
    let hwnd = CreateWindowExW(
        WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_NOREDIRECTIONBITMAP,
        class,
        w!(""),
        WS_POPUP,
        0,
        0,
        1,
        1,
        None,
        None,
        instance,
        None,
    )?;
    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
    let on = BOOL(1);
    let _ = DwmSetWindowAttribute(hwnd, DWMWA_EXCLUDED_FROM_PEEK, &on as *const _ as *const _, 4);
    let _ = DwmSetWindowAttribute(hwnd, DWMWA_TRANSITIONS_FORCEDISABLED, &on as *const _ as *const _, 4);
    Ok(hwnd)
}

pub struct PerchWindow {
    hwnd: HWND,
    _target: DesktopWindowTarget,
    pub root: ContainerVisual,
    /// Pencerenin ekrandaki sol kenarı.
    pub left: i32,
    /// Pencerenin pill'in üst kenarından ne kadar yukarıda başladığı.
    rise: i32,
    /// Son yerleştirildiği pill üst kenarı.
    edge: i32,
    shown: bool,
}

impl PerchWindow {
    /// Pill'in üst kenarından `rise` piksel yukarıda başlayan, `height` piksel
    /// yüksekliğinde, monitör genişliğinde tünek.
    pub fn new(gfx: &Gfx, view: &LiveView, rise: i32, height: i32) -> Result<Self> {
        let hwnd = unsafe { create_window()? };
        let mon = view.mon;
        unsafe {
            // Dock'un hemen altına: büyüyen ikonlar ve önizlemeler hayvanın üstünde kalır.
            let _ = SetWindowPos(hwnd, view.hwnd, mon.left, view.pill.top - rise, mon.right - mon.left, height.max(1), SWP_NOACTIVATE);
        }
        let target = unsafe { gfx.comp.cast::<ICompositorDesktopInterop>()?.CreateDesktopWindowTarget(hwnd, true)? };
        let root = gfx.comp.CreateContainerVisual()?;
        target.SetRoot(&root)?;
        Ok(Self { hwnd, _target: target, root, left: mon.left, rise, edge: view.pill.top, shown: false })
    }

    /// Dock'un yüksekliği ya da yeri değiştiyse (pill'in üst kenarı kaydıysa)
    /// tüneği yeni kenara taşır. Değişmediyse tek bir karşılaştırma.
    pub fn follow(&mut self, pill: RECT) {
        if pill.top == self.edge {
            return;
        }
        self.edge = pill.top;
        unsafe {
            let _ = SetWindowPos(self.hwnd, None, self.left, pill.top - self.rise, 0, 0, SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOSIZE);
        }
    }

    pub fn show(&mut self, on: bool) {
        if on != self.shown {
            self.shown = on;
            unsafe {
                let _ = ShowWindow(self.hwnd, if on { SW_SHOWNOACTIVATE } else { SW_HIDE });
            }
        }
    }

    /// Ekran x'inin pencere içi karşılığı.
    pub fn local_x(&self, screen_x: i32) -> f32 {
        (screen_x - self.left) as f32
    }
}

impl Drop for PerchWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
