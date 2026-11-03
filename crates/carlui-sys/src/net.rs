//! Ağ: fiziksel bağdaştırıcıların (Wi-Fi, Ethernet) toplam alınan baytı.
//! Sanal bağdaştırıcılar (Hyper-V, VPN tüneli, geri döngü) sayılmaz; yoksa
//! aynı trafik iki kez sayılırdı.

use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;

/// Açılıştan beri alınan toplam bayt (yalnızca farkı anlamlıdır).
pub fn rx_total() -> u64 {
    unsafe {
        let mut t: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        if GetIfTable2(&mut t).is_err() || t.is_null() {
            return 0;
        }
        let rows = std::slice::from_raw_parts((*t).Table.as_ptr(), (*t).NumEntries as usize);
        // HardwareInterface, bit alanının ilk bitidir.
        let sum = rows.iter().filter(|r| r.OperStatus == IfOperStatusUp && r.InterfaceAndOperStatusFlags._bitfield & 1 != 0).map(|r| r.InOctets).sum();
        FreeMibTable(t as _);
        sum
    }
}
