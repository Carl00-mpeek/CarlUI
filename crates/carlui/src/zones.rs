//! Pencere yakalama bölgeleri: bir pencere başlığından sürüklenirken Shift
//! basılı tutulursa imlecin olduğu ekranda bölgeler belirir, imlecin altındaki
//! vurgulanır; bırakınca pencere o bölgeye oturur. Shift ile birlikte Ctrl de
//! basılırsa Ctrl'e basıldığı andaki bölgeden imlecin bölgesine kadar olan
//! alan birleşir (2×2 ızgarada yarım ekran, üç sütunda iki sütun…).
//!
//! Neden ayrı iş parçacığı: ana süreç DPI farkında değil (Windows koordinatları
//! ölçekler), bölgelerin ise fiziksel pikselde, her monitörün kendi ölçeğinde
//! hesaplanması gerekir. Bu iş parçacığı "monitör başına DPI farkında" çalışır;
//! sürükleme kancası, katman penceresi ve yerleştirme hep burada.
//!
//! Boştayken maliyet: yalnızca "sürükleme başladı/bitti" olay kancası.
//! Zamanlayıcı sürükleme sürerken çalışır; katman yalnızca seçim değişince
//! yeniden çizilir. Tam ekranda kanca sökülür.

use carlui_core::config::Config;
use carlui_core::zones::{self, Rect, ZoneLayout};
use carlui_core::{info, warn};
use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering::*};
use std::sync::{Mutex, Once};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::{Dwm::*, Gdi::*},
        System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
        UI::{Accessibility::*, HiDpi::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};

/// Ayarlar değişti: `WANT`'ı yeniden oku.
const WM_SYNC: u32 = WM_APP + 1;
/// Sürükleme sürerken tuşları ve imleci yoklayan zamanlayıcı.
const TIMER_POLL: usize = 1;
/// Sürükleme bittikten hemen sonra yerleştirme (Windows kendi yerleştirmesini bitirsin).
const TIMER_PLACE: usize = 2;
const CLASS: PCWSTR = w!("CarlUI.Zones");

#[derive(Clone, Debug, PartialEq)]
struct Settings {
    on: bool,
    layout: ZoneLayout,
    custom: Vec<[f32; 4]>,
    gap: u32,
    accent: (u8, u8, u8),
}

static WANT: Mutex<Option<Settings>> = Mutex::new(None);
static THREAD: AtomicU32 = AtomicU32::new(0);
static START: Once = Once::new();

/// Ayara ve tam ekran durumuna göre bölgeleri açar/kapatır. Ucuzdur; her
/// tam ekran kontrolünde çağrılabilir (değişiklik yoksa hiçbir şey yapmaz).
pub fn sync(cfg: &Config, fullscreen: bool) {
    let a = cfg.theme.accent_pair().0;
    let s = Settings {
        on: cfg.tools.snap_zones && !fullscreen,
        layout: cfg.tools.zone_layout,
        custom: cfg.tools.zones_custom.clone(),
        gap: cfg.tools.zone_gap.min(64),
        accent: (a.0, a.1, a.2),
    };
    {
        let Ok(mut w) = WANT.lock() else { return };
        if w.as_ref() == Some(&s) {
            return;
        }
        let on = s.on;
        *w = Some(s);
        // Hiç açılmadıysa ve açılması da istenmiyorsa iş parçacığı kurulmaz.
        if !on && THREAD.load(Acquire) == 0 && !START.is_completed() {
            return;
        }
    }
    START.call_once(|| {
        if let Err(e) = std::thread::Builder::new().name("zones".into()).spawn(run) {
            warn!("bölgeler başlatılamadı: {e}");
        }
    });
    let t = THREAD.load(Acquire);
    if t != 0 {
        let _ = unsafe { PostThreadMessageW(t, WM_SYNC, WPARAM(0), LPARAM(0)) };
    }
}

fn pressed(vk: VIRTUAL_KEY) -> bool {
    (unsafe { GetAsyncKeyState(vk.0 as i32) }) < 0
}

fn cursor() -> POINT {
    let mut p = POINT::default();
    let _ = unsafe { GetCursorPos(&mut p) };
    p
}

fn rect_of(r: RECT) -> Rect {
    [r.left, r.top, r.right, r.bottom]
}

