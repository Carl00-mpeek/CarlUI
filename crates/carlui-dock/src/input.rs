//! Bu modül global girdi takibini arayüzden ayrı, kendi iş parçacığında
//! çözüyor ve iki işi yapar:
//! - **Ekran kenarı:** Otomatik gizlemede imleç dock'un altındaki kenara
//!   değince dock'a haber verir (yalnızca durum değiştiğinde mesaj atılır).
//! - **Win+1…9** (değiştirici ayarlanabilir, ör. Ctrl+Alt+1…9): Sabitlenmiş/açık
//!   uygulamaları açar. Rakam yutulduğu için değiştirici bırakılınca Başlat menüsü
//!   ya da menü çubuğu açılmasın diye araya atanmamış bir tuş (0xE8) eklenir; bu,
//!   Windows'ta yaygın kullanılan zararsız bir maskeleme tuşudur.
//! - **Windows tuşu tek başına:** Başlat menüsü yerine CarlUI'nin Win menüsü
//!   ya da üstteki adanın araması açılır (ayar: `shortcuts.win_key`). Win bırakılırken araya başka tuş girmediyse gerçek "bırakma"
//!   yutulur, yerine maskeleme tuşu + yapay bir "Win bırak" gönderilir; böylece
//!   Windows tuşu takılı kalmaz ve Başlat menüsü açılmaz. Tam ekranda yapılmaz.
//! - **Dönüştürücü (Win+Alt, ayarlanabilir):** Yalnızca değiştiricilerden
//!   oluşan bir akor. Hepsi basılınca "kurulur" (maskeleme tuşu gönderilir ki
//!   Alt menü çubuğu / Başlat açılmasın), arada başka tuşa basılmadan biri
//!   bırakılınca dock'a haber verilir. Win+Alt+R gibi Windows kısayolları
//!   araya tuş girdiği için tetiklemez.
//!
//! Windows, cevap vermekte geciken düşük seviyeli kancaları sessizce kaldırır.
//! Bekçi iş parçacığı her 2 saniyede bir imlecin hareket edip etmediğine ve
//! kancaya olay gelip gelmediğine bakar; imleç hareket ettiği hâlde olay
//! gelmiyorsa kancaları söküp yeniden kurar.

