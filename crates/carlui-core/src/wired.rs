//! Bu modül "kablolu bağlıyken hızlı panel ne göstersin?" sorusunu çözüyor.
//! Wi-Fi kartı olmayan (ya da kapalı olan) bilgisayarda Wi-Fi kutucuğu boşta
//! kalmasın diye kablolu bağlantı burada okunur: fiziksel bir Ethernet
//! bağdaştırıcısı açık ve kablo takılıysa bağlı sayılır. Sanal bağdaştırıcılar
//! (Hyper-V, VPN, geri döngü) sayılmaz.

use windows::{
    core::{w, HSTRING, PCWSTR},
    Win32::{
        Foundation::HWND,
        NetworkManagement::{
            IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2},
            Ndis::{IfOperStatusUp, MediaConnectStateConnected},
        },
        UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    },
};

/// IF_TYPE_ETHERNET_CSMACD (ipifcons.h).
const ETHERNET: u32 = 6;

/// Fiziksel bir Ethernet bağdaştırıcısı bağlı mı?
pub fn wired_up() -> bool {
    unsafe {
        let mut t: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        if GetIfTable2(&mut t).is_err() || t.is_null() {
            return false;
        }
        let rows = std::slice::from_raw_parts((*t).Table.as_ptr(), (*t).NumEntries as usize);
        // HardwareInterface, bit alanının ilk bitidir.
        let up = rows.iter().any(|r| r.Type == ETHERNET && r.InterfaceAndOperStatusFlags._bitfield & 1 != 0 && r.OperStatus == IfOperStatusUp && r.MediaConnectState == MediaConnectStateConnected);
        FreeMibTable(t as _);
        up
    }
}

#[cfg(test)]
mod tests {
    /// Elle çalıştırılır: `cargo test -p carlui-core wired -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn wired_now() {
        println!("kablolu bağlı: {}", super::wired_up());
    }
}

/// Kablosuz ağ seçimini açar: Wi-Fi kartı varsa Windows'un ağ listesi, yoksa
/// (seçilecek ağ olmadığından) ağ durumu sayfası.
pub fn open_networks(wifi_present: bool) {
    let uri = if wifi_present { "ms-availablenetworks:" } else { "ms-settings:network-status" };
    unsafe {
        ShellExecuteW(HWND::default(), w!("open"), &HSTRING::from(uri), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}
