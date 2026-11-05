//! Bu modül CarlHub'daki ekran görüntüsü aracının yerini alıyor. Eskisi tüm
//! ekranı seçenek sunmadan masaüstüne kaydediyordu; tam ekran oyunda da
//! oyunu arka plana atıyordu. Burada:
//! - Kısayol (`shortcuts.screenshot`, varsayılan Alt+X) alçak düzey klavye
//!   kancasıyla yakalanır; oyunlarda da çalışır.
//! - Tüm monitörler o an birlikte dondurulur, düzenleyici hepsine yayılır:
//!   alan seç (monitörler arası da), çiz, yaz, sil, geri al; Kopyala ya da Kaydet.
//! - Özel (exclusive) tam ekran oyunda üstüne pencere çizilemez: oyunun ekranı
//!   doğrudan alınıp panoya kopyalanır; bildirimdeki "Düzenle" ile alan kırpılır.
//! - Oyun öndeyse düzenleyici odak almaz: tuşlar ve tıklamalar kancadan
//!   okunup oyuna gitmeden yutulur, böylece oyun simge durumuna düşmez.
//! - "Yazıyı kopyala" (Ctrl+T) seçimdeki metni OCR ile okuyup panoya koyar (bkz. `ocr`).
//! - Seçimdeki QR kodları okunup türüne ayrılır (bkz. `qr`); 2FA QR'ı tek
//!   düğmeyle Carl Vault'a aktarılır.
//! - Bitince adaya "Aç / Düzenle / Sil" düğmeli bir bildirim düşer. Sil
//!   dosyayı Geri Dönüşüm Kutusu'na atar, panodaki kaydı da kaldırır.

mod capture;
mod ocr;
mod overlay;
pub(crate) mod qr;

use crate::*;
use carlui_core::config::{ShotAction, ShotFormat};
use overlay::{Out, Overlay};
use zeroize::{Zeroize, Zeroizing};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU32},
};
use windows::Win32::UI::Shell::{SHFileOperationW, ShellExecuteW, SHFILEOPSTRUCTW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_SILENT, FO_DELETE};

// WM_APP+40/41 carlui-core'da (WM_CARLUI_SEARCH/START) adaya gelen mesajlar;
// çakışmasın diye ekran görüntüsü 60'tan başlar.
const WM_APP_SHOT: u32 = WM_APP + 60;
const WM_APP_SHOT_KEY: u32 = WM_APP + 61;
const WM_APP_SHOT_CHAR: u32 = WM_APP + 62;
const WM_APP_SHOT_MOUSE: u32 = WM_APP + 63;
const CLASS: PCWSTR = w!("CarlUI.Shot");

/// Kısayol: sanal tuş + değiştirici bitleri (1 Ctrl, 2 Alt, 4 Shift, 8 Win).
static SHOT_VK: AtomicU32 = AtomicU32::new(0);
static SHOT_MODS: AtomicU32 = AtomicU32::new(0);
/// Düzenleyici açık ve odaksız: tüm tuşlar/tıklamalar bize gelsin.
static PASSIVE: AtomicBool = AtomicBool::new(false);
static OVERLAY_HWND: AtomicIsize = AtomicIsize::new(0);
static SWALLOW_UP: AtomicU32 = AtomicU32::new(0);

pub struct Shots {
    kb_hook: HHOOK,
    mouse_hook: HHOOK,
    overlay: Option<Overlay>,
    last: Option<String>,
    count: u32,
}

fn down(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk.0 as i32) < 0 }
}

fn mods_now() -> u32 {
    (down(VK_CONTROL) as u32) | (down(VK_MENU) as u32) << 1 | (down(VK_SHIFT) as u32) << 2 | ((down(VK_LWIN) || down(VK_RWIN)) as u32) << 3
}

