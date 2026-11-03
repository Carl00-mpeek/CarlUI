//! Bu modül "tam ekran oyun ya da uygulama var mı?" sorusunu oyuna hiç
//! dokunmadan çözüyor: Windows'un kendi bildirim durumu sorgusu
//! (`SHQueryUserNotificationState`, özel tam ekran D3D'yi bildirir) ile öndeki
//! pencerenin monitörü çerçevesiz biçimde tamamen kaplayıp kaplamadığına bakar.
//! Hook, enjeksiyon veya render müdahalesi yok; anti-cheat ile uyumlu.

use carlui_core::win;
use windows::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    UI::{Shell::*, WindowsAndMessaging::*},
};

/// CarlUI.Shot: ekran görüntüsü düzenleyicisi tüm ekranı kaplar ama oyun değildir.
/// XamlExplorerHostIslandWindow (Alt+Tab / görev görünümü), Shell_SystemDim (UAC
/// karartması) ve ForegroundStaging (pencere geçişi) de monitörü kaplayan kabuk yüzeyleridir.
const SHELL_CLASSES: [&str; 9] = [
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "CarlUI.Host",
    "CarlUI.Shot",
    "XamlExplorerHostIslandWindow",
    "Shell_SystemDim",
    "ForegroundStaging",
];

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct State {
    pub active: bool,
    pub exclusive: bool,
    /// Tam ekran pencerenin monitörü (`win::monitor_name`); Windows görev çubuğu
    /// gibi yalnızca o monitördeki yüzeyler gizlenir.
    pub monitor: String,
}

pub fn query() -> State {
    // CarlUI'nin kendi tam ekran katmanı öndeyken (ekran görüntüsü seçimi)
    // Windows "meşgul" der; bu oyun değildir, dock uyutulmasın.
    // Seçim katmanı odak almaz; bu yüzden öndeki pencereye ek olarak katmanın
    // görünür olup olmadığına da bakılır.
    let shot = unsafe { FindWindowW(windows::core::w!("CarlUI.Shot"), None) }.is_ok_and(|h| unsafe { IsWindowVisible(h).as_bool() });
    if shot || win::class_name(unsafe { GetForegroundWindow() }).starts_with("CarlUI.") {
        return State::default();
    }
    let quns = unsafe { SHQueryUserNotificationState() }.unwrap_or(QUNS_ACCEPTS_NOTIFICATIONS);
    let exclusive = quns == QUNS_RUNNING_D3D_FULL_SCREEN;
    let busy = quns == QUNS_BUSY || quns == QUNS_PRESENTATION_MODE;
    let active = exclusive || busy || foreground_covers_monitor();
    let monitor = if active { win::monitor_name(unsafe { GetForegroundWindow() }) } else { String::new() };
    State { active, exclusive, monitor }
}

/// Masaüstü (Win+D sonrası) öne geldi mi?
pub fn desktop_is_foreground() -> bool {
    let fg = unsafe { GetForegroundWindow() };
    matches!(win::class_name(fg).as_str(), "Progman" | "WorkerW")
}

fn foreground_covers_monitor() -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_invalid() || !IsWindowVisible(fg).as_bool() {
            return false;
        }
        // CarlUI'nin kendi katmanları (ekran görüntüsü seçimi vb.) da oyun değildir.
        let class = win::class_name(fg);
        // Windows'un Başlat/Arama gibi kabuk yüzeyleri (CoreWindow) monitörü kaplar ama tam ekran değildir.
        if SHELL_CLASSES.contains(&class.as_str()) || class.starts_with("CarlUI.") || class == "Windows.UI.Core.CoreWindow" {
            return false;
        }
        // Büyütülmüş pencere kenarlarından monitöre taşar (-8 px); tam ekran değildir.
        // Firefox ise video/F11 tam ekranında pencereyi "büyütülmüş" bırakıp monitöre
        // birebir oturtur: büyütülmüşse yalnızca tam örtüşme tam ekran sayılır.
        let zoomed = IsZoomed(fg).as_bool();
        // Başlıklı maksimize pencereler tam ekran değildir (kenarları taşsa bile).
        let style = GetWindowLongW(fg, GWL_STYLE) as u32;
        if style & WS_CAPTION.0 == WS_CAPTION.0 {
            return false;
        }
        let mut r = RECT::default();
        if GetWindowRect(fg, &mut r).is_err() {
            return false;
        }
        let mon = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(mon, &mut mi).as_bool() {
            return false;
        }
        let m = mi.rcMonitor;
        if zoomed {
            return r == m;
        }
        r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
    }
}
