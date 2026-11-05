//! Bu modül CarlUI'nin küçük araçlarının (`carlui-tools.exe`) giriş noktası.
//! Sorun: renk seçici, toplu yeniden adlandırma ve disk analizörü için ayrı
//! programlar kurmak ya da sürekli açık tutmak. Çözüm: ana süreç kısayolu
//! yakalayınca bu programı ilgili bayrakla başlatır; araç işini bitirince
//! kapanır, arkada bellek tutmaz.
//!
//! `--pick` renk seçici · `--rename [--hwnd N]` yeniden adlandırma ·
//! `--disk [sürücü]` disk analizörü.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
extern crate carlui_core;

mod color;
mod disk;
mod field;
mod note;
mod picker;
mod rename;
mod select;

use carlui_core::log;
use carlui_ui::view;
use windows::Win32::{
    Foundation::HWND,
    System::Com::*,
    UI::{HiDpi::*, WindowsAndMessaging::GetForegroundWindow},
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    let mode = if has("--pick") {
        "pick"
    } else if has("--rename") {
        "rename"
    } else {
        "disk"
    };
    log::init(&format!("tools-{mode}"));
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let r = match mode {
        "pick" => picker::run(),
        "rename" => {
            let fg = args.iter().position(|a| a == "--hwnd").and_then(|i| args.get(i + 1)).and_then(|s| s.parse::<isize>().ok()).map(|h| HWND(h as _));
            rename::run(fg.unwrap_or_else(|| unsafe { GetForegroundWindow() }))
        }
        _ => {
            let drive = args.iter().find(|a| !a.starts_with("--")).cloned();
            disk::run(drive)
        }
    };
    if let Err(e) = r {
        carlui_core::error!("{mode}: {e}");
    }
}
