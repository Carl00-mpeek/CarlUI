//! Dock ikonunun arkasındaki süreçler: sağ tık menüsündeki "Programı durdur /
//! sürdür / kapat" ve "Görev yöneticisi" buradan çalışır. Bir uygulamanın
//! birden çok penceresi aynı ya da farklı süreçlerde olabilir; hepsine birlikte
//! uygulanır. Durdurulmuş olup olmadığı çekirdekten okunur (`carlui_sys::process`),
//! görev yöneticisinden yapılan değişiklik de doğru görünür.

use crate::{apps, dock::Entry};
use carlui_core::info;
use carlui_sys::process;
use windows::Win32::{Foundation::HWND, UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL}};
use windows::core::*;

/// Girdinin süreçleri. explorer.exe hiç verilmez: masaüstü ve görev çubuğu da
/// odur, durdurmak her şeyi dondurur, kapatmak ekranı siyah bırakır.
pub fn pids(e: &Entry) -> Vec<u32> {
    let mut v: Vec<u32> = e
        .windows
        .iter()
        .map(|&h| apps::process_id_of_window(h))
        .filter(|&p| p != 0 && !carlui_core::win::process_exe(p).ends_with("\\explorer.exe"))
        .collect();
    v.sort();
    v.dedup();
    v
}

/// Süreç yoksa `None`; hepsi durdurulmuşsa `Some(true)`.
pub fn suspended(pids: &[u32]) -> Option<bool> {
    if pids.is_empty() {
        return None;
    }
    let all = process::list();
    Some(pids.iter().all(|p| all.iter().any(|q| q.pid == *p && q.suspended)))
}

pub fn suspend(pids: &[u32], name: &str) {
    let ok = pids.iter().filter(|&&p| process::suspend(p)).count();
    info!("{name} durduruldu ({ok}/{} süreç)", pids.len());
}

pub fn resume(pids: &[u32], name: &str) {
    let ok = pids.iter().filter(|&&p| process::resume(p)).count();
    info!("{name} sürdürüldü ({ok}/{} süreç)", pids.len());
}

pub fn kill(pids: &[u32], name: &str) {
    // Durdurulmuş süreç de sonlanabilir; yine de önce sürdür ki temiz kapansın.
    for &p in pids {
        process::resume(p);
    }
    let ok = pids.iter().filter(|&&p| process::terminate(p)).count();
    info!("{name} kapatıldı ({ok}/{} süreç)", pids.len());
}

/// CarlUI görev yöneticisini açar, verilen süreç seçili gelir.
pub fn task_manager(pid: Option<u32>) {
    launch("carlui-taskmgr.exe", pid.map(|p| format!("--pid {p}")).unwrap_or_default());
}

/// CarlUI ayarlar penceresini açar.
pub fn settings() {
    launch("carlui-settings.exe", String::new());
}

/// Dönüştürücü kısayolu: öndeki pencere (Gezgin ya da masaüstü) seçimi için verilir.
pub fn converter() {
    let fg = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
    launch("carlui-convert.exe", format!("--hwnd {}", fg.0 as isize));
}

fn launch(name: &str, args: String) {
    let exe = std::env::current_exe().unwrap_or_default().with_file_name(name);
    if !exe.exists() {
        info!("{name} bulunamadı: {}", exe.display());
        return;
    }
    unsafe {
        // Yeni pencere dock'un arkasında kalmasın: öne gelme iznini devret.
        let _ = windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(windows::Win32::UI::WindowsAndMessaging::ASFW_ANY);
        ShellExecuteW(HWND::default(), w!("open"), &HSTRING::from(exe.as_os_str()), &HSTRING::from(args), PCWSTR::null(), SW_SHOWNORMAL);
    }
}
