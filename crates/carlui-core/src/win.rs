//! Birden çok modülün ihtiyaç duyduğu küçük Win32 yardımcıları.

use windows::{
    core::*,
    Win32::{Foundation::*, Graphics::Gdi::*, UI::WindowsAndMessaging::*},
};

/// Pencereyi öne getirip klavyeyi ona verir. Windows bunu her zaman izin
/// vermez; izin yoksa önce kendimize atanmamış bir tuş göndeririz (son girdi
/// bizden gelmiş sayılır), o da olmazsa öndeki pencerenin girdisine bağlanırız.
pub fn force_foreground(hwnd: HWND) {
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    let key = |up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(0xE8), dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, ..Default::default() } },
    };
    unsafe {
        if SetForegroundWindow(hwnd).as_bool() && GetForegroundWindow() == hwnd {
            let _ = SetFocus(hwnd);
            return;
        }
        SendInput(&[key(false), key(true)], std::mem::size_of::<INPUT>() as i32);
        if !(SetForegroundWindow(hwnd).as_bool() && GetForegroundWindow() == hwnd) {
            let fg = GetForegroundWindow();
            let ft = GetWindowThreadProcessId(fg, None);
            let me = GetCurrentThreadId();
            if ft != 0 && ft != me && AttachThreadInput(me, ft, true).as_bool() {
                let _ = BringWindowToTop(hwnd);
                let _ = SetForegroundWindow(hwnd);
                let _ = AttachThreadInput(me, ft, false);
            }
        }
        if GetForegroundWindow() != hwnd {
            // Son çare: Alt basılıyken Windows öne geçişe izin verir. Alt
            // bırakılmadan araya atanmamış tuş girer ki pencere menüye geçmesin.
            let alt = |up: bool| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VK_MENU, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, ..Default::default() } },
            };
            SendInput(&[alt(false)], std::mem::size_of::<INPUT>() as i32);
            let _ = SetForegroundWindow(hwnd);
            SendInput(&[key(false), key(true), alt(true)], std::mem::size_of::<INPUT>() as i32);
        }
        let _ = SetFocus(hwnd);
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Monitor {
    pub handle: HMONITOR,
    /// Monitörün tamamı (fiziksel piksel).
    pub rect: RECT,
    /// Çalışma alanı (görev çubuğu hariç).
    pub work: RECT,
    pub primary: bool,
}

/// Pencerenin bulunduğu monitörün aygıt adı (`\\.\DISPLAY1`). Süreçler farklı
/// DPI farkındalığında olabildiği için monitörler dikdörtgenle değil adla karşılaştırılır.
pub fn monitor_name(hwnd: HWND) -> String {
    unsafe {
        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFOEXW::default();
        mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if !GetMonitorInfoW(mon, &mut mi as *mut MONITORINFOEXW as *mut MONITORINFO).as_bool() {
            return String::new();
        }
        let n = mi.szDevice.iter().position(|&c| c == 0).unwrap_or(mi.szDevice.len());
        String::from_utf16_lossy(&mi.szDevice[..n])
    }
}

/// Tam ekran olayı bu pencerenin monitöründe mi? (Monitör bildirilmemişse evet.)
pub fn fullscreen_here(hwnd: HWND, monitor: &str) -> bool {
    monitor.is_empty() || monitor_name(hwnd) == monitor
}

pub fn monitors() -> Vec<Monitor> {
    unsafe extern "system" fn cb(h: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let list = &mut *(data.0 as *mut Vec<Monitor>);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(h, &mut info).as_bool() {
            list.push(Monitor {
                handle: h,
                rect: info.rcMonitor,
                work: info.rcWork,
                primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
            });
        }
        TRUE
    }
    let mut list: Vec<Monitor> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(cb), LPARAM(&mut list as *mut _ as isize));
    }
    list
}

pub fn primary_monitor() -> Option<Monitor> {
    let all = monitors();
    all.iter().find(|m| m.primary).copied().or_else(|| all.first().copied())
}

/// Belirli bir sınıf adına sahip tüm üst düzey pencereler.
pub fn top_windows_of_class(class: &str) -> Vec<HWND> {
    let class = HSTRING::from(class);
    let mut out = Vec::new();
    let mut after = HWND::default();
    unsafe {
        while let Ok(h) = FindWindowExW(HWND::default(), after, &class, PCWSTR::null()) {
            if h.is_invalid() {
                break;
            }
            out.push(h);
            after = h;
        }
    }
    out
}