use carlui_core::{
    info,
    keys::{Combo, Mods},
    warn,
};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, AtomicU64, AtomicU8, Ordering},
        Arc,
    },
    time::Duration,
};
use windows::Win32::{
    Foundation::*,
    System::{LibraryLoader::GetModuleHandleW, SystemInformation::GetTickCount, Threading::GetCurrentThreadId},
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

pub const WM_APP_EDGE: u32 = WM_APP + 20;
pub const WM_APP_WINKEY: u32 = WM_APP + 21;
pub const WM_APP_CONVERT: u32 = WM_APP + 23;
const WM_APP_REHOOK: u32 = WM_APP + 22;
const WM_APP_RELOAD: u32 = WM_APP + 24;
const MASK_KEY: u16 = 0xE8;

static DOCK: AtomicIsize = AtomicIsize::new(0);
static SPAN_L: AtomicI32 = AtomicI32::new(0);
static SPAN_R: AtomicI32 = AtomicI32::new(0);
static EDGE_Y: AtomicI32 = AtomicI32::new(i32::MAX);
static WANT_EDGE: AtomicBool = AtomicBool::new(false);
static WANT_KEYS: AtomicBool = AtomicBool::new(false);
/// Uygulama numaraları için gereken değiştiriciler (bit: 1 Win, 2 Ctrl, 4 Alt, 8 Shift); 0 = kapalı.
static DIGIT_MODS: AtomicU8 = AtomicU8::new(0);
/// Windows tuşu tek başına: 0 = Windows'a bırak, 1 = arama, 2 = Win menüsü.
static WANT_START: AtomicU8 = AtomicU8::new(0);
/// Pano kısayolu Win içeriyorsa (Win+V gibi): değiştirici maskesi << 16 | tuş; 0 = yok.
/// Windows bu kısayolları kendine ayırdığı için ada RegisterHotKey ile alamaz, kanca yakalar.
static CLIP_COMBO: AtomicU32 = AtomicU32::new(0);
static FULLSCREEN: AtomicBool = AtomicBool::new(false);
/// Özel (D3D exclusive) tam ekran oyun: Win tuşu Windows'a bırakılır; menümüz
/// oyunun üstünde görünemez. Video/çerçevesiz tam ekranda menü yine açılır.
static EXCLUSIVE: AtomicBool = AtomicBool::new(false);

/// Özel tam ekranın bittiği an (GetTickCount, ms).
static EXCLUSIVE_OFF_AT: AtomicU32 = AtomicU32::new(0);
/// Özel tam ekran bittikten sonra Win tuşu bu kadar süre daha Windows'a bırakılır:
/// oyun bir anlığına odağı kaybedip (geçiş yüzeyi, başka monitördeki pencere)
/// tam ekran algısı kısa süre "yok" deyince menümüz oyunun üstünde açılıp onu düşürmesin.
const EXCLUSIVE_GRACE_MS: u32 = 2000;

pub fn set_exclusive(on: bool) {
    if EXCLUSIVE.swap(on, Ordering::Relaxed) && !on {
        EXCLUSIVE_OFF_AT.store(unsafe { GetTickCount() }, Ordering::Relaxed);
    }
}

/// Özel tam ekran var mı ya da az önce mi bitti?
fn exclusive_recent() -> bool {
    EXCLUSIVE.load(Ordering::Relaxed) || unsafe { GetTickCount() }.wrapping_sub(EXCLUSIVE_OFF_AT.load(Ordering::Relaxed)) < EXCLUSIVE_GRACE_MS
}
/// Dönüştürücü akoru (DIGIT_MODS ile aynı bit düzeni); 0 = kapalı.
static CONV_MODS: AtomicU8 = AtomicU8::new(0);
/// Akor tamamlandı ve henüz araya başka tuş girmedi.
static CONV_ARMED: AtomicBool = AtomicBool::new(false);
/// Win basılıyken başka bir tuşa basıldı mı (Win+E, Win+Shift+S …)?
static WIN_USED: AtomicBool = AtomicBool::new(false);
static AT_EDGE: AtomicBool = AtomicBool::new(false);
static LAST_EVENT: AtomicU64 = AtomicU64::new(0);
static WIN_DOWN: AtomicBool = AtomicBool::new(false);
/// Klavye kancasının gördüğü son tuş olayının zamanı (GetTickCount).
static LAST_KEY: AtomicU32 = AtomicU32::new(0);
static SWALLOWED: AtomicBool = AtomicBool::new(false);
static STALL: AtomicBool = AtomicBool::new(false);
static INPUT_TID: AtomicU32 = AtomicU32::new(0);
/// Test ve tanı için: kancalar kaç kez yeniden kuruldu.
pub static REHOOKS: AtomicU64 = AtomicU64::new(0);

pub struct Input {
    tid: u32,
    stop: Arc<AtomicBool>,
}

/// Kenar: 0 alt, 1 sol, 2 sağ (yan dock'ta aralık dikeydir, EDGE_Y kenarın x'i).
static EDGE_SIDE: AtomicU8 = AtomicU8::new(0);

/// Dock'un kenardaki aralığı değişince güncellenir: alt dock'ta (sol, sağ, alt
/// kenarın y'si), yan dock'ta (üst, alt, kenarın x'i).
pub fn set_span(l: i32, r: i32, edge: i32, side: carlui_core::config::DockPosition) {
    use carlui_core::config::DockPosition;
    SPAN_L.store(l, Ordering::SeqCst);
    SPAN_R.store(r, Ordering::SeqCst);
    EDGE_Y.store(edge, Ordering::SeqCst);
    EDGE_SIDE.store(
        match side {
            DockPosition::Bottom => 0,
            DockPosition::Left => 1,
            DockPosition::Right => 2,
        },
        Ordering::SeqCst,
    );
}

fn post(msg: u32, wp: usize) {
    unsafe {
        let _ = PostMessageW(HWND(DOCK.load(Ordering::SeqCst) as _), msg, WPARAM(wp), LPARAM(0));
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 {
        if STALL.swap(false, Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(1500));
        }
        LAST_EVENT.store(GetTickCount() as u64, Ordering::Relaxed);
        if WANT_EDGE.load(Ordering::Relaxed) {
            let info = &*(lp.0 as *const MSLLHOOKSTRUCT);
            let slack = 24;
            let edge = EDGE_Y.load(Ordering::Relaxed);
            let (along, at_edge) = match EDGE_SIDE.load(Ordering::Relaxed) {
                0 => (info.pt.x, info.pt.y >= edge),
                // Yan kenarın ötesinde başka bir monitör olabilir: yalnızca kenar pikselleri.
                1 => (info.pt.y, info.pt.x <= edge && info.pt.x >= edge - 1),
                _ => (info.pt.y, info.pt.x >= edge && info.pt.x <= edge + 1),
            };
            let at = at_edge && along >= SPAN_L.load(Ordering::Relaxed) - slack && along <= SPAN_R.load(Ordering::Relaxed) + slack;
            if AT_EDGE.swap(at, Ordering::Relaxed) != at {
                post(WM_APP_EDGE, at as usize);
            }
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

fn send_mask_key() {
    let key = |up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: VIRTUAL_KEY(MASK_KEY), dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, ..Default::default() },
        },
    };
    unsafe {
        SendInput(&[key(false), key(true)], std::mem::size_of::<INPUT>() as i32);
    }
}

/// Yutulan "Win bırak" yerine: maskeleme tuşu + yapay Win bırakma.
fn release_win_quietly(win: u16) {
    let key = |vk: u16, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) } | if vk == win { KEYEVENTF_EXTENDEDKEY } else { KEYBD_EVENT_FLAGS(0) },
                ..Default::default()
            },
        },
    };
    unsafe {
        SendInput(&[key(MASK_KEY, false), key(MASK_KEY, true), key(win, true)], std::mem::size_of::<INPUT>() as i32);
    }
}

