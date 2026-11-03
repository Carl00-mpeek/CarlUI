//! Bu modül "arka planda çalışan uygulamaların tepsi ikonlarını nasıl alırız?"
//! sorusunu çözüyor.
//!
//! Uygulamalar ikonlarını `Shell_NotifyIcon` ile bildirir; Windows bunu
//! `Shell_TrayWnd` sınıfındaki ilk üst düzey pencereye `WM_COPYDATA` olarak
//! gönderir. Biz bu sınıfta gizli bir pencere açıp z-sırasında explorer'ınkinin
//! üstünde tutarız; böylece mesajlar önce bize gelir. Her mesajı okuduktan sonra
//! aynen explorer'ın gerçek tepsisine iletiriz: explorer'ın durumu hep tam kalır,
//! bu modül kapanır ya da çökerse hiçbir ikon kaybolmaz. Açılışta
//! "TaskbarCreated" yayınıyla uygulamalardan ikonlarını yeniden istemeleri
//! sağlanır.
//!
//! Tıklamalar, uygulamanın istediği bildirim sürümüne göre (v4 ya da eski)
//! uygulamanın kendi geri çağırma mesajıyla iletilir.

use carlui_core::{info, win};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{DataExchange::COPYDATASTRUCT, Threading::*},
        UI::WindowsAndMessaging::*,
    },
};

const NIM_ADD: u32 = 0;
const NIM_MODIFY: u32 = 1;
const NIM_DELETE: u32 = 2;
const NIM_SETVERSION: u32 = 4;
const NIF_MESSAGE: u32 = 0x01;
const NIF_ICON: u32 = 0x02;
const NIF_TIP: u32 = 0x04;
const NIF_STATE: u32 = 0x08;
const NIF_INFO: u32 = 0x10;
const NIF_GUID: u32 = 0x20;
const NIS_HIDDEN: u32 = 0x01;
const TRAY_SIGNATURE: u32 = 0x3475_3423;
const NIN_SELECT: u32 = WM_USER;

/// Shell'in WM_COPYDATA ile gönderdiği yapı; tutamaçlar her mimaride 32 bittir.
#[repr(C)]
#[derive(Clone, Copy)]
struct NotifyIconData32 {
    cb_size: u32,
    hwnd: u32,
    uid: u32,
    flags: u32,
    callback: u32,
    hicon: u32,
    tip: [u16; 128],
    state: u32,
    state_mask: u32,
    info: [u16; 256],
    version_or_timeout: u32,
    info_title: [u16; 64],
    info_flags: u32,
    guid: GUID,
    balloon_icon: u32,
}

#[repr(C)]
struct ShellTrayData {
    signature: u32,
    message: u32,
    nid: NotifyIconData32,
}

/// Shell_NotifyIconGetRect'in gönderdiği yapı.
#[repr(C)]
struct IconIdentifier32 {
    signature: u32,
    message: u32,
    cb_size: u32,
    hwnd: u32,
    uid: u32,
    guid: GUID,
}

#[derive(Clone, Debug)]
pub struct TrayIcon {
    pub hwnd: HWND,
    pub uid: u32,
    pub guid: Option<GUID>,
    pub callback: u32,
    pub hicon: HICON,
    pub tip: String,
    pub version: u32,
    pub hidden: bool,
    pub pid: u32,
    /// Panelde görünen kısa ad (uygulamanın dosya açıklaması ya da ipucunun ilk satırı).
    pub name: String,
    /// İkon değişti; panel yeniden çizmeli.
    pub icon_dirty: bool,
}

/// Uygulamanın gösterdiği balon bildirimi (bildirim modülüne iletilir).
pub struct Balloon {
    pub title: String,
    pub text: String,
    pub app: String,
}

pub enum Change {
    None,
    /// İkon listesi ya da bir ikonun görüntüsü değişti.
    Icons,
    Balloon(Balloon),
}

pub struct Host {
    pub hwnd: HWND,
    pub icons: Vec<TrayIcon>,
    /// Shell_NotifyIconGetRect sorularına verilecek dikdörtgen (dock'taki tray düğmesi).
    pub anchor: RECT,
}