pub fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Pencere explorer.exe sürecine mi ait?
pub fn is_explorer_window(hwnd: HWND) -> bool {
    window_exe(hwnd).ends_with("\\explorer.exe")
}

/// Kullanıcının İndirilenler klasörü (taşınmışsa yeni yeri).
pub fn downloads_dir() -> Option<std::path::PathBuf> {
    use windows::Win32::UI::Shell::{FOLDERID_Downloads, SHGetKnownFolderPath, KNOWN_FOLDER_FLAG};
    unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_Downloads, KNOWN_FOLDER_FLAG(0), None).ok()?;
        let s = p.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as _));
        s.map(std::path::PathBuf::from)
    }
}

/// Pencerenin sürecinin exe yolu, küçük harfle (erişilemezse boş).
pub fn window_exe(hwnd: HWND) -> String {
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    process_exe(pid)
}

/// Sürecin exe yolu (küçük harf); açılamazsa boş.
pub fn process_exe(pid: u32) -> String {
    use windows::Win32::System::Threading::*;
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if ok { String::from_utf16_lossy(&buf[..len as usize]).to_lowercase() } else { String::new() }
    }
}

/// Carl Vault'un konuştuğu tarayıcılar (exe adı, küçük harf).
pub const BROWSERS: [&str; 9] = ["firefox.exe", "chrome.exe", "msedge.exe", "brave.exe", "opera.exe", "vivaldi.exe", "librewolf.exe", "waterfox.exe", "zen.exe"];

/// Exe adı (ör. `chrome.exe`) ya da tam yolu bilinen bir tarayıcının mı.
pub fn is_browser(exe: &str) -> bool {
    let name = exe.rsplit('\\').next().unwrap_or_default().to_ascii_lowercase();
    BROWSERS.contains(&name.as_str())
}

/// Bu makinedeki bir IPv4 TCP bağlantısının istemci tarafındaki süreç.
/// Sunucu (`server_port`) kabul ettiği bağlantının karşı ucunu (`peer`) verir;
/// Windows'un bağlantı tablosunda yerel ucu `peer`, uzak ucu sunucu olan satırın
/// sahibi döner. Bulunamazsa None.
pub fn tcp_client_pid(peer: std::net::SocketAddrV4, server_port: u16) -> Option<u32> {
    use windows::Win32::NetworkManagement::IpHelper::*;
    const AF_INET: u32 = 2;
    // Tablo bayt sırası: adres ağ sırasında, port alt 16 bitte ağ sırasında.
    let addr = u32::from_ne_bytes(peer.ip().octets());
    let port = |p: u16| p.to_be() as u32;
    unsafe {
        let mut size = 0u32;
        let _ = GetExtendedTcpTable(None, &mut size, false, AF_INET, TCP_TABLE_OWNER_PID_CONNECTIONS, 0);
        for _ in 0..4 {
            // u32 hizasında tampon (satırlar u32 alanlı).
            let mut buf = vec![0u32; (size as usize).div_ceil(4) + 16];
            size = (buf.len() * 4) as u32;
            let r = GetExtendedTcpTable(Some(buf.as_mut_ptr() as _), &mut size, false, AF_INET, TCP_TABLE_OWNER_PID_CONNECTIONS, 0);
            if r == ERROR_INSUFFICIENT_BUFFER.0 {
                continue;
            }
            if r != 0 {
                return None;
            }
            let t = &*(buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID);
            let rows = std::slice::from_raw_parts(t.table.as_ptr(), t.dwNumEntries as usize);
            return rows
                .iter()
                .find(|r| r.dwLocalAddr == addr && (r.dwLocalPort & 0xFFFF) == port(peer.port()) && (r.dwRemotePort & 0xFFFF) == port(server_port))
                .map(|r| r.dwOwningPid);
        }
        None
    }
}

/// Tarayıcı / video oynatıcı / sistem uygulaması mı? Bunların tam ekranı (video,
/// sunum) oyun sayılmaz; kısayollar çalışmaya devam eder.
pub fn is_media_window(hwnd: HWND) -> bool {
    const EXES: [&str; 18] = [
        "firefox.exe", "chrome.exe", "msedge.exe", "brave.exe", "opera.exe", "vivaldi.exe", "zen.exe", "librewolf.exe",
        "vlc.exe", "mpv.exe", "mpc-hc64.exe", "mpc-be64.exe", "potplayermini64.exe", "wmplayer.exe", "microsoft.media.player.exe",
        "applicationframehost.exe", "powerpnt.exe", "explorer.exe",
    ];
    let exe = window_exe(hwnd);
    let name = exe.rsplit('\\').next().unwrap_or("");
    EXES.contains(&name) || matches!(class_name(hwnd).as_str(), "MozillaWindowClass" | "Chrome_WidgetWin_1")
}

