//! Bu modül "CarlUI ne kadar bellek kullanıyor?" sorusunu çözüyor: sürecin
//! çalışma kümesi tanı dökümlerinde gösterilir.
//!
//! Not: Eskiden oyunda modüller uyutuluyordu (EcoQoS, arka plan önceliği,
//! belleği boşaltma). Kazancı birkaç on MB'tı; tam ekran algısı her gidip
//! geldiğinde sayfaların geri yüklenmesi dock'u ve adayı dondurduğu için
//! kaldırıldı.

use windows::Win32::System::{ProcessStatus::K32GetProcessMemoryInfo, ProcessStatus::PROCESS_MEMORY_COUNTERS, Threading::GetCurrentProcess};

/// Kullanılan bellek (MB, çalışma kümesi).
pub fn working_set_mb() -> f64 {
    unsafe {
        let mut c = PROCESS_MEMORY_COUNTERS { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
        if K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb).as_bool() {
            c.WorkingSetSize as f64 / 1024.0 / 1024.0
        } else {
            0.0
        }
    }
}
