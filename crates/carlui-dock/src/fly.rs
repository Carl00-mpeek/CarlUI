//! Bu modül "pencere küçülürken dock'a gitsin, açılırken dock'tan gelsin"
//! işini çözüyor. Windows 11'de küçültme animasyonunun hedefini yalnızca
//! Windows görev çubuğu belirleyebiliyor: kabuk kancasına gelen
//! `HSHELL_GETMINRECT` sorusu animasyon başladıktan sonra geliyor ve yanıtı
//! kullanılmıyor. Görev çubuğu gizliyken pencere olduğu yerde söner.
//!
//! Bu yüzden pencerenin canlı DWM görüntüsü saydam, tıklanamayan bir katmanda
//! pencere ile dock simgesi arasında uçurulur. Her uçuş kendi iş parçacığında
//! çalışır; dock'un çizimini bekletmez.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, Once,
    },
    time::Instant,
};
use windows::{
    core::w,
    Win32::{
        Foundation::*,
        Graphics::{Dwm::*, Gdi::*},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::*,
    },
};

/// Küçülme ve açılma süreleri (ms). Windows'un kendi animasyonuna yakın;
/// animasyon hızı ayarıyla ölçeklenir (`motion_ms`).
const MIN_MS: f32 = 250.0;
const RESTORE_MS: f32 = 230.0;
/// Simgeye vardığında görüntünün saydamlığı (0-255).
const END_ALPHA: f32 = 110.0;
/// Aynı anda en çok kaç uçuş.
const MAX_FLIGHTS: usize = 6;

static SEQ: AtomicU64 = AtomicU64::new(0);
/// Pencere başına sürmekte olan uçuşun numarası: yenisi gelince eskisi biter.
static LIVE: Mutex<Option<HashMap<isize, u64>>> = Mutex::new(None);

/// Kullanıcı "küçültürken ve büyütürken pencereleri canlandır" ayarını
/// kapattıysa ya da CarlUI'de "hareketi azalt" açıksa uçuş da yapılmaz.
pub fn enabled() -> bool {
    if carlui_core::theme::reduced_motion() {
        return false;
    }
    let mut ai = ANIMATIONINFO { cbSize: std::mem::size_of::<ANIMATIONINFO>() as u32, iMinAnimate: 0 };
    unsafe { SystemParametersInfoW(SPI_GETANIMATION, ai.cbSize, Some(&mut ai as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)).is_ok() && ai.iMinAnimate != 0 }
}

/// Pencere ile dock simgesi arasında uçuşu başlatır. `minimizing` doğruysa
/// pencereden simgeye, değilse simgeden pencereye.
pub fn start(src: HWND, icon: RECT, minimizing: bool) {
    let Some(win) = (unsafe { window_rect(src, minimizing) }) else { return };
    let seq = SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    {
        let Ok(mut m) = LIVE.lock() else { return };
        let map = m.get_or_insert_with(HashMap::new);
        // Hepsini küçült (Win+M) onlarca pencereyi aynı anda indirir: birkaçı
        // uçsun, gerisi Windows'un kendi animasyonuyla kalsın.
        if map.len() >= MAX_FLIGHTS && !map.contains_key(&(src.0 as isize)) {
            return;
        }
        map.insert(src.0 as isize, seq);
    }
    let raw = src.0 as isize;
    std::thread::spawn(move || {
        let src = HWND(raw as _);
        if let Err(e) = unsafe { run(src, win, icon, minimizing, seq) } {
            carlui_core::warn!("dock uçuş animasyonu: {e}");
        }
        if let Ok(mut m) = LIVE.lock() {
            if let Some(map) = m.as_mut() {
                if map.get(&raw) == Some(&seq) {
                    map.remove(&raw);
                }
            }
        }
    });
}

fn current(src: HWND, seq: u64) -> bool {
    LIVE.lock().ok().and_then(|m| m.as_ref().and_then(|m| m.get(&(src.0 as isize)).copied())) == Some(seq)
}

