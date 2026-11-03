//! Bu modül kontrol merkezinin hızlı ayarlarını çözüyor:
//! - **Ses:** Varsayılan çıkış aygıtının ana ses seviyesi (WASAPI).
//! - **Parlaklık:** Harici monitörlerde DDC/CI üzerinden. Desteklemeyen
//!   monitörde kaydırıcı hiç gösterilmez.
//! - **Wi-Fi / Bluetooth:** Windows'un radyo API'si (ayarlar uygulamasındaki
//!   anahtarlarla aynı).
//!
//! Asıl işlemler `carlui_core::quick` içinde (Spotlight da kullanır). DDC/CI ve
//! radyo çağrıları yavaş olabildiği (50–300 ms) için burada arka plan iş
//! parçacığında yapılır; sonuç panele mesajla döner.

pub use carlui_core::quick::{brightness_set, Volume};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    System::WinRT::*,
    UI::WindowsAndMessaging::PostMessageW,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct Radios {
    pub wifi: Option<bool>,
    pub bluetooth: Option<bool>,
}

fn post(notify: isize, msg: u32, wp: usize) {
    unsafe {
        let _ = PostMessageW(HWND(notify as _), msg, WPARAM(wp), LPARAM(0));
    }
}

/// Arka planda radyo durumunu okur ve `notify` penceresine `msg` gönderir
/// (wParam: bit0-1 Wi-Fi, bit2-3 Bluetooth; 0 = yok, 1 = kapalı, 2 = açık).
pub fn query_radios(notify: isize, msg: u32) {
    std::thread::spawn(move || {
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        let (wifi, bt) = carlui_core::quick::radio_states();
        let enc = |v: Option<bool>| match v {
            None => 0usize,
            Some(false) => 1,
            Some(true) => 2,
        };
        post(notify, msg, enc(wifi) | (enc(bt) << 2));
    });
}

pub fn decode(wp: usize) -> Radios {
    let dec = |v: usize| match v & 3 {
        1 => Some(false),
        2 => Some(true),
        _ => None,
    };
    Radios { wifi: dec(wp), bluetooth: dec(wp >> 2) }
}

/// Radyoyu açar/kapatır, bitince durumu yeniden okuyup bildirir.
pub fn set_radio(bluetooth: bool, on: bool, notify: isize, msg: u32) {
    std::thread::spawn(move || {
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        carlui_core::quick::set_radio(bluetooth, on);
        query_radios(notify, msg);
    });
}

/// Parlaklığı arka planda okur; wParam = 0..1000 (yoksa usize::MAX).
pub fn query_brightness(notify: isize, msg: u32) {
    std::thread::spawn(move || {
        let v = carlui_core::quick::brightness_get().map(|b| (b * 1000.0) as usize).unwrap_or(usize::MAX);
        post(notify, msg, v);
    });
}
