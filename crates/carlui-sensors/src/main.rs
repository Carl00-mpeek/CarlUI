//! Bu modül "Intel CPU sıcaklığı yalnızca yönetici + çekirdek sürücüsüyle
//! okunabiliyor, ama ada ve görev yöneticisi yetkisiz çalışıyor" sorununu
//! çözüyor. Küçük, penceresiz bir yardımcı: açılışta zamanlanmış görevle
//! yönetici olarak başlar, PawnIO sürücüsü + IntelMSR modülüyle saniyede bir
//! sıcaklıkları okur ve paylaşılan belleğe yazar (`carlui_sys::temp`).
//! Başka hiçbir şey yapmaz; okunacak bir şey yoksa bekler, yeniden dener.
//!
//! `--install`: kendisini "CarlUI Sensors" zamanlanmış görevi olarak kaydeder
//! (oturum açılışında, en yüksek yetkiyle) ve hemen başlatır. `--uninstall`
//! görevi siler. İkisi de yönetici olarak çalıştırılmalıdır.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]


use carlui_core::{info, log, warn};
use carlui_sys::temp::{Msr, Publisher};
use std::{os::windows::process::CommandExt, process::Command, time::Duration};
use windows::{core::*, Win32::System::Threading::CreateMutexW, Win32::Foundation::*};

/// PawnIO'nun imzalı Intel MSR modülü (LGPL 2.1, bkz. IntelMSR.COPYING.txt).
static MODULE: &[u8] = include_bytes!("../IntelMSR.bin");
const TASK: &str = "CarlUI Sensors";
const NO_WINDOW: u32 = 0x0800_0000;

fn schtasks(args: &[&str]) -> bool {
    Command::new("schtasks.exe").args(args).creation_flags(NO_WINDOW).status().map(|s| s.success()).unwrap_or(false)
}

fn install() {
    let exe = std::env::current_exe().unwrap_or_default();
    let tr = format!("\"{}\"", exe.display());
    let ok = schtasks(&["/Create", "/F", "/TN", TASK, "/TR", &tr, "/SC", "ONLOGON", "/RL", "HIGHEST", "/IT"]);
    info!("görev kaydı: {}", if ok { "tamam" } else { "başarısız" });
    if ok {
        schtasks(&["/Run", "/TN", TASK]);
    }
}

fn main() {
    log::init("sensors");
    match std::env::args().nth(1).as_deref() {
        Some("--install") => return install(),
        Some("--uninstall") => {
            schtasks(&["/End", "/TN", TASK]);
            let ok = schtasks(&["/Delete", "/F", "/TN", TASK]);
            info!("görev silindi: {ok}");
            return;
        }
        _ => {}
    }
    unsafe {
        let _m = CreateMutexW(None, true, w!("Local\\CarlUI.Sensors"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }
    }
    let Some(publisher) = Publisher::new() else {
        warn!("paylaşılan bellek açılamadı");
        return;
    };
    let mut reported = false;
    loop {
        match Msr::open(MODULE) {
            Ok(msr) => {
                info!("sıcaklık okuyucu hazır");
                reported = false;
                loop {
                    match msr.read() {
                        Some(t) => publisher.publish(&t),
                        None => break,
                    }
                    std::thread::sleep(Duration::from_millis(1000));
                }
                warn!("sıcaklık okunamadı, yeniden bağlanılıyor");
            }
            Err(e) if !reported => {
                warn!("{e}");
                reported = true;
            }
            Err(_) => {}
        }
        std::thread::sleep(Duration::from_secs(10));
    }
}