unsafe extern "system" fn kb_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 {
        let k = &*(lp.0 as *const KBDLLHOOKSTRUCT);
        let msg = wp.0 as u32;
        let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        let injected = k.flags.0 & LLKHF_INJECTED.0 != 0;
        let island = HWND(HWND_ISLAND.load(Ordering::Relaxed) as _);
        if PASSIVE.load(Ordering::Relaxed) && !injected {
            // Düzenleyici oyun üstünde açık: tuşlar oyuna gitmesin.
            if is_down {
                let _ = PostMessageW(island, WM_APP_SHOT_KEY, WPARAM(k.vkCode as usize), LPARAM(down(VK_CONTROL) as isize));
                let mut state = [0u8; 256];
                for vk in [VK_SHIFT, VK_LSHIFT, VK_RSHIFT, VK_CONTROL, VK_MENU, VK_RMENU, VK_LCONTROL, VK_RCONTROL] {
                    if down(vk) {
                        state[vk.0 as usize] = 0x80;
                    }
                }
                if GetKeyState(VK_CAPITAL.0 as i32) & 1 != 0 {
                    state[VK_CAPITAL.0 as usize] = 1;
                }
                let mut buf = [0u16; 4];
                let layout = GetKeyboardLayout(GetWindowThreadProcessId(GetForegroundWindow(), None));
                let n = ToUnicodeEx(k.vkCode, k.scanCode, &state, &mut buf, 4, layout);
                if n > 0 && !(down(VK_CONTROL) && !down(VK_MENU)) {
                    for c in char::decode_utf16(buf[..n as usize].iter().copied()).flatten() {
                        let _ = PostMessageW(island, WM_APP_SHOT_CHAR, WPARAM(c as usize), LPARAM(0));
                    }
                }
            }
            // Değiştiriciler geçsin (sistem durumları bozulmasın), gerisi yutulur.
            let vk = VIRTUAL_KEY(k.vkCode as u16);
            if ![VK_SHIFT, VK_LSHIFT, VK_RSHIFT, VK_CONTROL, VK_LCONTROL, VK_RCONTROL, VK_MENU, VK_LMENU, VK_RMENU].contains(&vk) {
                return LRESULT(1);
            }
        } else if !injected {
            let want = SHOT_VK.load(Ordering::Relaxed);
            if want != 0 && k.vkCode == want {
                if is_down && mods_now() == SHOT_MODS.load(Ordering::Relaxed) {
                    SWALLOW_UP.store(want, Ordering::Relaxed);
                    let _ = PostMessageW(island, WM_APP_SHOT, WPARAM(0), LPARAM(0));
                    return LRESULT(1);
                }
                if !is_down && SWALLOW_UP.swap(0, Ordering::Relaxed) == want {
                    return LRESULT(1);
                }
            }
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

unsafe extern "system" fn mouse_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 && PASSIVE.load(Ordering::Relaxed) {
        let m = &*(lp.0 as *const MSLLHOOKSTRUCT);
        let msg = wp.0 as u32;
        let ov = HWND(OVERLAY_HWND.load(Ordering::Relaxed) as _);
        let packed = LPARAM(((m.pt.y as i64) << 32 | (m.pt.x as u32 as i64)) as isize);
        match msg {
            WM_MOUSEMOVE => {
                let _ = PostMessageW(ov, WM_APP_SHOT_MOUSE, WPARAM(msg as usize), packed);
            }
            WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONUP => {
                let _ = PostMessageW(ov, WM_APP_SHOT_MOUSE, WPARAM(msg as usize), packed);
                return LRESULT(1);
            }
            WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_MBUTTONUP | WM_MOUSEWHEEL | WM_MOUSEHWHEEL | WM_LBUTTONDBLCLK => return LRESULT(1),
            _ => {}
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let pt = |lp: LPARAM| ((lp.0 & 0xFFFF) as i16 as f32, ((lp.0 >> 16) & 0xFFFF) as i16 as f32);
        match msg {
            WM_PAINT => {
                // Ada meşgulse (pencere açılırken eşzamanlı gelen ilk WM_PAINT) geçersiz
                // bırak: Windows boşalınca yeniden ister. Doğrulansaydı donmuş kare ilk
                // fare hareketine kadar hiç çizilmiyordu.
                let busy = with_app(|a| {
                    if let Some(o) = a.shots.overlay.as_mut() {
                        o.paint();
                    } else {
                        let _ = ValidateRect(hwnd, None);
                    }
                })
                .is_none();
                if busy {
                    let _ = DefWindowProcW(hwnd, msg, wp, lp);
                    let _ = InvalidateRect(hwnd, None, false);
                }
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_MOUSEACTIVATE if PASSIVE.load(Ordering::Relaxed) => LRESULT(MA_NOACTIVATE as isize),
            WM_SETCURSOR => {
                let _ = SetCursor(LoadCursorW(None, IDC_CROSS).unwrap_or_default());
                LRESULT(1)
            }
            WM_LBUTTONDOWN => {
                let (x, y) = pt(lp);
                SetCapture(hwnd);
                with_app(|a| a.shot_input(|o| o.down(x, y)));
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let (x, y) = pt(lp);
                let b = wp.0 & 1 != 0;
                with_app(|a| a.shot_input(|o| {
                    o.moved(x, y, b);
                    Out::None
                }));
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let (x, y) = pt(lp);
                let _ = ReleaseCapture();
                with_app(|a| a.shot_input(|o| {
                    o.up(x, y);
                    Out::None
                }));
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                with_app(|a| a.shot_input(|o| if o.typing() { o.key(0x1B, false) } else { Out::Cancel }));
                LRESULT(0)
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
                let vk = wp.0 as u16;
                with_app(|a| a.shot_input(|o| o.key(vk, ctrl)));
                LRESULT(0)
            }
            WM_CHAR => {
                if let Some(c) = char::from_u32(wp.0 as u32) {
                    with_app(|a| a.shot_input(|o| {
                        o.char(c);
                        Out::None
                    }));
                }
                LRESULT(0)
            }
            WM_APP_SHOT_MOUSE => {
                // Kancadan gelen ekran koordinatı → pencere içi.
                let (sx, sy) = ((lp.0 & 0xFFFF_FFFF) as u32 as i32, (lp.0 >> 32) as i32);
                let mut p = POINT { x: sx, y: sy };
                let _ = ScreenToClient(hwnd, &mut p);
                let (x, y) = (p.x as f32, p.y as f32);
                let left = down(VK_LBUTTON);
                match wp.0 as u32 {
                    WM_LBUTTONDOWN => with_app(|a| a.shot_input(|o| o.down(x, y))),
                    WM_LBUTTONUP => with_app(|a| a.shot_input(|o| {
                        o.up(x, y);
                        Out::None
                    })),
                    WM_RBUTTONUP => with_app(|a| a.shot_input(|o| if o.typing() { o.key(0x1B, false) } else { Out::Cancel })),
                    _ => with_app(|a| a.shot_input(|o| {
                        o.moved(x, y, left);
                        Out::None
                    })),
                };
                LRESULT(0)
            }
            WM_SIZE => {
                with_app(|a| {
                    if let Some(o) = a.shots.overlay.as_mut() {
                        o.resized();
                    }
                });
                LRESULT(0)
            }
            WM_CLOSE => {
                with_app(|a| a.shot_finish(Out::Cancel));
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

impl Shots {
    pub fn new(_island: HWND) -> Self {
        unsafe {
            let inst = GetModuleHandleW(None).unwrap_or_default();
            RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(overlay_proc), hInstance: inst.into(), lpszClassName: CLASS, ..Default::default() });
            let kb_hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb_proc), inst, 0).unwrap_or_default();
            if kb_hook.is_invalid() {
                carlui_core::warn!("ekran görüntüsü kısayolu dinlenemiyor");
            }
            Self { kb_hook, mouse_hook: HHOOK::default(), overlay: None, last: None, count: 0 }
        }
    }

    pub fn set_combo(&self, cfg: &Config) {
        match cfg.shortcuts.screenshot_combo() {
            Some(c) => {
                let m = c.mods;
                SHOT_VK.store(c.vk, Ordering::Relaxed);
                SHOT_MODS.store(m.ctrl as u32 | (m.alt as u32) << 1 | (m.shift as u32) << 2 | (m.win as u32) << 3, Ordering::Relaxed);
            }
            None => SHOT_VK.store(0, Ordering::Relaxed),
        }
    }

    pub fn debug(&self) -> serde_json::Value {
        let o = self.overlay.as_ref();
        serde_json::json!({
            "hook": !self.kb_hook.is_invalid(),
            "open": o.is_some(),
            "passive": o.map(|o| o.passive),
            "editing": o.and_then(|o| o.editing.as_ref().map(|p| p.display().to_string())),
            "selection": o.map(|o| o.has_selection()),
            "sel": o.and_then(|o| o.debug_sel()).map(|s| [s.0, s.1, s.2, s.3]),
            "last": self.last,
            "count": self.count,
        })
    }
}

impl Drop for Shots {
    fn drop(&mut self) {
        unsafe {
            if !self.kb_hook.is_invalid() {
                let _ = UnhookWindowsHookEx(self.kb_hook);
            }
            if !self.mouse_hook.is_invalid() {
                let _ = UnhookWindowsHookEx(self.mouse_hook);
            }
        }
    }
}

fn desktop() -> PathBuf {
    use windows::Win32::UI::Shell::{FOLDERID_Desktop, SHGetKnownFolderPath, KNOWN_FOLDER_FLAG};
    unsafe {
        if let Ok(p) = SHGetKnownFolderPath(&FOLDERID_Desktop, KNOWN_FOLDER_FLAG(0), None) {
            let s = p.to_string().unwrap_or_default();
            windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
            if !s.is_empty() {
                return PathBuf::from(s);
            }
        }
    }
    PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Desktop")
}

/// (tarih, saat): "2026-11-05", "14.03.22".
fn stamp() -> (String, String) {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    (format!("{}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay), format!("{:02}.{:02}.{:02}", t.wHour, t.wMinute, t.wSecond))
}

/// Aynı ada dosya varsa sonuna (2), (3) … ekler.
fn unique(dir: &Path, base: &str, ext: &str) -> PathBuf {
    let mut p = dir.join(format!("{base}.{ext}"));
    let mut i = 2;
    while p.exists() {
        p = dir.join(format!("{base} ({i}).{ext}"));
        i += 1;
    }
    p
}

pub fn open(path: &str) {
    let p = HSTRING::from(path);
    unsafe {
        ShellExecuteW(None, w!("open"), &p, None, None, SW_SHOWNORMAL);
    }
}

/// Dosyayı Geri Dönüşüm Kutusu'na gönderir.
pub fn recycle(path: &str) {
    if !Path::new(path).exists() {
        return;
    }
    let from: Vec<u16> = path.encode_utf16().chain([0, 0]).collect();
    let mut op = SHFILEOPSTRUCTW { wFunc: FO_DELETE, pFrom: PCWSTR(from.as_ptr()), fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT).0 as u16, ..Default::default() };
    let r = unsafe { SHFileOperationW(&mut op) };
    info!("ekran görüntüsü Geri Dönüşüm Kutusu'na: {path} ({})", if r == 0 { "tamam" } else { "başarısız" });
}

fn load_png(path: &str) -> Option<capture::Frame> {
    let bytes = std::fs::read(path).ok()?;
    let wic = carlui_clip::image::wic()?;
    let (conv, w, h) = carlui_clip::image::decode(&wic, &bytes)?;
    let mut px = vec![0u8; (w * h * 4) as usize];
    unsafe {
        use windows::Win32::Graphics::Imaging::IWICBitmapSource;
        let src: IWICBitmapSource = conv.cast().ok()?;
        src.CopyPixels(std::ptr::null(), w * 4, &mut px).ok()?;
    }
    Some(capture::Frame { w, h, px, rect: RECT::default(), monitors: Vec::new() })
}

/// Ham kareyi (BGRA) PNG'ye çevirir.
fn encode(f: &capture::Frame) -> Option<Vec<u8>> {
    use windows::Win32::Graphics::Imaging::{GUID_WICPixelFormat32bppBGRA, IWICBitmapSource};
    let wic = carlui_clip::image::wic()?;
    unsafe {
        let bmp = wic.CreateBitmapFromMemory(f.w, f.h, &GUID_WICPixelFormat32bppBGRA, f.w * 4, &f.px).ok()?;
        let src: IWICBitmapSource = bmp.cast().ok()?;
        carlui_clip::image::encode_png(&wic, &src)
    }
}

impl App {
    /// Kısayola basıldı: ekranı dondur, düzenleyiciyi aç.
    fn shot_start(&mut self) {
        if self.shots.overlay.is_some() {
            return;
        }
        // Alt bırakılınca öndeki uygulamanın menüsü açılmasın.
        unsafe {
            let k = |up: bool| INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(0xE8), dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, ..Default::default() } } };
            SendInput(&[k(false), k(true)], std::mem::size_of::<INPUT>() as i32);
        }
        let t = Instant::now();
        // Özel tam ekran oyunun üstüne pencere çizilemez (açılırsa oyun ya
        // simge durumuna düşer ya da düzenleyici görünmez). Oyunun ekranı
        // olduğu gibi alınır; alan, oyundan sonra bildirimdeki "Düzenle" ile kırpılır.
        if self.exclusive {
            let Some(frame) = capture::monitor_at(cursor()) else {
                carlui_core::warn!("ekran yakalanamadı");
                return;
            };
            info!("özel tam ekran: ekran doğrudan alındı {}×{} ({} ms)", frame.w, frame.h, t.elapsed().as_millis());
            if let Some(png) = encode(&frame) {
                self.shot_store(png, frame.w, frame.h, Out::Enter, None);
            }
            return;
        }
        let Some(frame) = capture::all() else {
            carlui_core::warn!("ekran yakalanamadı");
            return;
        };
        info!("ekranlar yakalandı {}×{}, {} monitör ({} ms)", frame.w, frame.h, frame.monitors.len(), t.elapsed().as_millis());
        self.shot_open(frame, None);
        info!("düzenleyici ekranda ({} ms)", t.elapsed().as_millis());
    }

    /// Bildirimdeki "Düzenle": kayıtlı resmi düzenleyicide aç.
    pub(crate) fn shot_edit(&mut self, path: &str) {
        if self.shots.overlay.is_some() {
            return;
        }
        match load_png(path) {
            Some(mut f) => {
                f.rect = self.mon;
                self.shot_open(f, Some(PathBuf::from(path)));
            }
            None => open(path),
        }
    }

    fn shot_open(&mut self, frame: capture::Frame, editing: Option<PathBuf>) {
        let r = if editing.is_some() { self.mon } else { frame.rect };
        // Oyun öndeyse odak alma (oyun arka plana düşmesin).
        let passive = self.game || searching::game_in_front();
        unsafe {
            let ex = WS_EX_TOPMOST | WS_EX_TOOLWINDOW | if passive { WS_EX_NOACTIVATE } else { WINDOW_EX_STYLE(0) };
            let Ok(hwnd) = CreateWindowExW(ex, CLASS, w!("CarlUI Screenshot"), WS_POPUP, r.left, r.top, r.right - r.left, r.bottom - r.top, None, None, GetModuleHandleW(None).unwrap_or_default(), None) else { return };
            let s = GetDpiForWindow(hwnd) as f32 / 96.0;
            match Overlay::new(hwnd, frame, editing, passive, s) {
                Ok(o) => self.shots.overlay = Some(o),
                Err(e) => {
                    carlui_core::warn!("düzenleyici açılamadı: {e}");
                    let _ = DestroyWindow(hwnd);
                    return;
                }
            }
            OVERLAY_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
            if passive {
                PASSIVE.store(true, Ordering::SeqCst);
                let inst = GetModuleHandleW(None).unwrap_or_default();
                self.shots.mouse_hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), inst, 0).unwrap_or_default();
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            } else {
                let _ = ShowWindow(hwnd, SW_SHOW);
                searching::force_foreground(hwnd);
            }
            // Donmuş kare hemen çizilsin (WM_PAINT'i beklemeden).
            if let Some(o) = self.shots.overlay.as_mut() {
                o.paint();
            }
        }
        info!("ekran görüntüsü düzenleyicisi açıldı (odaksız: {passive})");
    }

    fn shot_input(&mut self, f: impl FnOnce(&mut Overlay) -> Out) {
        let Some(o) = self.shots.overlay.as_mut() else { return };
        let out = f(o);
        if out != Out::None {
            self.shot_finish(out);
        }
    }

    fn shot_close(&mut self) {
        PASSIVE.store(false, Ordering::SeqCst);
        unsafe {
            if !self.shots.mouse_hook.is_invalid() {
                let _ = UnhookWindowsHookEx(self.shots.mouse_hook);
                self.shots.mouse_hook = HHOOK::default();
            }
            if let Some(o) = self.shots.overlay.take() {
                let _ = DestroyWindow(o.hwnd);
            }
        }
        OVERLAY_HWND.store(0, Ordering::SeqCst);
    }

    fn shot_finish(&mut self, out: Out) {
        let Some(o) = self.shots.overlay.as_ref() else { return };
        if out == Out::Cancel {
            self.shot_close();
            return;
        }
        if let Out::Qr(i, act) = out {
            let q = o.qr.get(i).map(|q| (q.kind, q.text.clone(), q.copy_text(), q.title()));
            self.shot_close();
            if let Some(q) = q {
                self.qr_act(act, q);
            }
            return;
        }
        if out == Out::Ocr {
            let area = o.sel_px();
            let px = self.shots.overlay.as_mut().map(|o| (std::mem::take(&mut o.frame.px), o.frame.w, o.frame.h));
            self.shot_close();
            if let (Some(area), Some((px, w, h))) = (area, px) {
                self.shot_ocr(px, w, h, area);
            }
            return;
        }
        let Some((png, w, h)) = o.render_png() else {
            self.shot_close();
            return;
        };
        let editing = o.editing.clone();
        self.shot_close();
        self.shot_store(png, w, h, out, editing);
    }

    /// "Yazıyı kopyala": OCR arayüzü dondurmasın diye işçi iş parçacığında
    /// çalışır; metin panoya yazılır, sonuç adaya bildirim olarak düşer.
    fn shot_ocr(&mut self, px: Vec<u8>, w: u32, h: u32, area: (u32, u32, u32, u32)) {
        let raw = self.island.hwnd.0 as isize;
        let notes = self.notes_in.clone();
        let _ = std::thread::Builder::new().name("ocr".into()).spawn(move || {
            let res = ocr::recognize(&px, w, h, area);
            drop(px);
            let (title, body) = match res {
                Ok(text) if text.is_empty() => (t!("Yazı bulunamadı", "No text found").to_string(), t!("Seçili alanda okunabilir yazı yok.", "No readable text in the selected area.").to_string()),
                Ok(text) => {
                    let ok = carlui_clip::write::set(HWND(raw as _), carlui_clip::write::Data::Text(&text)).is_some();
                    let first: String = text.lines().next().unwrap_or_default().chars().take(60).collect();
                    let n = text.lines().count();
                    info!("OCR: {n} satır okundu");
                    let head = if ok { t!("Yazı panoya kopyalandı", "Text copied to clipboard") } else { t!("Panoya konamadı", "Couldn't copy") };
                    (head.to_string(), if n > 1 { t!(format!("{first} … ({n} satır)"), format!("{first} … ({n} lines)")) } else { first })
                }
                Err(e) => {
                    carlui_core::warn!("OCR olmadı: {e}");
                    (t!("Yazı okunamadı", "Couldn't read text").to_string(), e)
                }
            };
            if let Ok(mut q) = notes.lock() {
                q.push_back(Note { app: t!("Ekran görüntüsü", "Screenshot").into(), title, body, ..Default::default() });
            }
            unsafe {
                let _ = PostMessageW(HWND(raw as _), WM_APP_NOTE, WPARAM(0), LPARAM(0));
            }
        });
    }

    /// QR düğmesi: (tür, okunan metin, kopyalanacak metin, başlık).
    pub(crate) fn qr_act(&mut self, act: qr::Act, (kind, text, copy, title): (qr::Kind, Zeroizing<String>, Zeroizing<String>, String)) {
        let note = |app: &mut Self, title: String, body: String| app.push_note(Note { app: "QR".into(), title, body, ..Default::default() });
        match act {
            qr::Act::Vault if kind == qr::Kind::Totp => self.qr_to_vault(text),
            qr::Act::Open if kind == qr::Kind::Url => unsafe {
                info!("QR bağlantısı açıldı: {title}");
                ShellExecuteW(None, w!("open"), &HSTRING::from(text.trim()), None, None, SW_SHOWNORMAL);
            },
            qr::Act::Copy => {
                let ok = carlui_clip::write::set(self.island.hwnd, carlui_clip::write::Data::Text(&copy)).is_some();
                let head = if ok { t!("QR panoya kopyalandı", "QR copied to clipboard") } else { t!("Panoya konamadı", "Couldn't copy") };
                note(self, head.into(), title);
            }
            _ => {}
        }
    }

    /// 2FA QR'ını Carl Vault'a verir. Anahtar yalnızca kasanın doğrulanmış
    /// yerel borusuna gider (panoya ya da diske yazılmaz). Kasa kapalıysa
    /// açılır ve hazır olunca istek yinelenir.
    fn qr_to_vault(&mut self, uri: Zeroizing<String>) {
        use carlui_core::vaultlink::{ask, Request};
        let raw = self.island.hwnd.0 as isize;
        let notes = self.notes_in.clone();
        let _ = std::thread::Builder::new().name("qr-vault".into()).spawn(move || {
            let mut req = Request::ImportTotp { uri: uri.to_string() };
            drop(uri);
            unsafe {
                // Kasa penceresi öne gelebilsin (odak şu an bizde).
                let _ = AllowSetForegroundWindow(ASFW_ANY);
            }
            let mut reply = ask(&req);
            if reply.is_err() {
                let exe = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("carlui-vault.exe")));
                if exe.is_some_and(|e| std::process::Command::new(e).spawn().is_ok()) {
                    for _ in 0..30 {
                        std::thread::sleep(Duration::from_millis(250));
                        unsafe {
                            let _ = AllowSetForegroundWindow(ASFW_ANY);
                        }
                        reply = ask(&req);
                        if reply.is_ok() {
                            break;
                        }
                    }
                }
            }
            if let Request::ImportTotp { uri } = &mut req {
                uri.zeroize();
            }
            let (ok, msg) = match reply {
                Ok(r) => (r.ok, r.message),
                Err(m) => (false, m),
            };
            info!("QR: Carl Vault aktarımı {}: {msg}", if ok { "tamam" } else { "olmadı" });
            let (title, body) = if ok { (t!("2FA Carl Vault'a aktarıldı", "2FA sent to Carl Vault").to_string(), msg) } else { (t!("Carl Vault'a aktarılamadı", "Couldn't send to Carl Vault").to_string(), msg) };
            if let Ok(mut q) = notes.lock() {
                q.push_back(Note { app: "Carl Vault".into(), title, body, ..Default::default() });
            }
            unsafe {
                let _ = PostMessageW(HWND(raw as _), WM_APP_NOTE, WPARAM(0), LPARAM(0));
            }
        });
    }

    /// PNG'yi dosyaya (ve Kopyala'da panoya) yazar, adaya bildirim düşer.
    fn shot_store(&mut self, png: Vec<u8>, w: u32, h: u32, out: Out, editing: Option<PathBuf>) {
        // Enter (ve özel tam ekran) ayardaki varsayılan eylemi yapar.
        let (copy, save) = match out {
            Out::Copy => (true, false),
            Out::Save => (false, true),
            _ => match self.cfg.shots.action {
                ShotAction::Copy => (true, false),
                ShotAction::Save => (false, true),
                ShotAction::Both => (true, true),
            },
        };
        let jpg = self.cfg.shots.format == ShotFormat::Jpg;
        let (date, time) = stamp();
        let base = self.cfg.shots.file_name(&date, &time);
        let ext = if jpg { "jpg" } else { "png" };
        let path = match &editing {
            Some(p) => p.clone(),
            None if save => unique(&desktop(), &base, ext),
            // Yalnızca kopyalanan da bir yere yazılır ki "Aç / Düzenle" çalışsın.
            None => {
                let dir = carlui_core::paths::data_dir().join("shots");
                let _ = std::fs::create_dir_all(&dir);
                unique(&dir, &base, ext)
            }
        };
        // Düzenlenen dosya kendi biçiminde kalır.
        let as_jpg = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"));
        let bytes = if as_jpg { carlui_clip::image::wic().and_then(|wic| carlui_clip::image::png_to_jpg(&wic, &png)) } else { None };
        if let Err(e) = std::fs::write(&path, bytes.as_deref().unwrap_or(&png)) {
            carlui_core::warn!("ekran görüntüsü yazılamadı: {e}");
        }
        let out = if copy { Out::Copy } else { Out::Save };
        let mut clip_id = None;
        if copy {
            if carlui_clip::write::set(self.island.hwnd, carlui_clip::write::Data::Png(&png)).is_some() {
                clip_id = Some(format!("t:{}", carlui_clip::store::now_ms()));
            }
        }
        let p = path.display().to_string();
        info!("ekran görüntüsü {}×{}: {} ({})", w, h, p, if out == Out::Copy { "kopyalandı" } else { "kaydedildi" });
        self.shots.last = Some(p.clone());
        self.shots.count += 1;
        let thumb = carlui_clip::image::wic().and_then(|wic| carlui_clip::image::thumb_png(&wic, &png, 96)).map(Arc::new);
        // Düzenlenen resmin eski bildirimi yenisiyle değişsin.
        self.hub.notes.retain(|n| n.shot.as_deref() != Some(p.as_str()));
        let body = match (out, editing.is_some()) {
            (Out::Copy, false) if self.exclusive => t!(format!("Oyun ekranı panoda · {w}×{h} · alan için Düzenle"), format!("Game screen on clipboard · {w}×{h} · Edit to crop")),
            (Out::Copy, false) if save => t!(format!("Panoya kopyalandı, masaüstüne kaydedildi · {w}×{h}"), format!("Copied and saved to desktop · {w}×{h}")),
            (Out::Copy, _) => t!(format!("Panoya kopyalandı · {w}×{h}"), format!("Copied to clipboard · {w}×{h}")),
            (_, true) => t!("Düzenleme kaydedildi", "Edit saved").to_string(),
            _ => t!(format!("Masaüstüne kaydedildi · {w}×{h}"), format!("Saved to desktop · {w}×{h}")),
        };
        self.push_note(Note { app: t!("Ekran görüntüsü", "Screenshot").into(), title: t!("Ekran görüntüsü", "Screenshot").into(), body, logo: thumb, shot: Some(p), clip_id, ..Default::default() });
    }

    /// Test: `shot-start`, `shot-edit {path}`, `shot-key {vk, ctrl}`, `shot-down/move/up {x, y}`, `shot-char {c}`.
    pub(crate) fn shot_event(&mut self, name: &str, data: &serde_json::Value) {
        let f = |k: &str| data.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        match name {
            "shot-start" => self.shot_start(),
            "shot-edit" => self.shot_edit(data.get("path").and_then(|v| v.as_str()).unwrap_or_default()),
            "shot-key" => {
                let (vk, ctrl) = (f("vk") as u16, data.get("ctrl").and_then(|v| v.as_bool()).unwrap_or(false));
                self.shot_input(|o| o.key(vk, ctrl));
            }
            "shot-down" => self.shot_input(|o| o.down(f("x"), f("y"))),
            "shot-move" => self.shot_input(|o| {
                o.moved(f("x"), f("y"), true);
                Out::None
            }),
            "shot-up" => self.shot_input(|o| {
                o.up(f("x"), f("y"));
                Out::None
            }),
            "shot-char" => {
                for c in data.get("c").and_then(|v| v.as_str()).unwrap_or_default().chars() {
                    self.shot_input(|o| {
                        o.char(c);
                        Out::None
                    });
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn window_message(_hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
    match msg {
        WM_APP_SHOT => {
            with_app(|a| a.shot_start());
        }
        WM_APP_SHOT_KEY => {
            let (vk, ctrl) = (wp.0 as u16, lp.0 != 0);
            with_app(|a| a.shot_input(|o| o.key(vk, ctrl)));
        }
        WM_APP_SHOT_CHAR => {
            if let Some(c) = char::from_u32(wp.0 as u32) {
                with_app(|a| a.shot_input(|o| {
                    o.char(c);
                    Out::None
                }));
            }
        }
        _ => return None,
    }
    Some(LRESULT(0))
}