/// Pencerenin küçük değilken kapladığı yer (görünmez kenarlar dahil: DWM
/// görüntüsü de bu dikdörtgeni kaplar).
unsafe fn window_rect(hwnd: HWND, minimizing: bool) -> Option<RECT> {
    if !minimizing {
        // Açılış olayı geldiğinde pencere yerine oturmuş olur.
        if IsIconic(hwnd).as_bool() {
            return None;
        }
        let mut r = RECT::default();
        GetWindowRect(hwnd, &mut r).ok()?;
        return Some(r);
    }
    if !IsIconic(hwnd).as_bool() {
        return None;
    }
    let mut wp = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
    GetWindowPlacement(hwnd, &mut wp).ok()?;
    let normal = wp.rcNormalPosition;
    let mon = MonitorFromRect(&normal, MONITOR_DEFAULTTONEAREST);
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    if !GetMonitorInfoW(mon, &mut mi).as_bool() {
        return None;
    }
    if wp.flags.0 & WPF_RESTORETOMAXIMIZED.0 != 0 {
        // Büyütülmüş pencere çalışma alanını kaplar, kenarları dışarı taşar.
        let mut r = mi.rcWork;
        let pad = GetSystemMetrics(SM_CXSIZEFRAME) + GetSystemMetrics(SM_CXPADDEDBORDER);
        r.left -= pad;
        r.top -= pad;
        r.right += pad;
        r.bottom += pad;
        return Some(r);
    }
    // Normal konum "çalışma alanı" koordinatında; araç pencerelerinde ekran koordinatında.
    let tool = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0;
    let (dx, dy) = if tool { (0, 0) } else { (mi.rcWork.left - mi.rcMonitor.left, mi.rcWork.top - mi.rcMonitor.top) };
    Some(RECT { left: normal.left + dx, top: normal.top + dy, right: normal.right + dx, bottom: normal.bottom + dy })
}

/// Pencere oranı korunarak simge kutusuna sığdırılmış dikdörtgen.
fn fit(icon: RECT, w: f32, h: f32) -> [f32; 4] {
    let (iw, ih) = ((icon.right - icon.left) as f32, (icon.bottom - icon.top) as f32);
    let k = (iw / w.max(1.0)).min(ih / h.max(1.0));
    let (cx, cy) = ((icon.left + icon.right) as f32 / 2.0, (icon.top + icon.bottom) as f32 / 2.0);
    [cx - w * k / 2.0, cy - h * k / 2.0, cx + w * k / 2.0, cy + h * k / 2.0]
}

fn lerp(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// Küçülürken: yavaş başlayıp simgeye hızlanarak girer. Açılırken: simgeden
/// hızlı çıkıp yerine yumuşak oturur.
pub fn ease(t: f32, minimizing: bool) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if minimizing {
        t * t * (2.2 - 1.2 * t)
    } else {
        1.0 - (1.0 - t).powi(3)
    }
}

extern "system" fn proc_(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

unsafe fn overlay(bounds: RECT) -> windows::core::Result<HWND> {
    static CLASS: Once = Once::new();
    let instance = GetModuleHandleW(None)?;
    CLASS.call_once(|| {
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(proc_),
            hInstance: instance.into(),
            lpszClassName: w!("CarlUI.DockFly"),
            ..Default::default()
        });
    });
    let hwnd = CreateWindowExW(
        WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_NOREDIRECTIONBITMAP,
        w!("CarlUI.DockFly"),
        w!(""),
        WS_POPUP,
        bounds.left,
        bounds.top,
        bounds.right - bounds.left,
        bounds.bottom - bounds.top,
        None,
        None,
        instance,
        None,
    )?;
    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
    let on = BOOL(1);
    let _ = DwmSetWindowAttribute(hwnd, DWMWA_EXCLUDED_FROM_PEEK, &on as *const _ as *const _, 4);
    // Katmanın kendisi belirip kaybolma animasyonu oynatmasın.
    let _ = DwmSetWindowAttribute(hwnd, DWMWA_TRANSITIONS_FORCEDISABLED, &on as *const _ as *const _, 4);
    Ok(hwnd)
}

