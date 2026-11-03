//! Menüden başlatılan şeyler: uygulamalar, CarlUI pencereleri ve güç komutları.

use crate::store::Tile;
use std::os::windows::process::CommandExt;
use windows::{
    core::*,
    Win32::{
        Foundation::HWND,
        System::{Power::SetSuspendState, Shutdown::LockWorkStation},
        UI::{Shell::ShellExecuteW, WindowsAndMessaging::*},
    },
};

/// `%VAR%` biçimindeki ortam değişkenlerini açar (dock'taki sabitlemeler gibi).
pub fn expand(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(a) = rest.find('%') {
        out.push_str(&rest[..a]);
        let after = &rest[a + 1..];
        match after.find('%') {
            Some(b) => {
                let name = &after[..b];
                match std::env::var(name) {
                    Ok(v) if !name.is_empty() => out.push_str(&v),
                    _ => out.push_str(&rest[a..a + b + 2]),
                }
                rest = &after[b + 1..];
            }
            None => {
                out.push_str(&rest[a..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Ayrı iş parçacığında açar: ShellExecute yüzlerce ms sürebilir; menünün
/// iş parçacığı kilitlenince kapanış animasyonu donup menü "pat" diye kayboluyordu.
fn shell(path: &str, args: &str) {
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
    let (path, args) = (HSTRING::from(path), HSTRING::from(args));
    std::thread::spawn(move || unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED | windows::Win32::System::Com::COINIT_DISABLE_OLE1DDE);
        ShellExecuteW(HWND::default(), w!("open"), &path, &args, PCWSTR::null(), SW_SHOWNORMAL);
    });
}

pub fn launch(t: &Tile) {
    carlui_core::info!("Win menüsü: {} açılıyor", t.name);
    shell(&expand(&t.path), &t.args);
}

/// Alt çubuktaki yerler: 0 İndirilenler, 1 Geri Dönüşüm Kutusu.
pub fn place(n: u8) {
    shell(if n == 0 { "shell:Downloads" } else { "shell:RecycleBinFolder" }, "");
}

/// CarlUI'nin yanındaki bir exe (ayarlar, görev yöneticisi).
pub fn sibling(exe: &str) {
    shell(&carlui_core::paths::sibling_exe(exe).to_string_lossy(), "");
}

#[derive(Clone, Copy)]
pub enum Power {
    Lock,
    SignOut,
    Sleep,
    Restart,
    Shutdown,
}

pub fn power(p: Power) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let run = |args: &[&str]| {
        let _ = std::process::Command::new("shutdown.exe").args(args).creation_flags(CREATE_NO_WINDOW).spawn();
    };
    unsafe {
        match p {
            Power::Lock => {
                let _ = LockWorkStation();
            }
            Power::SignOut => run(&["/l"]),
            Power::Sleep => {
                let _ = SetSuspendState(false, false, false);
            }
            Power::Restart => run(&["/r", "/t", "0"]),
            Power::Shutdown => run(&["/s", "/t", "0"]),
        }
    }
}