/// Pencerenin görünen kenarları (gölge payı hariç).
fn frame(h: HWND) -> Option<RECT> {
    let mut r = RECT::default();
    unsafe {
        if DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut _ as *mut _, std::mem::size_of::<RECT>() as u32).is_ok() && r.right > r.left {
            return Some(r);
        }
        GetWindowRect(h, &mut r).ok()?;
    }
    Some(r)
}

/// Monitörün çalışma alanı ve ölçeği (96 DPI'da 1).
fn monitor_at(p: POINT) -> (HMONITOR, Rect, f32) {
    unsafe {
        let m = MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(m, &mut mi);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        (m, rect_of(mi.rcWork), dx.max(96) as f32 / 96.0)
    }
}

/// Sürüklenen pencere bölgelere uygun mu? Üst düzey, görünür, büyütülebilir
/// ya da boyutlandırılabilir sıradan bir uygulama penceresi olmalı.
fn movable(h: HWND) -> bool {
    unsafe {
        if h.0.is_null() || GetAncestor(h, GA_ROOT) != h || !IsWindowVisible(h).as_bool() {
            return false;
        }
        let style = GetWindowLongW(h, GWL_STYLE) as u32;
        let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
        if style & WS_CHILD.0 != 0 || ex & WS_EX_TOOLWINDOW.0 != 0 {
            return false;
        }
        style & WS_THICKFRAME.0 != 0 || style & WS_MAXIMIZEBOX.0 != 0
    }
}

/// Sürükleme bir kenardan/köşeden mi başladı (yani boyutlandırma mı)?
/// Büyütülmüş pencere boyutlandırılamaz; onda her sürükleme taşımadır.
fn resizing(h: HWND, p: POINT, scale: f32) -> bool {
    if unsafe { IsZoomed(h) }.as_bool() {
        return false;
    }
    let Some(f) = frame(h) else { return false };
    let e = (6.0 * scale).round() as i32;
    !(p.x > f.left + e && p.x < f.right - e && p.y > f.top + e && p.y < f.bottom - e)
}

/// Pencereyi görünen kenarları `r` olacak şekilde koyar.
fn place(h: HWND, r: Rect) {
    unsafe {
        if !IsWindow(h).as_bool() {
            return;
        }
        if IsZoomed(h).as_bool() || IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        // Ekranlar arası geçişte DPI değişip boyut yeniden hesaplanır; ikinci tur oturtur.
        for _ in 0..2 {
            let mut all = RECT::default();
            if GetWindowRect(h, &mut all).is_err() {
                return;
            }
            let seen = frame(h).unwrap_or(all);
            let (l, t) = (seen.left - all.left, seen.top - all.top);
            let (rt, b) = (all.right - seen.right, all.bottom - seen.bottom);
            let _ = SetWindowPos(h, None, r[0] - l, r[1] - t, r[2] - r[0] + l + rt, r[3] - r[1] + t + b, SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER);
            if frame(h).is_some_and(|f| rect_of(f) == r) {
                break;
            }
        }
    }
}

/// Bölgeleri çizen katmanın piksel tamponu (GDI DIB, önceden çarpılmış BGRA).
struct Canvas {
    dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u32,
    w: i32,
    h: i32,
}

impl Canvas {
    fn new(w: i32, h: i32) -> Option<Canvas> {
        unsafe {
            let dc = CreateCompatibleDC(None);
            if dc.is_invalid() {
                return None;
            }
            let bi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let Ok(bmp) = CreateDIBSection(dc, &bi, DIB_RGB_COLORS, &mut bits, None, 0) else {
                let _ = DeleteDC(dc);
                return None;
            };
            let old = SelectObject(dc, bmp);
            Some(Canvas { dc, bmp, old, bits: bits as *mut u32, w, h })
        }
    }

    fn px(&mut self) -> &mut [u32] {
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.w * self.h) as usize) }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bmp);
            let _ = DeleteDC(self.dc);
        }
    }
}

/// Önceden çarpılmış renk (0..1).
#[derive(Clone, Copy)]
struct Paint([f32; 4]);

impl Paint {
    fn rgb(r: u8, g: u8, b: u8, a: f32) -> Paint {
        Paint([r as f32 / 255.0 * a, g as f32 / 255.0 * a, b as f32 / 255.0 * a, a])
    }
}