/// Tam ekran uygulama/oyun açıkken Windows tuşuna dokunma. Dock o sırada
/// gizli olduğundan fare kancası da sökülür: yüksek hızlı oyun faresinin her
/// olayı dock'u uyandırmasın. Çıkışta kancalar baştan kurulur (oyunda düşmüş
/// bir klavye kancası da böylece onarılır).
pub fn set_fullscreen(on: bool) {
    if FULLSCREEN.swap(on, Ordering::SeqCst) != on {
        let tid = INPUT_TID.load(Ordering::SeqCst);
        if tid != 0 {
            unsafe {
                let _ = PostThreadMessageW(tid, WM_APP_RELOAD, WPARAM(0), LPARAM(0));
            }
        }
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 {
        LAST_EVENT.store(GetTickCount() as u64, Ordering::Relaxed);
        LAST_KEY.store(GetTickCount(), Ordering::Relaxed);
        let info = &*(lp.0 as *const KBDLLHOOKSTRUCT);
        let injected = info.flags.0 & LLKHF_INJECTED.0 != 0;
        let down = wp.0 as u32 == WM_KEYDOWN || wp.0 as u32 == WM_SYSKEYDOWN;
        let up = wp.0 as u32 == WM_KEYUP || wp.0 as u32 == WM_SYSKEYUP;
        let vk = info.vkCode;
        let is_win = vk == VK_LWIN.0 as u32 || vk == VK_RWIN.0 as u32;
        // Yalnızca kendi maskeleme tuşumuzu ve kendi gönderdiğimiz Win'i yok say;
        // AutoHotkey/uzak masaüstü gibi araçların gönderdiği Win+N de çalışsın.
        if !(injected && (vk == MASK_KEY as u32 || is_win)) {
            // Win bırakması kaçtıysa (kanca yeniden kurulurken, yönetici penceresi
            // öndeyken) WIN_DOWN takılı kalır; arada yazılan her tuş Win'i "kullanılmış"
            // sayar ve sonraki Win basışı menüyü açmaz. GetAsyncKeyState bu olaydan
            // önceki gerçek durumu verir: Win aslında basılı değilse bayrağı düzelt.
            let win_held = || [VK_LWIN, VK_RWIN].iter().any(|k| GetAsyncKeyState(k.0 as i32) as u16 & 0x8000 != 0);
            if WIN_DOWN.load(Ordering::Relaxed) && down && !win_held() {
                WIN_DOWN.store(false, Ordering::Relaxed);
            }
            if is_win {
                if down && !WIN_DOWN.load(Ordering::Relaxed) {
                    WIN_USED.store(false, Ordering::Relaxed);
                }
                WIN_DOWN.store(down, Ordering::Relaxed);
                if up && SWALLOWED.swap(false, Ordering::Relaxed) {
                    send_mask_key();
                } else if up && WANT_START.load(Ordering::Relaxed) != 0 && !exclusive_recent() && !WIN_USED.load(Ordering::Relaxed) {
                    let modifier = [VK_SHIFT, VK_CONTROL, VK_MENU].iter().any(|k| GetAsyncKeyState(k.0 as i32) as u16 & 0x8000 != 0);
                    // Menü çalışmıyorsa aramaya, o da yoksa Windows'un Başlat'ına düşer.
                    let opened = || match WANT_START.load(Ordering::Relaxed) {
                        2 => carlui_core::win::toggle_start() || carlui_core::win::toggle_search(),
                        _ => carlui_core::win::toggle_search(),
                    };
                    if !modifier && opened() {
                        release_win_quietly(vk as u16);
                        return LRESULT(1);
                    }
                }
            } else if down && WIN_DOWN.load(Ordering::Relaxed) {
                WIN_USED.store(true, Ordering::Relaxed);
            }
            let held = |k: VIRTUAL_KEY| GetAsyncKeyState(k.0 as i32) as u16 & 0x8000 != 0;
            let now = (WIN_DOWN.load(Ordering::Relaxed) as u8) | (held(VK_CONTROL) as u8) << 1 | (held(VK_MENU) as u8) << 2 | (held(VK_SHIFT) as u8) << 3;
            let conv = CONV_MODS.load(Ordering::Relaxed);
            if conv != 0 && !FULLSCREEN.load(Ordering::Relaxed) {
                // GetAsyncKeyState bu olaydan önceki durumu verir; olayın kendisini ekle.
                let bit = if is_win {
                    1
                } else if vk == VK_CONTROL.0 as u32 || vk == VK_LCONTROL.0 as u32 || vk == VK_RCONTROL.0 as u32 {
                    2
                } else if vk == VK_MENU.0 as u32 || vk == VK_LMENU.0 as u32 || vk == VK_RMENU.0 as u32 {
                    4
                } else if vk == VK_SHIFT.0 as u32 || vk == VK_LSHIFT.0 as u32 || vk == VK_RSHIFT.0 as u32 {
                    8
                } else {
                    0
                };
                // WIN_DOWN yukarıda bu olayla güncellendi; bırakmada önceki hâli geri koy.
                let before = if is_win && up { now | 1 } else { now };
                let after = if bit == 0 { before } else if down { before | bit } else { before & !bit };
                if bit == 0 {
                    if down {
                        CONV_ARMED.store(false, Ordering::Relaxed);
                    }
                } else if down && after == conv {
                    if !CONV_ARMED.swap(true, Ordering::Relaxed) {
                        send_mask_key();
                    }
                } else if up && CONV_ARMED.swap(false, Ordering::Relaxed) && before == conv {
                    post(WM_APP_CONVERT, 0);
                } else if down && after & !conv != 0 {
                    CONV_ARMED.store(false, Ordering::Relaxed);
                }
            }
            let clip = CLIP_COMBO.load(Ordering::Relaxed);
            // Tam ekranda da yakalanır: Windows'un kendi Win+V paneli oyunu küçültür,
            // ada ise paneli oyunun üstünde odak almadan açar.
            if !is_win && clip != 0 && vk == clip & 0xFFFF && now as u32 == clip >> 16 {
                if down {
                    SWALLOWED.store(true, Ordering::Relaxed);
                    send_mask_key();
                    carlui_core::win::toggle_clip();
                }
                if down || up {
                    return LRESULT(1);
                }
            }
            let want = DIGIT_MODS.load(Ordering::Relaxed);
            if !is_win && want != 0 && (0x31..=0x39).contains(&vk) {
                if now == want {
                    if down {
                        SWALLOWED.store(true, Ordering::Relaxed);
                        // Değiştirici "kullanıldı" sayılsın: Alt menü çubuğu / Başlat açılmasın.
                        send_mask_key();
                        post(WM_APP_WINKEY, (vk - 0x30) as usize);
                    }
                    if down || up {
                        return LRESULT(1);
                    }
                }
            }
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

fn install() -> Vec<HHOOK> {
    let mut hooks = Vec::new();
    unsafe {
        let module = GetModuleHandleW(None).unwrap_or_default();
        // Fare kancası (tam ekran dışında) her zaman kurulur: bekçi onun nabzıyla kancaların canlılığını ölçer.
        if !FULLSCREEN.load(Ordering::SeqCst) {
            match SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0) {
                Ok(h) => hooks.push(h),
                Err(e) => warn!("fare kancası kurulamadı: {e}"),
            }
        }
        if WANT_KEYS.load(Ordering::SeqCst) {
            match SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0) {
                Ok(h) => hooks.push(h),
                Err(e) => warn!("klavye kancası kurulamadı: {e}"),
            }
        }
    }
    LAST_EVENT.store(unsafe { GetTickCount() as u64 }, Ordering::SeqCst);
    hooks
}

impl Input {
    /// `digits`: uygulama numaraları değiştiricisi (Win+1…9 gibi); `start`: Windows tuşu tek başına
    /// ne açar (0 Windows'unki, 1 arama, 2 Win menüsü).
    /// `clip`: pano kısayolu; yalnızca Win içerenler burada yakalanır.
    /// `convert`: dönüştürücü akoru (yalnızca değiştiriciler, ör. Win+Alt).
    pub fn start(dock: HWND, edge: bool, digits: Option<Mods>, start: u8, clip: Option<Combo>, convert: Option<Mods>) -> Option<Self> {
        let bits = |m: Mods| m.win as u8 | (m.ctrl as u8) << 1 | (m.alt as u8) << 2 | (m.shift as u8) << 3;
        let mask = digits.map(bits).unwrap_or(0);
        let clip = clip.filter(|c| c.mods.win).map(|c| (bits(c.mods) as u32) << 16 | c.vk).unwrap_or(0);
        CLIP_COMBO.store(clip, Ordering::SeqCst);
        let conv = convert.map(bits).unwrap_or(0);
        CONV_MODS.store(conv, Ordering::SeqCst);
        CONV_ARMED.store(false, Ordering::SeqCst);
        let keys = mask != 0 || start != 0 || clip != 0 || conv != 0;
        if !edge && !keys {
            return None;
        }
        DOCK.store(dock.0 as isize, Ordering::SeqCst);
        WANT_EDGE.store(edge, Ordering::SeqCst);
        WANT_KEYS.store(keys, Ordering::SeqCst);
        DIGIT_MODS.store(mask, Ordering::SeqCst);
        WANT_START.store(start, Ordering::SeqCst);
        AT_EDGE.store(false, Ordering::SeqCst);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("input".into())
            .spawn(move || unsafe {
                let mut hooks = install();
                let _ = tx.send(GetCurrentThreadId());
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    if msg.message == WM_APP_REHOOK || msg.message == WM_APP_RELOAD {
                        for h in hooks.drain(..) {
                            let _ = UnhookWindowsHookEx(h);
                        }
                        AT_EDGE.store(false, Ordering::SeqCst);
                        hooks = install();
                        if msg.message == WM_APP_REHOOK {
                            REHOOKS.fetch_add(1, Ordering::SeqCst);
                            warn!("girdi kancaları düşmüştü, yeniden kuruldu");
                        } else {
                            info!("tam ekran: {}, fare kancası {}", FULLSCREEN.load(Ordering::SeqCst), if FULLSCREEN.load(Ordering::SeqCst) { "söküldü" } else { "kuruldu" });
                        }
                    }
                }
                for h in hooks {
                    let _ = UnhookWindowsHookEx(h);
                }
            })
            .ok()?;
        let tid = rx.recv().ok()?;
        INPUT_TID.store(tid, Ordering::SeqCst);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_w = stop.clone();
        std::thread::Builder::new()
            .name("input-watchdog".into())
            .spawn(move || {
                let mut last = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut last);
                }
                while !stop_w.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_secs(2));
                    let mut now = POINT::default();
                    unsafe {
                        let _ = GetCursorPos(&mut now);
                    }
                    let moved = now.x != last.x || now.y != last.y;
                    last = now;
                    // Yalnızca gerçek girdi (donanım/SendInput) sayılır: imleci SetCursorPos ile
                    // taşıyan programlar (oyunlar, uzak masaüstü) yanlış alarm üretmesin.
                    let mut lii = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
                    let real_input = unsafe { GetLastInputInfo(&mut lii).as_bool() };
                    let hook_last = LAST_EVENT.load(Ordering::SeqCst) as u32;
                    let input_after_hook = real_input && (lii.dwTime.wrapping_sub(hook_last) as i32) > 2000;
                    // Tam ekranda fare kancası yok: nabız ölçülemez, çıkışta zaten yeniden kurulur.
                    if moved && input_after_hook && !FULLSCREEN.load(Ordering::SeqCst) {
                        unsafe {
                            let _ = PostThreadMessageW(tid, WM_APP_REHOOK, WPARAM(0), LPARAM(0));
                        }
                    }
                }
            })
            .ok();
        info!("girdi takibi başladı (kenar: {edge}, uygulama numaraları: {digits:?}, Win tuşu: {start}, dönüştürücü: {convert:?})");
        Some(Self { tid, stop })
    }

    /// Test için: fare kancasını bir kez Windows'un zaman aşımından uzun beklet.
    /// Windows kancayı kaldırır; bekçinin bunu fark edip yeniden kurması beklenir.
    pub fn stall_once(&self) {
        STALL.store(true, Ordering::SeqCst);
    }
}

impl Drop for Input {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = INPUT_TID.compare_exchange(self.tid, 0, Ordering::SeqCst, Ordering::SeqCst);
        unsafe {
            let _ = PostThreadMessageW(self.tid, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

/// Windows'un Başlat'ı az önce öne geldi: bizim menümüzle değiştirilmeli mi?
/// Yalnızca Win tuşu menümüze bağlıysa, tam ekran (oyun) yokken ve klavye
/// kancası son `ms` içinde hiçbir tuş görmediyse (kanca kör kaldıysa: yönetici
/// penceresi öndeyken Windows tuşları ona hiç göstermez). Kanca tuşu gördüyse
/// Başlat'ı kullanıcı bilerek açmıştır (Ctrl+Esc, Shift+Win …): dokunulmaz.
pub fn start_redirect_ok(ms: u32) -> bool {
    let quiet = unsafe { GetTickCount() }.wrapping_sub(LAST_KEY.load(Ordering::Relaxed)) > ms;
    WANT_START.load(Ordering::Relaxed) == 2 && WANT_KEYS.load(Ordering::Relaxed) && !FULLSCREEN.load(Ordering::Relaxed) && !exclusive_recent() && quiet
}
