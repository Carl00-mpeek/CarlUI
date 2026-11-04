//! Belirteçle istenen değeri hedef pencereye yazar (Aşama 2). Değer panoya
//! konmaz; klavye girdisi olarak (KEYEVENTF_UNICODE) öndeki pencereye gider.
//! Önce hedef doğrulanır: gerçek bir üst düzey, görünür pencere olmalı, kasa
//! ya da istekte bulunan ada olmamalı ve klavye odağı onda olmalı.

use carlui_core::t;
use std::time::{Duration, Instant};
use windows::Win32::{
    Foundation::HWND,
    System::Threading::GetCurrentProcessId,
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};
use zeroize::Zeroize;

/// Hedefi doğrular; odak birkaç yüz ms içinde ona geçmezse hata.
pub fn check_target(hwnd: HWND, client_pid: u32) -> Result<String, String> {
    unsafe {
        if hwnd.is_invalid() || !IsWindow(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool() || GetAncestor(hwnd, GA_ROOT) != hwnd {
            return Err(t!("Hedef pencere geçersiz.", "The target window isn't valid.").into());
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 || pid == GetCurrentProcessId() || pid == client_pid {
            return Err(t!("Hedef pencere kabul edilmedi.", "The target window was refused.").into());
        }
        // Ada pano panelini kapatıp odağı önceki pencereye geri verir; az bekle.
        let start = Instant::now();
        while GetForegroundWindow() != hwnd {
            if start.elapsed() > Duration::from_millis(700) {
                return Err(t!("Hedef pencere önde değil; yazılmadı.", "The target window isn't in front; nothing was typed.").into());
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        let exe = carlui_core::win::window_exe(hwnd);
        Ok(exe.rsplit('\\').next().unwrap_or_default().to_string())
    }
}

fn key(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } } }
}

/// Basılı değiştiriciler (Ctrl/Alt/Shift/Win) bırakılana dek bekler (en çok 1 sn);
/// yoksa Ctrl+harf gibi kısayollar tetiklenirdi.
fn wait_modifiers() {
    let start = Instant::now();
    let held = || [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN].iter().any(|k| unsafe { GetAsyncKeyState(k.0 as i32) } < 0);
    while held() && start.elapsed() < Duration::from_secs(1) {
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Metni öndeki pencereye yazar; hepsi gönderildiyse true. (Yönetici olarak
/// çalışan pencereler Windows gereği girdiyi almayabilir.)
pub fn type_text(text: &str) -> bool {
    wait_modifiers();
    let mut inputs = Vec::with_capacity(text.len() * 2);
    for ch in text.chars() {
        match ch {
            '\r' => {}
            '\n' => {
                inputs.push(key(VK_RETURN, 0, KEYBD_EVENT_FLAGS(0)));
                inputs.push(key(VK_RETURN, 0, KEYEVENTF_KEYUP));
            }
            '\t' => {
                inputs.push(key(VK_TAB, 0, KEYBD_EVENT_FLAGS(0)));
                inputs.push(key(VK_TAB, 0, KEYEVENTF_KEYUP));
            }
            _ => {
                let mut b = [0u16; 2];
                for u in ch.encode_utf16(&mut b).iter() {
                    inputs.push(key(VIRTUAL_KEY(0), *u, KEYEVENTF_UNICODE));
                    inputs.push(key(VIRTUAL_KEY(0), *u, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
                }
                b.zeroize();
            }
        }
    }
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) } as usize;
    let all = sent == inputs.len();
    // Girdi dizisi değerin karakterlerini taşıyor: bellekten sil.
    for i in inputs.iter_mut() {
        unsafe {
            std::ptr::write_volatile(&mut i.Anonymous.ki.wScan, 0);
        }
    }
    all
}