/// Explorer'ın gerçek `Shell_TrayWnd` penceresi (tray modülününki değil).
pub fn explorer_tray() -> Option<HWND> {
    top_windows_of_class("Shell_TrayWnd").into_iter().find(|&h| is_explorer_window(h))
}

/// Adadaki aramayı aç/kapat isteği (dock'tan: Windows tuşu ya da Başlat/Ara düğmesi).
pub const WM_CARLUI_SEARCH: u32 = WM_APP + 40;
pub const ISLAND_CLASS: PCWSTR = w!("CarlUI.Island");

/// Adaya "aramayı aç/kapat" der. Ada çalışmıyorsa false.
pub fn toggle_search() -> bool {
    post_island(0)
}

/// Adaya "pano geçmişini aç/kapat" der (WM_CARLUI_SEARCH, wParam = 1).
pub fn toggle_clip() -> bool {
    post_island_with(1, 0)
}

/// Aramayı açar (açıksa kapatmaz) ve ilk harfi kutuya yazar: Win menüsünde
/// yazmaya başlayınca harf kaybolmasın (wParam = 2, lParam = UTF-16 karakter).
pub fn search_with(c: u16) -> bool {
    post_island_with(2, c as isize)
}

fn post_island(what: usize) -> bool {
    post_island_with(what, 0)
}

fn post_island_with(what: usize, data: isize) -> bool {
    unsafe {
        match FindWindowW(ISLAND_CLASS, PCWSTR::null()) {
            Ok(h) if !h.is_invalid() => {
                // Bu süreç girdiyi aldığı için adanın öne gelmesine izin verebilir.
                let _ = AllowSetForegroundWindow(ASFW_ANY);
                PostMessageW(h, WM_CARLUI_SEARCH, WPARAM(what), LPARAM(data)).is_ok()
            }
            _ => false,
        }
    }
}

/// Win menüsünü aç/kapat isteği (dock'tan: Windows tuşu ya da Başlat düğmesi).
pub const WM_CARLUI_START: u32 = WM_APP + 41;
pub const START_CLASS: PCWSTR = w!("CarlUI.Start");

/// Win menüsüne "aç/kapat" der. Menü çalışmıyorsa false.
pub fn toggle_start() -> bool {
    unsafe {
        match FindWindowW(START_CLASS, PCWSTR::null()) {
            Ok(h) if !h.is_invalid() => {
                let _ = AllowSetForegroundWindow(ASFW_ANY);
                PostMessageW(h, WM_CARLUI_START, WPARAM(0), LPARAM(0)).is_ok()
            }
            _ => false,
        }
    }
}

/// Dock'a "Win menüsü açık: görün ve yerinde kal" (wParam 1) / "kapandı" (0).
pub const WM_CARLUI_DOCK_HOLD: u32 = WM_APP + 42;

pub fn dock_hold(on: bool) {
    unsafe {
        if let Ok(h) = FindWindowW(w!("CarlUI.Dock"), PCWSTR::null()) {
            if !h.is_invalid() {
                let _ = PostMessageW(h, WM_CARLUI_DOCK_HOLD, WPARAM(on as usize), LPARAM(0));
            }
        }
    }
}

/// Kayıtlı pencere mesajı (ör. "TaskbarCreated").
pub fn registered_message(name: &str) -> u32 {
    unsafe { RegisterWindowMessageW(&HSTRING::from(name)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browsers() {
        assert!(is_browser("chrome.exe"));
        assert!(is_browser(r"c:\program files\mozilla firefox\firefox.exe"));
        assert!(is_browser("MSEDGE.EXE"));
        assert!(!is_browser("powershell.exe"));
        assert!(!is_browser(r"c:\temp\chrome.exe.evil"));
    }

    #[test]
    fn tcp_peer_is_found() {
        // Kendi kendimize bağlanırız: karşı ucun sahibi bu süreç olmalı.
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let _c = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let (s, _) = l.accept().unwrap();
        let std::net::SocketAddr::V4(peer) = s.peer_addr().unwrap() else { panic!("ipv4") };
        assert_eq!(tcp_client_pid(peer, port), Some(std::process::id()));
        // Yanlış sunucu portu: eşleşme yok.
        assert_eq!(tcp_client_pid(peer, port.wrapping_add(1)), None);
    }
}