fn pack(c: [f32; 4]) -> u32 {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    (q(c[3]) << 24) | (q(c[0]) << 16) | (q(c[1]) << 8) | q(c[2])
}

fn unpack(p: u32) -> [f32; 4] {
    let f = |s: u32| ((p >> s) & 0xFF) as f32 / 255.0;
    [f(16), f(8), f(0), f(24)]
}

/// Yuvarlak köşeli dikdörtgen: dolgu + kenar çizgisi, kenar yumuşatmalı.
fn round_rect(px: &mut [u32], w: i32, h: i32, r: Rect, rad: f32, line: f32, fill: Paint, edge: Paint) {
    let (x0, y0) = (r[0].max(0), r[1].max(0));
    let (x1, y1) = (r[2].min(w), r[3].min(h));
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let (cx, cy) = ((r[0] + r[2]) as f32 / 2.0, (r[1] + r[3]) as f32 / 2.0);
    let (bx, by) = ((r[2] - r[0]) as f32 / 2.0, (r[3] - r[1]) as f32 / 2.0);
    let rad = rad.min(bx).min(by);
    let cov = |d: f32| (0.5 - d).clamp(0.0, 1.0);
    for y in y0..y1 {
        let qy = (y as f32 + 0.5 - cy).abs() - by + rad;
        let row = (y * w) as usize;
        for x in x0..x1 {
            let qx = (x as f32 + 0.5 - cx).abs() - bx + rad;
            let d = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - rad;
            let outer = cov(d);
            if outer <= 0.0 {
                continue;
            }
            let inner = cov(d + line);
            let band = outer - inner;
            let mut s = [0.0f32; 4];
            for (i, v) in s.iter_mut().enumerate() {
                *v = fill.0[i] * inner + edge.0[i] * band;
            }
            let i = row + x as usize;
            let dst = unpack(px[i]);
            let k = 1.0 - s[3];
            px[i] = pack([s[0] + dst[0] * k, s[1] + dst[1] * k, s[2] + dst[2] * k, s[3] + dst[3] * k]);
        }
    }
}

/// Katmanı çizer: hafif karartılmış alan, soluk bölgeler, vurgulu seçim.
/// Dikdörtgenler katmanın kendi koordinatında, boşluğu düşülmüş haldedir.
fn paint(px: &mut [u32], w: i32, h: i32, zones: &[Rect], pick: Option<Rect>, accent: (u8, u8, u8), scale: f32) {
    px.fill(pack([0.0, 0.0, 0.0, 0.18]));
    let rad = 10.0 * scale;
    let line = (1.5 * scale).max(1.0);
    for z in zones {
        round_rect(px, w, h, *z, rad, line, Paint::rgb(255, 255, 255, 0.08), Paint::rgb(255, 255, 255, 0.55));
    }
    if let Some(p) = pick {
        let (r, g, b) = accent;
        round_rect(px, w, h, p, rad, line * 2.0, Paint::rgb(r, g, b, 0.38), Paint::rgb(r, g, b, 1.0));
    }
}

/// Süren sürükleme.
struct Drag {
    hwnd: HWND,
    /// Bölgelerin hesaplandığı monitör (imleç başka ekrana geçince yeniden hesaplanır).
    mon: Option<isize>,
    work: Rect,
    scale: f32,
    /// Boşluksuz bölgeler (ekran koordinatı).
    zones: Vec<Rect>,
    /// Ctrl'e basılmadan önceki son bölge: birleştirmenin bir ucu.
    anchor: Option<usize>,
    /// Bırakılırsa pencerenin gideceği alan (boşluksuz).
    pick: Option<Rect>,
    /// Katman görünürken çizilen durum (aynıysa yeniden çizilmez).
    drawn: Option<(isize, Option<Rect>)>,
}

struct State {
    overlay: HWND,
    hook: HWINEVENTHOOK,
    set: Settings,
    drag: Option<Drag>,
    canvas: Option<Canvas>,
    /// Sürükleme bitince yerleştirilecek pencere ve alanı.
    pending: Option<(HWND, Rect)>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.try_borrow_mut().ok().and_then(|mut s| s.as_mut().map(f)))
}