unsafe fn run(src: HWND, win: RECT, icon: RECT, minimizing: bool, seq: u64) -> windows::core::Result<()> {
    let bounds = RECT {
        left: win.left.min(icon.left),
        top: win.top.min(icon.top),
        right: win.right.max(icon.right),
        bottom: win.bottom.max(icon.bottom),
    };
    let hwnd = overlay(bounds)?;
    let result = (|| {
        let thumb = DwmRegisterThumbnail(hwnd, src)?;
        let (ww, wh) = ((win.right - win.left) as f32, (win.bottom - win.top) as f32);
        let full = [win.left as f32, win.top as f32, win.right as f32, win.bottom as f32];
        let small = fit(icon, ww, wh);
        let (from, to) = if minimizing { (full, small) } else { (small, full) };
        let ms = carlui_core::theme::motion_ms(if minimizing { MIN_MS } else { RESTORE_MS });
        let (ox, oy) = (bounds.left as f32, bounds.top as f32);
        let set = |t: f32| {
            let e = ease(t, minimizing);
            let r = lerp(from, to, e);
            let a = if minimizing { 255.0 + (END_ALPHA - 255.0) * e } else { END_ALPHA + (255.0 - END_ALPHA) * e };
            let props = DWM_THUMBNAIL_PROPERTIES {
                dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_VISIBLE | DWM_TNP_OPACITY | DWM_TNP_SOURCECLIENTAREAONLY,
                rcDestination: RECT {
                    left: (r[0] - ox).round() as i32,
                    top: (r[1] - oy).round() as i32,
                    right: (r[2] - ox).round() as i32,
                    bottom: (r[3] - oy).round() as i32,
                },
                opacity: a.round().clamp(0.0, 255.0) as u8,
                fVisible: TRUE,
                fSourceClientAreaOnly: FALSE,
                ..Default::default()
            };
            let _ = DwmUpdateThumbnailProperties(thumb, &props);
        };
        set(0.0);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let t0 = Instant::now();
        loop {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let t = t0.elapsed().as_secs_f32() * 1000.0 / ms;
            if t >= 1.0 || !current(src, seq) || !IsWindow(src).as_bool() {
                break;
            }
            set(t);
            let _ = DwmFlush();
        }
        let _ = DwmUnregisterThumbnail(thumb);
        Ok(())
    })();
    let _ = DestroyWindow(hwnd);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oran_korunur() {
        let icon = RECT { left: 100, top: 1000, right: 160, bottom: 1060 };
        let r = fit(icon, 1600.0, 900.0);
        assert!((r[2] - r[0] - 60.0).abs() < 0.01, "geniş pencere simgenin genişliğine sığar");
        assert!(((r[3] - r[1]) - 33.75).abs() < 0.01);
        assert!(((r[1] + r[3]) / 2.0 - 1030.0).abs() < 0.01, "simgenin ortasında");
    }

    #[test]
    fn yumusama_uclari() {
        for m in [true, false] {
            assert!(ease(0.0, m).abs() < 1e-6);
            assert!((ease(1.0, m) - 1.0).abs() < 1e-6);
            assert!(ease(0.5, m) > 0.0 && ease(0.5, m) < 1.0);
        }
        // Küçülürken başta yavaş, açılırken başta hızlı.
        assert!(ease(0.2, true) < 0.2);
        assert!(ease(0.2, false) > 0.2);
    }

    /// Canlı: 2. ekranda bir test penceresi küçültülüp açılır, uçuş kareleri
    /// sayılır. `cargo test -p carlui-dock uc_kucul -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn uc_kucul_ac() {
        unsafe {
            let w = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("CarlUI uçuş testi"),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                2300,
                200,
                700,
                500,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let pump = |ms: u64| {
                let t = Instant::now();
                while t.elapsed().as_millis() < ms as u128 {
                    let mut msg = MSG::default();
                    while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                        DispatchMessageW(&msg);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            };
            pump(600);
            let icon = RECT { left: 2100, top: 980, right: 2160, bottom: 1040 };
            let _ = ShowWindow(w, SW_MINIMIZE);
            let r = window_rect(w, true).expect("küçük pencerenin yeri");
            println!("küçülmeden önceki yer: {r:?}");
            assert!(r.left >= 2280 && r.left <= 2320 && r.right - r.left >= 690, "{r:?}");
            start(w, icon, true);
            pump(500);
            let _ = ShowWindow(w, SW_RESTORE);
            start(w, icon, false);
            pump(500);
            let _ = DestroyWindow(w);
        }
    }
}
