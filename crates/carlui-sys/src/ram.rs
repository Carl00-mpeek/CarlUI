//! Fiziksel bellek ve kayıt (commit) kullanımı.

use windows::Win32::System::{
    ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION},
    SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX},
};

#[derive(Clone, Copy, Default, Debug)]
pub struct Ram {
    pub total: f64,
    pub used: f64,
    /// Kayıtlı (commit) bellek ve sınırı.
    pub commit: f64,
    pub commit_limit: f64,
    /// Önbellek (bekleme listesi dahil).
    pub cached: f64,
}

impl Ram {
    pub fn percent(&self) -> f32 {
        if self.total > 0.0 { (self.used / self.total * 100.0) as f32 } else { 0.0 }
    }
}

pub fn read() -> Ram {
    unsafe {
        let mut ms = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
        let _ = GlobalMemoryStatusEx(&mut ms);
        let mut pi = PERFORMANCE_INFORMATION { cb: std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32, ..Default::default() };
        let _ = GetPerformanceInfo(&mut pi, pi.cb);
        let page = pi.PageSize as f64;
        Ram {
            total: ms.ullTotalPhys as f64,
            used: (ms.ullTotalPhys - ms.ullAvailPhys) as f64,
            commit: pi.CommitTotal as f64 * page,
            commit_limit: pi.CommitLimit as f64 * page,
            cached: pi.SystemCache as f64 * page,
        }
    }
}
