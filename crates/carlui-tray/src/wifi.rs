//! Kablosuz ağ listesi (WLAN API): görünen ağlar, bağlı olan ve kayıtlı
//! profili olanlara tek tıkla bağlanma. Parola isteyen yeni ağlar için
//! Windows'un kendi ağ listesi açılır (parolayı o sorar).

use windows::{
    core::*,
    Win32::{Foundation::HANDLE, NetworkManagement::WiFi::*},
};

#[derive(Clone, Debug, PartialEq)]
pub struct Network {
    pub ssid: String,
    /// Kayıtlı profil adı (boşsa profil yok: parola gerekebilir).
    pub profile: String,
    /// 0-100.
    pub signal: u32,
    pub secure: bool,
    pub connected: bool,
}

struct Client(HANDLE);

impl Client {
    fn open() -> Option<Self> {
        let (mut ver, mut h) = (0u32, HANDLE::default());
        (unsafe { WlanOpenHandle(2, None, &mut ver, &mut h) } == 0).then_some(Self(h))
    }

    /// İlk kablosuz bağdaştırıcı.
    fn interface(&self) -> Option<GUID> {
        unsafe {
            let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
            if WlanEnumInterfaces(self.0, None, &mut list) != 0 || list.is_null() {
                return None;
            }
            let n = (*list).dwNumberOfItems as usize;
            let items = std::slice::from_raw_parts((*list).InterfaceInfo.as_ptr(), n);
            let g = items.first().map(|i| i.InterfaceGuid);
            WlanFreeMemory(list as _);
            g
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        unsafe {
            WlanCloseHandle(self.0, None);
        }
    }
}

fn wide(s: &[u16]) -> String {
    String::from_utf16_lossy(&s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())])
}

/// Görünen ağlar (aynı ad bir kez; bağlı olan önce, sonra sinyale göre).
/// `None`: kablosuz bağdaştırıcı yok.
pub fn networks() -> Option<Vec<Network>> {
    let c = Client::open()?;
    let g = c.interface()?;
    let mut out: Vec<Network> = Vec::new();
    unsafe {
        let mut list: *mut WLAN_AVAILABLE_NETWORK_LIST = std::ptr::null_mut();
        if WlanGetAvailableNetworkList(c.0, &g, 0, None, &mut list) != 0 || list.is_null() {
            return Some(out);
        }
        let n = (*list).dwNumberOfItems as usize;
        for a in std::slice::from_raw_parts((*list).Network.as_ptr(), n) {
            let len = (a.dot11Ssid.uSSIDLength as usize).min(32);
            let ssid = String::from_utf8_lossy(&a.dot11Ssid.ucSSID[..len]).into_owned();
            if ssid.is_empty() {
                continue;
            }
            let net = Network {
                ssid,
                profile: if a.dwFlags & WLAN_AVAILABLE_NETWORK_HAS_PROFILE != 0 { wide(&a.strProfileName) } else { String::new() },
                signal: a.wlanSignalQuality.min(100),
                secure: a.bSecurityEnabled.as_bool(),
                connected: a.dwFlags & WLAN_AVAILABLE_NETWORK_CONNECTED != 0,
            };
            // Aynı ad profilli ve profilsiz iki kez gelebilir: bağlı/profilli olanı tut.
            match out.iter_mut().find(|o| o.ssid == net.ssid) {
                Some(o) => {
                    if net.connected || (o.profile.is_empty() && !net.profile.is_empty()) {
                        *o = net;
                    }
                }
                None => out.push(net),
            }
        }
        WlanFreeMemory(list as _);
    }
    out.sort_by(|a, b| b.connected.cmp(&a.connected).then(b.signal.cmp(&a.signal)));
    Some(out)
}

/// Bağdaştırıcıya yeniden tarama yaptırır (sonuç birkaç saniyede gelir).
pub fn scan() {
    if let Some(c) = Client::open() {
        if let Some(g) = c.interface() {
            unsafe {
                WlanScan(c.0, &g, None, None, None);
            }
        }
    }
}

/// Kayıtlı profille bağlanır.
pub fn connect(profile: &str) -> bool {
    let Some(c) = Client::open() else { return false };
    let Some(g) = c.interface() else { return false };
    let name = HSTRING::from(profile);
    let p = WLAN_CONNECTION_PARAMETERS {
        wlanConnectionMode: wlan_connection_mode_profile,
        strProfile: PCWSTR(name.as_ptr()),
        dot11BssType: dot11_BSS_type_any,
        ..Default::default()
    };
    unsafe { WlanConnect(c.0, &g, &p, None) == 0 }
}
