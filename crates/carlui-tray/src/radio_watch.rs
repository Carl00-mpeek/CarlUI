//! Bu modül "Wi-Fi / Bluetooth / kablo durumu değişince panel nasıl haberdar
//! olsun?" sorusunu yoklama yapmadan çözüyor:
//! - **Radyolar:** Her radyonun `StateChanged` olayına abone olunur. Windows
//!   ayarlarından, klavye tuşundan ya da uçak kipinden yapılan değişiklik
//!   anında gelir.
//! - **Kablo:** `NotifyIpInterfaceChange` ile ağ arayüzü değişince haber alınır.
//! - **Sonradan beliren adaptör:** Windows her üst düzey pencereye
//!   `WM_DEVICECHANGE` gönderir. Panel penceresi bunu alınca radyolar yeniden
//!   listelenir (ör. açılışta Bluetooth adaptörü geç hazır olduğunda).
//!
//! Durum yalnızca değiştiğinde panele `WM_APP_RADIOS` ile gönderilir. Panel
//! açılırken hiçbir şey sorgulanmaz, son bilinen durum kullanılır.

use crate::quick::{encode, Radios};
use std::{
    ffi::c_void,
    sync::{
        atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicUsize, Ordering},
        Mutex,
    },
    time::Duration,
};
use windows::{
    Devices::Radios::Radio,
    Foundation::{EventRegistrationToken, TypedEventHandler},
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        System::WinRT::*,
        UI::WindowsAndMessaging::PostMessageW,
    },
};

/// Son gönderilen kodlanmış durum; `usize::MAX` = henüz bilinmiyor.
static LAST: AtomicUsize = AtomicUsize::new(usize::MAX);
static TARGET: AtomicIsize = AtomicIsize::new(0);
static MSG: AtomicU32 = AtomicU32::new(0);
static RUNNING: AtomicBool = AtomicBool::new(false);
static RESCAN_PENDING: AtomicBool = AtomicBool::new(false);
static SUBS: Mutex<Vec<(Radio, EventRegistrationToken)>> = Mutex::new(Vec::new());

/// AF_UNSPEC
const ANY_FAMILY: u16 = 0;

type IfChange = unsafe extern "system" fn(ctx: *const c_void, row: *const c_void, kind: i32);

#[link(name = "iphlpapi")]
extern "system" {
    fn NotifyIpInterfaceChange(family: u16, callback: IfChange, ctx: *const c_void, initial: u8, handle: *mut isize) -> u32;
}

unsafe extern "system" fn on_interface(_: *const c_void, _: *const c_void, _: i32) {
    refresh();
}

/// İzlemeyi başlatır: durumu bir kez okur, sonra yalnızca olaylarla günceller.
pub fn start(notify: isize, msg: u32) {
    TARGET.store(notify, Ordering::Relaxed);
    MSG.store(msg, Ordering::Relaxed);
    std::thread::spawn(|| {
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        subscribe();
        let mut handle = 0isize;
        let err = unsafe { NotifyIpInterfaceChange(ANY_FAMILY, on_interface, std::ptr::null(), 0, &mut handle) };
        if err != 0 {
            carlui_core::warn!("ağ arayüzü bildirimi kurulamadı: {err}");
        }
        RUNNING.store(true, Ordering::Release);
    });
}

/// İzleyici çalışıyor mu (değilse panel açılışta durumu kendisi sorar).
pub fn running() -> bool {
    RUNNING.load(Ordering::Acquire)
}

/// Aygıt eklendi/çıkarıldı: radyoları yeniden listele. Art arda gelen
/// bildirimler tek taramada birleştirilir.
pub fn rescan() {
    if !running() || RESCAN_PENDING.swap(true, Ordering::AcqRel) {
        return;
    }
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(1500));
        RESCAN_PENDING.store(false, Ordering::Release);
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        subscribe();
    });
}

/// Radyoları listeler, her birinin durum olayına abone olur ve eski
/// abonelikleri bırakır.
fn subscribe() {
    let list = carlui_core::quick::radios().unwrap_or_default();
    let mut fresh = Vec::with_capacity(list.len());
    for radio in list {
        match radio.StateChanged(&TypedEventHandler::new(|_, _| {
            refresh();
            Ok(())
        })) {
            Ok(token) => fresh.push((radio, token)),
            Err(e) => carlui_core::warn!("radyo olayına abone olunamadı: {e}"),
        }
    }
    let old = std::mem::replace(&mut *SUBS.lock().unwrap_or_else(|e| e.into_inner()), fresh);
    for (radio, token) in old {
        let _ = radio.RemoveStateChanged(token);
    }
    refresh();
}

/// Abone olunan radyolardan ve kablodan durumu hesaplar; değiştiyse panele bildirir.
fn refresh() {
    let (wifi, bluetooth) = {
        let subs = SUBS.lock().unwrap_or_else(|e| e.into_inner());
        let radios: Vec<Radio> = subs.iter().map(|(r, _)| r.clone()).collect();
        drop(subs);
        carlui_core::quick::states_of(&radios)
    };
    let r = Radios { wifi, bluetooth, wired: carlui_core::wired::wired_up() };
    let code = encode(r);
    if LAST.swap(code, Ordering::AcqRel) == code {
        return;
    }
    carlui_core::info!("radyo durumu: Wi-Fi {:?}, Bluetooth {:?}, kablo {}", r.wifi, r.bluetooth, r.wired);
    unsafe {
        let _ = PostMessageW(HWND(TARGET.load(Ordering::Relaxed) as _), MSG.load(Ordering::Relaxed), WPARAM(code), LPARAM(0));
    }
}