/// Üst düzey pencereler ve yalnızca-mesaj pencereleri (tepsi ikonu sahipleri çoğu zaman bunlardır).
fn all_windows() -> Vec<HWND> {
    unsafe extern "system" fn cb(h: HWND, l: LPARAM) -> BOOL {
        (*(l.0 as *mut Vec<HWND>)).push(h);
        TRUE
    }
    let mut v: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut v as *mut _ as isize));
        let mut after = HWND::default();
        while let Ok(h) = FindWindowExW(HWND_MESSAGE, after, PCWSTR::null(), PCWSTR::null()) {
            if h.is_invalid() {
                break;
            }
            v.push(h);
            after = h;
        }
    }
    v
}

fn wstr(buf: &[u16]) -> String {
    String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())])
}

fn exe_description(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return None;
        }
        file_description(&String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// Exe'nin sürüm bilgisindeki açıklaması ("Discord" gibi); yoksa dosya adı.
pub fn file_description(exe: &str) -> Option<String> {
    use windows::Win32::Storage::FileSystem::*;
    unsafe {
        let path = HSTRING::from(exe);
        let size = GetFileVersionInfoSizeW(&path, None);
        let stem = std::path::Path::new(&path.to_string_lossy()).file_stem().map(|s| s.to_string_lossy().to_string());
        if size == 0 {
            return stem;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(&path, 0, size, data.as_mut_ptr() as _).ok()?;
        let mut ptr = std::ptr::null_mut();
        let mut n = 0u32;
        if !VerQueryValueW(data.as_ptr() as _, w!("\\VarFileInfo\\Translation"), &mut ptr, &mut n).as_bool() || n < 4 {
            return stem;
        }
        let (lang, cp) = (*(ptr as *const u16), *(ptr as *const u16).add(1));
        let key = HSTRING::from(format!("\\StringFileInfo\\{lang:04x}{cp:04x}\\FileDescription"));
        if !VerQueryValueW(data.as_ptr() as _, &key, &mut ptr, &mut n).as_bool() || n == 0 {
            return stem;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(ptr as *const u16, n as usize));
        let s = s.trim_end_matches('\0').trim().to_string();
        if s.is_empty() { stem } else { Some(s) }
    }
}

fn short_name(desc: Option<String>, tip: &str) -> String {
    // Sürüm açıklamaları bazen uzun ("NVIDIA Container", "Steam Client Bootstrapper").
    let base = desc.unwrap_or_else(|| tip.lines().next().unwrap_or_default().to_string());
    let base = base.replace(" Client Bootstrapper", "").replace(" Bootstrapper", "");
    base.split(" - ").next().unwrap_or_default().trim().to_string()
}

impl Host {
    pub fn new(hwnd: HWND) -> Self {
        Self { hwnd, icons: Vec::new(), anchor: RECT::default() }
    }

    /// Uygulamalardan ikonlarını yeniden göndermelerini iste.
    pub fn request_icons(&self) {
        unsafe {
            // Uygulamalar cevabı ilk bulunan Shell_TrayWnd'ye gönderir; o an biz olmalıyız.
            self.keep_on_top();
            let first = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()).unwrap_or_default();
            if first != self.hwnd {
                carlui_core::warn!("ikon isteği: ilk Shell_TrayWnd biz değiliz ({:?})", first.0);
            }
            // HWND_BROADCAST bazı uygulamalara (IDM, ISLC) ulaşmıyor; her pencereye tek tek gönder.
            let msg = win::registered_message("TaskbarCreated");
            let me = GetCurrentProcessId();
            let mut n = 0;
            for h in all_windows() {
                let mut pid = 0;
                GetWindowThreadProcessId(h, Some(&mut pid));
                if pid != me && PostMessageW(h, msg, WPARAM(0), LPARAM(0)).is_ok() {
                    n += 1;
                }
            }
            info!("ikon isteği {n} pencereye gönderildi");
        }
    }

    /// Penceremizi explorer'ın tepsisinin üstünde tut: Windows mesajı ilk bulduğu pencereye gönderir.
    pub fn keep_on_top(&self) {
        unsafe {
            let _ = SetWindowPos(self.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }

    fn find(&self, n: &NotifyIconData32) -> Option<usize> {
        if n.flags & NIF_GUID != 0 {
            if let Some(i) = self.icons.iter().position(|i| i.guid == Some(n.guid)) {
                return Some(i);
            }
        }
        self.icons.iter().position(|i| i.hwnd.0 as usize as u32 == n.hwnd && i.uid == n.uid)
    }

    /// Ölmüş pencerelere ait ikonları temizle (çöken uygulamalar ikon bırakır).
    pub fn prune(&mut self) -> bool {
        let before = self.icons.len();
        self.icons.retain(|i| unsafe { IsWindow(i.hwnd).as_bool() });
        before != self.icons.len()
    }

    /// WM_COPYDATA: ikon mesajını işler. Explorer'a iletmeyi çağıran yapar (bkz.
    /// `forward_copydata`): iletme yüzlerce ms sürebilir ve bu sırada gelen başka
    /// uygulamaların mesajları iç içe işlenir; o an uygulama kilitli olursa ikonları
    /// kaybolurdu. Dönüş: (kendi sonucumuz, değişiklik, explorer'a da iletilsin mi).
    pub fn copydata(&mut self, lp: LPARAM) -> (Option<LRESULT>, Change, bool) {
        let cds = unsafe { &*(lp.0 as *const COPYDATASTRUCT) };
        match cds.dwData {
            1 if cds.cbData as usize >= std::mem::size_of::<u32>() * 2 + 24 => {
                let data = unsafe { &*(cds.lpData as *const ShellTrayData) };
                if data.signature == TRAY_SIGNATURE {
                    let (ok, c) = self.notify(data.message, &data.nid, cds.cbData as usize);
                    return (Some(LRESULT(ok as isize)), c, true);
                }
                (None, Change::None, true)
            }
            3 if cds.cbData as usize >= std::mem::size_of::<IconIdentifier32>() => {
                // Shell_NotifyIconGetRect: uygulama ikonunun yerini soruyor (açılır pencereyi
                // konumlandırmak için). Dock'taki tray düğmesini gösteriyoruz.
                let id = unsafe { &*(cds.lpData as *const IconIdentifier32) };
                let r = self.anchor;
                let v = if id.message == 1 { (r.left & 0xFFFF) | (r.top << 16) } else { (r.right & 0xFFFF) | (r.bottom << 16) };
                (Some(LRESULT(v as isize)), Change::None, false)
            }
            _ => (None, Change::None, true),
        }
    }

    /// Meşgulken gelen ikon mesajı: kopyasını saklar, sonra `copydata` ile işlenir.
    pub fn copy_of(lp: LPARAM) -> (usize, Vec<u8>) {
        let cds = unsafe { &*(lp.0 as *const COPYDATASTRUCT) };
        let bytes = if cds.lpData.is_null() { Vec::new() } else { unsafe { std::slice::from_raw_parts(cds.lpData as *const u8, cds.cbData as usize) }.to_vec() };
        (cds.dwData, bytes)
    }

    /// `copy_of` ile saklanan mesajı işler.
    pub fn copydata_saved(&mut self, data: &(usize, Vec<u8>)) -> Change {
        let cds = COPYDATASTRUCT { dwData: data.0, cbData: data.1.len() as u32, lpData: data.1.as_ptr() as *mut _ };
        self.copydata(LPARAM(&cds as *const _ as isize)).1
    }

    /// İkon mesajını explorer'ın tepsisine iletir (uygulama kilidi dışında çağrılır).
    pub fn forward_copydata(hwnd: HWND, wp: WPARAM, lp: LPARAM) -> LRESULT {
        let cds = unsafe { &*(lp.0 as *const COPYDATASTRUCT) };
        let t = std::time::Instant::now();
        let r = Host::new(hwnd).forward(WM_COPYDATA, wp, lp);
        let ms = t.elapsed().as_millis();
        if ms > 150 {
            carlui_core::warn!("explorer'a iletme yavaş: {ms} ms (dwData {})", cds.dwData);
        }
        r
    }

    /// Mesajı explorer'ın gerçek tepsisine iletir.
    pub fn forward(&self, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        let Some(real) = win::explorer_tray().filter(|&h| h != self.hwnd) else { return LRESULT(0) };
        let mut res = 0usize;
        unsafe {
            let _ = SendMessageTimeoutW(real, msg, wp, lp, SMTO_ABORTIFHUNG, 2000, Some(&mut res));
        }
        LRESULT(res as isize)
    }

    fn notify(&mut self, msg: u32, n: &NotifyIconData32, size: usize) -> (bool, Change) {
        let hwnd = HWND(n.hwnd as usize as _);
        // Eski uygulamalar daha kısa yapı gönderir; alanları boyuta göre oku.
        let has = |offset: usize| size >= 8 + offset;
        let guid_ok = n.flags & NIF_GUID != 0 && has(std::mem::offset_of!(NotifyIconData32, guid) + 16);
        match msg {
            NIM_ADD | NIM_MODIFY => {
                // Bilinmeyen bir ikon için gelen MODIFY de eklenir: bazı uygulamalar
                // "TaskbarCreated" sonrasında doğrudan MODIFY gönderir.
                let idx = match self.find(n) {
                    Some(i) => i,
                    None => {
                        let mut pid = 0;
                        unsafe {
                            GetWindowThreadProcessId(hwnd, Some(&mut pid));
                        }
                        info!("yeni ikon: pid {pid}, uid {}, mesaj {msg}", n.uid);
                        self.icons.push(TrayIcon {
                            hwnd,
                            uid: n.uid,
                            guid: guid_ok.then_some(n.guid),
                            callback: 0,
                            hicon: HICON::default(),
                            tip: String::new(),
                            version: 0,
                            hidden: false,
                            pid,
                            name: String::new(),
                            icon_dirty: true,
                        });
                        self.icons.len() - 1
                    }
                };
                let icon = &mut self.icons[idx];
                if n.flags & NIF_MESSAGE != 0 {
                    icon.callback = n.callback;
                }
                if n.flags & NIF_ICON != 0 {
                    icon.hicon = HICON(n.hicon as usize as _);
                    icon.icon_dirty = true;
                }
                if n.flags & NIF_TIP != 0 && has(std::mem::offset_of!(NotifyIconData32, tip) + 2) {
                    icon.tip = wstr(&n.tip);
                }
                if n.flags & NIF_STATE != 0 && has(std::mem::offset_of!(NotifyIconData32, state_mask) + 4) {
                    if n.state_mask & NIS_HIDDEN != 0 {
                        icon.hidden = n.state & NIS_HIDDEN != 0;
                    }
                }
                if icon.name.is_empty() || n.flags & NIF_TIP != 0 {
                    icon.name = short_name(exe_description(icon.pid), &icon.tip);
                }
                let balloon = (n.flags & NIF_INFO != 0 && has(std::mem::offset_of!(NotifyIconData32, info_title) + 2))
                    .then(|| Balloon { title: wstr(&n.info_title), text: wstr(&n.info), app: icon.name.clone() })
                    .filter(|b| !b.text.is_empty());
                (true, balloon.map(Change::Balloon).unwrap_or(Change::Icons))
            }
            NIM_DELETE => match self.find(n) {
                Some(i) => {
                    self.icons.remove(i);
                    (true, Change::Icons)
                }
                None => (false, Change::None),
            },
            NIM_SETVERSION => {
                if let Some(i) = self.find(n) {
                    self.icons[i].version = n.version_or_timeout;
                }
                (true, Change::None)
            }
            _ => (true, Change::None),
        }
    }

    /// Bir ikona tıklamayı uygulamanın istediği biçimde iletir.
    pub fn click(&self, i: usize, right: bool, at: (i32, i32)) {
        let Some(icon) = self.icons.get(i) else { return };
        if icon.callback == 0 {
            return;
        }
        unsafe {
            // Uygulama menüsünü/penceresini öne getirebilsin (son girdiyi biz aldık).
            // Süreci bilinmeyen ikonda herkese izin ver (0 geçersiz bir kimlik).
            let _ = AllowSetForegroundWindow(if icon.pid != 0 { icon.pid } else { ASFW_ANY });
            let send = |m: u32| {
                let (wp, lp) = if icon.version >= 4 {
                    let pos = ((at.0 as u32 & 0xFFFF) | ((at.1 as u32 & 0xFFFF) << 16)) as usize;
                    (WPARAM(pos), LPARAM(((m & 0xFFFF) | ((icon.uid & 0xFFFF) << 16)) as isize))
                } else {
                    (WPARAM(icon.uid as usize), LPARAM(m as isize))
                };
                let _ = PostMessageW(icon.hwnd, icon.callback, wp, lp);
            };
            if right {
                send(WM_RBUTTONDOWN);
                send(WM_RBUTTONUP);
                if icon.version >= 4 {
                    send(WM_CONTEXTMENU);
                }
            } else {
                send(WM_LBUTTONDOWN);
                send(WM_LBUTTONUP);
                if icon.version >= 4 {
                    send(NIN_SELECT);
                }
            }
        }
    }

    pub fn log_summary(&self) {
        let names: Vec<&str> = self.icons.iter().filter(|i| !i.hidden).map(|i| i.name.as_str()).collect();
        info!("tepsi ikonları ({}): {}", names.len(), names.join(", "));
    }
}