fn gap_px(s: &Settings, scale: f32) -> i32 {
    (s.gap as f32 * scale).round() as i32
}

fn hide(st: &mut State) {
    if let Some(d) = st.drag.as_mut() {
        d.drawn = None;
    }
    unsafe {
        let _ = ShowWindow(st.overlay, SW_HIDE);
    }
}

/// Zamanlayıcı: Shift'e, Ctrl'e ve imlece bakıp seçimi ve katmanı günceller.
fn poll(st: &mut State) {
    let Some(d) = st.drag.as_mut() else { return };
    if !pressed(VK_SHIFT) {
        d.pick = None;
        d.anchor = None;
        if d.drawn.is_some() {
            hide(st);
        }
        return;
    }
    let p = cursor();
    let (m, work, scale) = monitor_at(p);
    if d.mon != Some(m.0 as isize) || d.work != work {
        let aspect = (work[2] - work[0]) as f32 / (work[3] - work[1]).max(1) as f32;
        d.zones = zones::rects(&zones::fractions(st.set.layout, &st.set.custom, aspect), work);
        d.mon = Some(m.0 as isize);
        d.work = work;
        d.scale = scale;
        d.anchor = None;
    }
    let cur = zones::hit(&d.zones, p.x, p.y);
    let ctrl = pressed(VK_CONTROL);
    if !ctrl || d.anchor.is_none() {
        d.anchor = cur;
    }
    d.pick = match (cur, d.anchor) {
        (Some(c), Some(a)) if ctrl => Some(zones::span(d.zones[a], d.zones[c])),
        (Some(c), _) => Some(d.zones[c]),
        _ => None,
    };
    let key = (m.0 as isize, d.pick);
    if d.drawn == Some(key) {
        return;
    }
    let (w, h) = (work[2] - work[0], work[3] - work[1]);
    if w <= 0 || h <= 0 {
        return;
    }
    if st.canvas.as_ref().is_none_or(|c| c.w != w || c.h != h) {
        st.canvas = Canvas::new(w, h);
    }
    let Some(c) = st.canvas.as_mut() else { return };
    let gap = gap_px(&st.set, d.scale);
    let local = |r: Rect| [r[0] - work[0], r[1] - work[1], r[2] - work[0], r[3] - work[1]];
    let shown: Vec<Rect> = d.zones.iter().map(|z| local(zones::inset(*z, work, gap))).collect();
    let pick = d.pick.map(|r| local(zones::inset(r, work, gap)));
    paint(c.px(), w, h, &shown, pick, st.set.accent, d.scale);
    unsafe {
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
        let at = POINT { x: work[0], y: work[1] };
        let size = SIZE { cx: w, cy: h };
        let from = POINT::default();
        let _ = UpdateLayeredWindow(st.overlay, None, Some(&at), Some(&size), c.dc, Some(&from), COLORREF(0), Some(&blend), ULW_ALPHA);
        if d.drawn.is_none() {
            // Sürüklenen pencerenin hemen arkasına: pencere bölgelerin üstünde görünür.
            let _ = SetWindowPos(st.overlay, d.hwnd, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
    }
    d.drawn = Some(key);
}

unsafe extern "system" fn on_drag(_: HWINEVENTHOOK, event: u32, hwnd: HWND, id_object: i32, id_child: i32, _: u32, _: u32) {
    if id_object != OBJID_WINDOW.0 || id_child != 0 {
        return;
    }
    with(|st| match event {
        EVENT_SYSTEM_MOVESIZESTART => {
            st.drag = None;
            if !st.set.on || !movable(hwnd) {
                return;
            }
            let p = cursor();
            let (_, work, scale) = monitor_at(p);
            if resizing(hwnd, p, scale) {
                return;
            }
            st.drag = Some(Drag { hwnd, mon: None, work, scale, zones: Vec::new(), anchor: None, pick: None, drawn: None });
            let _ = SetTimer(st.overlay, TIMER_POLL, 15, None);
            poll(st);
        }
        EVENT_SYSTEM_MOVESIZEEND => {
            let Some(d) = st.drag.as_ref().filter(|d| d.hwnd == hwnd) else { return };
            let _ = KillTimer(st.overlay, TIMER_POLL);
            // Bırakıldığı anda Shift hâlâ basılıysa yerleştir; değilse sıradan taşımadır.
            let target = d.pick.filter(|_| pressed(VK_SHIFT)).map(|r| zones::inset(r, d.work, gap_px(&st.set, d.scale)));
            hide(st);
            st.drag = None;
            if let Some(r) = target {
                st.pending = Some((hwnd, r));
                let _ = SetTimer(st.overlay, TIMER_PLACE, 30, None);
            }
        }
        _ => {}
    });
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_TIMER if wp.0 == TIMER_POLL => {
                with(poll);
                LRESULT(0)
            }
            WM_TIMER if wp.0 == TIMER_PLACE => {
                let _ = KillTimer(hwnd, TIMER_PLACE);
                if let Some(Some((h, r))) = with(|st| st.pending.take()) {
                    place(h, r);
                }
                LRESULT(0)
            }
            WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

/// `WANT`'taki ayarı uygular: kancayı takar/söker.
fn apply(st: &mut State) {
    let Some(s) = WANT.lock().ok().and_then(|w| w.clone()) else { return };
    let was = st.set.on;
    st.set = s;
    if st.set.on == was {
        return;
    }
    unsafe {
        if st.set.on {
            st.hook = SetWinEventHook(EVENT_SYSTEM_MOVESIZESTART, EVENT_SYSTEM_MOVESIZEEND, None, Some(on_drag), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS);
            if st.hook.is_invalid() {
                warn!("bölgeler: sürükleme kancası kurulamadı");
                st.set.on = false;
                return;
            }
        } else {
            if !st.hook.is_invalid() {
                let _ = UnhookWinEvent(st.hook);
            }
            st.hook = HWINEVENTHOOK::default();
            let _ = KillTimer(st.overlay, TIMER_POLL);
            hide(st);
            st.drag = None;
            st.canvas = None;
        }
    }
    info!("pencere bölgeleri: {}", if st.set.on { "açık" } else { "kapalı" });
}

fn run() {
    unsafe {
        let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let Ok(inst) = GetModuleHandleW(None) else { return };
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: inst.into(), lpszClassName: CLASS, ..Default::default() });
        let overlay = match CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            CLASS,
            w!("CarlUI Zones"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            inst,
            None,
        ) {
            Ok(h) => h,
            Err(e) => {
                warn!("bölgeler: katman penceresi açılamadı: {e}");
                return;
            }
        };
        // Mesaj kuyruğu pencereyle oluştu; artık iş parçacığına mesaj gönderilebilir.
        THREAD.store(GetCurrentThreadId(), Release);
        let off = Settings { on: false, layout: ZoneLayout::Auto, custom: Vec::new(), gap: 0, accent: (0, 0, 0) };
        STATE.with(|s| *s.borrow_mut() = Some(State { overlay, hook: HWINEVENTHOOK::default(), set: off, drag: None, canvas: None, pending: None }));
        with(apply);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.hwnd.0.is_null() && msg.message == WM_SYNC {
                with(apply);
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renk_paketleme_gidip_gelir() {
        let c = [0.2, 0.4, 0.6, 0.8];
        let back = unpack(pack(c));
        for i in 0..4 {
            assert!((back[i] - c[i]).abs() < 0.01);
        }
        assert_eq!(pack([0.0, 0.0, 0.0, 1.0]), 0xFF00_0000);
    }

    #[test]
    fn secim_vurgulanir_disi_karartilir() {
        let (w, h) = (100, 60);
        let mut px = vec![0u32; (w * h) as usize];
        let zones = [[0, 0, 50, 60], [50, 0, 100, 60]];
        paint(&mut px, w, h, &zones, Some(zones[1]), (255, 0, 0), 1.0);
        let at = |x: i32, y: i32| unpack(px[(y * w + x) as usize]);
        // Seçili bölgenin ortası kırmızıya çalar, diğerininki gridir.
        let sel = at(75, 30);
        let other = at(25, 30);
        assert!(sel[0] > sel[1] + 0.2 && sel[3] > other[3]);
        assert!((other[0] - other[1]).abs() < 0.01);
        // Köşenin tam ucu yuvarlatıldığı için yalnızca zemin rengindedir.
        assert!((at(0, 0)[3] - 0.18).abs() < 0.01);
    }
}
