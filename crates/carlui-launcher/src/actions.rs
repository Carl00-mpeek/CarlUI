//! Bu modül "seçilen sonuçla ne yapılır?" sorununu çözüyor: uygulamayı açmak
//! ya da açık penceresini öne getirmek, dosya konumunu göstermek, panoya
//! kopyalamak, sistem komutları ve yapay zekâ istemini ilgili pencereye yazmak.
//! Uzun süren işler (yeniden başlatma, istemin pencereye yazılması) ayrı iş
//! parçacığında yapılır; arayüz beklemez. Hiçbir uygulama zorla kapatılmaz.

use crate::search::{Act, Sys};
use carlui_core::{info, warn};
use std::time::{Duration, Instant};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{
            DataExchange::*,
            Memory::*,
            Ole::CF_UNICODETEXT,
            Power::SetSuspendState,
            Shutdown::*,
            Threading::*,
        },
        UI::{Input::KeyboardAndMouse::*, Shell::ShellExecuteW, WindowsAndMessaging::*},
    },
};

fn shell(verb: PCWSTR, target: &str, args: &str) -> bool {
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        let r = ShellExecuteW(HWND::default(), verb, &HSTRING::from(target), &HSTRING::from(args), PCWSTR::null(), SW_SHOWNORMAL);
        r.0 as isize > 32
    }
}

pub fn bring_to_front(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindowAsync(hwnd, SW_RESTORE);
        }
        if !SetForegroundWindow(hwnd).as_bool() {
            SwitchToThisWindow(hwnd, true);
        }
    }
}

pub fn copy_text(s: &str) -> bool {
    unsafe {
        if OpenClipboard(HWND::default()).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let wide: Vec<u16> = s.encode_utf16().chain([0]).collect();
        let bytes = wide.len() * 2;
        let ok = (|| -> Result<()> {
            let h = GlobalAlloc(GMEM_MOVEABLE, bytes)?;
            let p = GlobalLock(h) as *mut u16;
            if p.is_null() {
                return Err(E_OUTOFMEMORY.into());
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
            let _ = GlobalUnlock(h);
            SetClipboardData(CF_UNICODETEXT.0 as u32, HANDLE(h.0))?;
            Ok(())
        })()
        .is_ok();
        let _ = CloseClipboard();
        ok
    }
}

fn type_text(s: &str) {
    let mut inputs = Vec::new();
    for u in s.encode_utf16() {
        for up in [false, true] {
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT { wScan: u, dwFlags: KEYEVENTF_UNICODE | if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, ..Default::default() },
                },
            });
        }
    }
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

fn window_title(h: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(h, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n])
}

/// Başlığında `hint` geçen pencere öne gelince istemi yazar (en çok 15 sn bekler).
fn type_into(hint: &'static str, prompt: String) {
    std::thread::spawn(move || {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(15) {
            std::thread::sleep(Duration::from_millis(250));
            let fg = unsafe { GetForegroundWindow() };
            if window_title(fg).to_lowercase().contains(&hint.to_lowercase()) {
                // Sayfanın yazı alanı odaklansın diye kısa bir süre daha bekle.
                std::thread::sleep(Duration::from_millis(900));
                if unsafe { GetForegroundWindow() } == fg {
                    type_text(&prompt);
                    info!("istem {hint} penceresine yazıldı");
                }
                return;
            }
        }
        warn!("{hint} penceresi 15 sn içinde öne gelmedi; istem yazılmadı");
    });
}

fn process_path(pid: u32) -> String {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        if ok.is_ok() { String::from_utf16_lossy(&buf[..len as usize]) } else { String::new() }
    }
}

fn restart(pid: u32, hwnd: isize, launch: String) {
    std::thread::spawn(move || unsafe {
        let exe = process_path(pid);
        let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) else { return };
        let _ = PostMessageW(HWND(hwnd as _), WM_CLOSE, WPARAM(0), LPARAM(0));
        let closed = WaitForSingleObject(h, 8000) == WAIT_OBJECT_0;
        let _ = CloseHandle(h);
        if !closed {
            warn!("uygulama kapanmadı (pid {pid}); zorla kapatılmadı, yeniden başlatılmadı");
            return;
        }
        // Store uygulamaları exe yoluyla açılamaz; kimlikleriyle aç.
        let target = if launch.starts_with("shell:") || exe.is_empty() { launch } else { exe };
        shell(w!("open"), &target, "");
    });
}

/// Eylemi uygular. Arayüz kapanmalıysa true.
pub fn run(act: &Act) -> bool {
    match act {
        Act::App { launch, hwnd } => {
            match hwnd {
                Some(h) if unsafe { IsWindow(HWND(*h as _)).as_bool() } => bring_to_front(HWND(*h as _)),
                _ => {
                    shell(w!("open"), launch, "");
                }
            }
            true
        }
        Act::Open(t) => {
            if t == "explorer:" || t == "explorer.exe" {
                shell(w!("open"), "explorer.exe", "")
            } else {
                shell(w!("open"), t, "")
            };
            true
        }
        Act::Admin(p) => {
            shell(w!("runas"), p, "");
            true
        }
        Act::Reveal(p) => {
            shell(w!("open"), "explorer.exe", &format!("/select,\"{p}\""));
            true
        }
        Act::Copy(s) => {
            copy_text(s);
            true
        }
        Act::Close(h) => unsafe {
            let _ = PostMessageW(HWND(*h as _), WM_CLOSE, WPARAM(0), LPARAM(0));
            true
        },
        Act::Restart { pid, hwnd, launch } => {
            restart(*pid, *hwnd, launch.clone());
            true
        }
        Act::System(s) => {
            system(*s);
            true
        }
        Act::Ai { url, launch, window_hint, prompt } => {
            if let Some(u) = url {
                shell(w!("open"), u, "");
            } else if let Some(l) = launch {
                shell(w!("open"), l, "");
            }
            if !prompt.is_empty() {
                type_into(window_hint, prompt.clone());
            }
            true
        }
        Act::Quick(q) => {
            crate::quickset::run(*q);
            true
        }
        // Zamanlayıcıyı ada kendisi kurar (bkz. carlui-island timing).
        Act::Timer { .. } | Act::Stopwatch | Act::TimerStop | Act::Download(_) | Act::DownloadCancel => true,
        Act::AllApps | Act::Pending | Act::None => false,
    }
}

fn system(s: Sys) {
    info!("sistem komutu: {s:?}");
    unsafe {
        match s {
            Sys::Lock => {
                let _ = LockWorkStation();
            }
            Sys::Sleep => {
                // Arayüz kapansın diye kısa gecikme.
                std::thread::spawn(|| {
                    std::thread::sleep(Duration::from_millis(400));
                    let _ = SetSuspendState(false, false, false);
                });
            }
            Sys::SignOut => {
                let _ = ExitWindowsEx(EWX_LOGOFF, SHTDN_REASON_FLAG_PLANNED);
            }
            // Kapatma/yeniden başlatma için sistemin kendi aracı (yetki ayarını kendisi yapar).
            Sys::Restart => {
                shell(w!("open"), "shutdown.exe", "/r /t 0");
            }
            Sys::Shutdown => {
                shell(w!("open"), "shutdown.exe", "/s /t 0");
            }
        }
    }
}
