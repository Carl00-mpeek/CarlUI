//! Bu modül kontrol merkezinin hızlı ayarlarını çözüyor:
//! - **Ses:** Varsayılan çıkış aygıtının ana ses seviyesi (WASAPI).
//! - **Parlaklık:** Harici monitörlerde DDC/CI üzerinden. Desteklemeyen
//!   monitörde kaydırıcı hiç gösterilmez.
//! - **Wi-Fi / Bluetooth:** Windows'un radyo API'si (ayarlar uygulamasındaki
//!   anahtarlarla aynı).
//!
//! Asıl işlemler `carlui_core::quick` içinde (arama da kullanır). DDC/CI ve
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
    /// Fiziksel Ethernet kablosu takılı ve bağlı.
    pub wired: bool,
}

impl Radios {
    /// Wi-Fi kutucuğu "Ethernet" olarak mı görünsün: kablolu bağlı ve Wi-Fi açık değil.
    pub fn ethernet_shown(&self) -> bool {
        self.wired && self.wifi != Some(true)
    }
}

fn post(notify: isize, msg: u32, wp: usize) {
    unsafe {
        let _ = PostMessageW(HWND(notify as _), msg, WPARAM(wp), LPARAM(0));
    }
}

/// Arka planda radyo durumunu okur ve `notify` penceresine `msg` gönderir
/// (wParam: bit0-1 Wi-Fi, bit2-3 Bluetooth; 0 = yok, 1 = kapalı, 2 = açık;
/// bit4 = Ethernet bağlı).
pub fn query_radios(notify: isize, msg: u32) {
    std::thread::spawn(move || {
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        let (wifi, bt) = carlui_core::quick::radio_states();
        post(notify, msg, encode(Radios { wifi, bluetooth: bt, wired: carlui_core::wired::wired_up() }));
    });
}

pub fn encode(r: Radios) -> usize {
    let enc = |v: Option<bool>| match v {
        None => 0usize,
        Some(false) => 1,
        Some(true) => 2,
    };
    enc(r.wifi) | (enc(r.bluetooth) << 2) | ((r.wired as usize) << 4)
}

pub fn decode(wp: usize) -> Radios {
    let dec = |v: usize| match v & 3 {
        1 => Some(false),
        2 => Some(true),
        _ => None,
    };
    Radios { wifi: dec(wp), bluetooth: dec(wp >> 2), wired: wp & 16 != 0 }
}

/// Ethernet kutucuğuna tıklandı: Wi-Fi kartı varsa Wi-Fi açılır ve Windows'un
/// ağ listesi gösterilir (kablosuz ağ seçilir); kart yoksa ağ ayarları açılır.
pub fn to_wireless(r: Radios, notify: isize, msg: u32) {
    std::thread::spawn(move || {
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        let present = r.wifi.is_some();
        if present {
            carlui_core::quick::set_radio(false, true);
        }
        carlui_core::wired::open_networks(present);
        query_radios(notify, msg);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radios_roundtrip() {
        for wifi in [None, Some(false), Some(true)] {
            for bluetooth in [None, Some(false), Some(true)] {
                for wired in [false, true] {
                    let r = decode(encode(Radios { wifi, bluetooth, wired }));
                    assert_eq!((r.wifi, r.bluetooth, r.wired), (wifi, bluetooth, wired));
                }
            }
        }
    }

    #[test]
    fn ethernet_face() {
        // Kartı olmayan bilgisayar, kablo takılı: Ethernet görünür.
        assert!(Radios { wifi: None, bluetooth: None, wired: true }.ethernet_shown());
        assert!(Radios { wifi: Some(false), bluetooth: None, wired: true }.ethernet_shown());
        // Wi-Fi açıkken kutucuk Wi-Fi'dir (tıklayınca kapanır, Ethernet'e dönülür).
        assert!(!Radios { wifi: Some(true), bluetooth: None, wired: true }.ethernet_shown());
        assert!(!Radios { wifi: None, bluetooth: None, wired: false }.ethernet_shown());
    }
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
