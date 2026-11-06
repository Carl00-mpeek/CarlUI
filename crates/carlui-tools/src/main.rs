//! Bu modül CarlUI'nin küçük araçlarının (`carlui-tools.exe`) giriş noktası.
//! Sorun: renk seçici, toplu yeniden adlandırma ve disk analizörü için ayrı
//! programlar kurmak ya da sürekli açık tutmak. Çözüm: ana süreç kısayolu
//! yakalayınca bu programı ilgili bayrakla başlatır; araç işini bitirince
//! kapanır, arkada bellek tutmaz.
//!
//! `--pick` renk seçici · `--rename [--hwnd N]` yeniden adlandırma ·
//! `--disk [sürücü]` disk analizörü · `--unlock [--hwnd N] [yollar]` dosya
//! kilidi açıcı · `--layouts` pencere düzenleri.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
extern crate carlui_core;

mod color;
mod disk;
mod field;
mod layouts;
mod note;
mod picker;
mod rename;
mod select;
mod unlock;

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
    } else if has("--unlock") {
        "unlock"
    } else if has("--layouts") {
        "layouts"
    } else {
        "disk"
    };
    log::init(&format!("tools-{mode}"));
    carlui_ui::tone::follow(&carlui_core::config::load().theme);
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let fg = args.iter().position(|a| a == "--hwnd").and_then(|i| args.get(i + 1)).and_then(|s| s.parse::<isize>().ok()).map(|h| HWND(h as _));
    let r = match mode {
        "pick" => picker::run(),
        "rename" => rename::run(fg.unwrap_or_else(|| unsafe { GetForegroundWindow() })),
        "layouts" => layouts::run(),
        "unlock" => {
            let given = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || args[i - 1] != "--hwnd"));
            let mut paths: Vec<std::path::PathBuf> = given.map(|(_, a)| a.into()).collect();
            if let (true, Some(fg)) = (paths.is_empty(), fg) {
                paths = select::selection(fg).into_iter().map(Into::into).collect();
            }
            unlock::run(paths)
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
