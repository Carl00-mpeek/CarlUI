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
    // Kum havuzu ayrı sınıf adı kullanır (gerçek dock'un tüneğiyle karışmasın).
    let class = if crate::sandbox::on() { w!("CarlUI.DockSandboxPet") } else { w!("CarlUI.DockPet") };
    overlay_window(class, &CLASS)
}

/// Tıklamayı geçiren, odak almayan, saydam katman penceresi (gizli başlar).
/// Sınıf ilk çağrıda bir kez kaydedilir.
pub(super) unsafe fn overlay_window(class: PCWSTR, once: &'static Once) -> Result<HWND> {
    layer_window(class, once, WS_EX_TOPMOST)
}

/// `overlay_window` gibi ama en üstte değil: sıradan pencerelerin arkasında kalabilir.
unsafe fn layer_window(class: PCWSTR, once: &'static Once, extra: WINDOW_EX_STYLE) -> Result<HWND> {
    let instance = GetModuleHandleW(None)?;
    once.call_once(|| {
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(perch_proc), hInstance: instance.into(), lpszClassName: class, ..Default::default() });
    });
    let hwnd = CreateWindowExW(
        WS_EX_TOOLWINDOW | extra | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_NOREDIRECTIONBITMAP,
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

    /// Pencerenin ekrandaki sol üst köşesi.
    pub fn origin(&self) -> (i32, i32) {
        (self.left, self.edge - self.rise)
    }

    /// Yerleştirildiği pill üst kenarı.
    pub fn edge(&self) -> i32 {
        self.edge
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

/// Masaüstünün hemen üstünde, sıradan pencerelerin arkasında duran,
/// monitör boyutunda tıklamayı geçiren katman (yağan kar burada çizilir:
/// pencerelerin önüne değil, masaüstüne yağar).
pub struct BackdropWindow {
    hwnd: HWND,
    _target: DesktopWindowTarget,
    pub root: ContainerVisual,
    shown: bool,
}

impl BackdropWindow {
    pub fn new(gfx: &Gfx, mon: RECT) -> Result<Self> {
        static CLASS: Once = Once::new();
        let class = if crate::sandbox::on() { w!("CarlUI.DockSandboxSnowBack") } else { w!("CarlUI.DockSnowBack") };
        let hwnd = unsafe { layer_window(class, &CLASS, WINDOW_EX_STYLE(0))? };
        unsafe {
            let _ = SetWindowPos(hwnd, None, mon.left, mon.top, (mon.right - mon.left).max(1), (mon.bottom - mon.top).max(1), SWP_NOACTIVATE | SWP_NOZORDER);
        }
        let made = (|| -> Result<_> {
            let target = unsafe { gfx.comp.cast::<ICompositorDesktopInterop>()?.CreateDesktopWindowTarget(hwnd, false)? };
            let root = gfx.comp.CreateContainerVisual()?;
            target.SetRoot(&root)?;
            Ok((target, root))
        })();
        match made {
            Ok((target, root)) => {
                let mut me = Self { hwnd, _target: target, root, shown: false };
                me.keep_z();
                Ok(me)
            }
            Err(e) => {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                Err(e)
            }
        }
    }

    pub fn show(&mut self, on: bool) {
        if on != self.shown {
            self.shown = on;
            unsafe {
                let _ = ShowWindow(self.hwnd, if on { SW_SHOWNOACTIVATE } else { SW_HIDE });
            }
            if on {
                self.keep_z();
            }
        }
    }

    /// Masaüstü penceresinin (simgeleri taşıyan Progman/WorkerW) hemen üstüne
    /// yerleşir. Zaten oradaysa dokunmaz.
    pub fn keep_z(&mut self) {
        unsafe {
            let Some(desk) = desktop_host() else { return };
            let prev = GetWindow(desk, GW_HWNDPREV).unwrap_or_default();
            if prev == self.hwnd {
                return;
            }
            // Önünde sıradan pencere yoksa (hepsi simge durumunda) en üstteki
            // sıradan pencere konumu: en üstte kalıcı (topmost) olmadan.
            let topmost = !prev.is_invalid() && (GetWindowLongW(prev, GWL_EXSTYLE) as u32) & WS_EX_TOPMOST.0 != 0;
            let after = if prev.is_invalid() || topmost { HWND_NOTOPMOST } else { prev };
            let _ = SetWindowPos(self.hwnd, after, 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOOWNERZORDER);
        }
    }
}

impl Drop for BackdropWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// Masaüstü simgelerini taşıyan üst düzey pencere (Progman ya da bir WorkerW).
pub(super) fn desktop_host() -> Option<HWND> {
    unsafe extern "system" fn cb(h: HWND, data: LPARAM) -> BOOL {
        if FindWindowExW(h, None, w!("SHELLDLL_DefView"), None).is_ok_and(|v| !v.is_invalid()) {
            *(data.0 as *mut HWND) = h;
            return FALSE;
        }
        TRUE
    }
    let mut found = HWND::default();
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut found as *mut _ as isize));
    }
    (!found.is_invalid()).then_some(found)
}
