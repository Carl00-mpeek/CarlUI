//! Bu modül "program kaldırılınca geride klasör ve kayıt artıkları kalıyor"
//! sorununu çözüyor: tam kaldırıcı. Önce uygulamanın kendi kaldırıcısı çalışır,
//! ardından artıklar taranır ve kullanıcının onayladıkları temizlenir.
//!
//! Güvenlik kuralları: yalnızca tam ad eşleşmesi aranır, korunan klasör ve
//! anahtarlara dokunulmaz (`safety`), dosyalar Geri Dönüşüm Kutusu'na gider,
//! kayıt anahtarları silinmeden önce `.reg` olarak yedeklenir (`clean`).
//!
//! `--clean <dosya>`: normal kullanıcının silemediği öğeler için yönetici
//! yetkisiyle açılan penceresiz geçiş.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
extern crate carlui_core;

mod apps;
mod clean;
#[allow(dead_code)]
#[path = "../../carlui-tools/src/field.rs"]
mod field;
mod leftovers;
mod names;
mod reg;
mod run;
mod safety;
mod ui;

use carlui_core::log;
use windows::Win32::{System::Com::*, UI::HiDpi::*};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(i) = args.iter().position(|a| a == "--clean") {
        log::init("uninstall-clean");
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        if let Some(f) = args.get(i + 1) {
            clean::elevated(f);
        }
        return;
    }
    log::init("uninstall");
    carlui_ui::tone::follow(&carlui_core::config::load().theme);
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    if let Err(e) = ui::run() {
        error!("uninstall: {e}");
    }
}
