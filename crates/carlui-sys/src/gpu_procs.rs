//! Süreç başına GPU kullanımı: "GPU Engine(*)\Utilization Percentage"
//! sayaçları. Örnek adı "pid_1234_luid_0x…_phys_0_eng_3_engtype_3D" biçimindedir;
//! Windows Görev Yöneticisi gibi, her süreç için motor türü (3D, Copy,
//! VideoDecode …) başına toplanır ve en yükseği alınır. Yönetici gerekmez,
//! tüm ekran kartlarında (NVIDIA, AMD, Intel) çalışır.

use std::collections::HashMap;
use windows::{core::*, Win32::System::Performance::*};

pub struct Meter {
    query: isize,
    util: isize,
}

impl Meter {
    pub fn new() -> Self {
        let mut m = Self { query: 0, util: 0 };
        unsafe {
            if PdhOpenQueryW(None, 0, &mut m.query) == 0 {
                let _ = PdhAddEnglishCounterW(m.query, h!("\\GPU Engine(*)\\Utilization Percentage"), 0, &mut m.util);
                let _ = PdhCollectQueryData(m.query);
            }
        }
        m
    }

    /// pid → GPU yüzdesi (0..100). Sıfır olanlar listede yok.
    pub fn read(&mut self) -> HashMap<u32, f32> {
        let mut per: HashMap<(u32, String), f64> = HashMap::new();
        if self.util == 0 {
            return HashMap::new();
        }
        unsafe {
            let _ = PdhCollectQueryData(self.query);
            let fmt = PDH_FMT(PDH_FMT_DOUBLE.0 | 0x8000);
            let (mut size, mut count) = (0u32, 0u32);
            let _ = PdhGetFormattedCounterArrayW(self.util, fmt, &mut size, &mut count, None);
            if size == 0 {
                return HashMap::new();
            }
            let mut buf = vec![0u8; size as usize];
            if PdhGetFormattedCounterArrayW(self.util, fmt, &mut size, &mut count, Some(buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W)) != 0 {
                return HashMap::new();
            }
            let items = std::slice::from_raw_parts(buf.as_ptr() as *const PDH_FMT_COUNTERVALUE_ITEM_W, count as usize);
            for it in items {
                let v = it.FmtValue.Anonymous.doubleValue;
                if it.FmtValue.CStatus != 0 || v <= 0.0 {
                    continue;
                }
                let name = it.szName.to_string().unwrap_or_default();
                let Some(pid) = name.strip_prefix("pid_").and_then(|s| s.split('_').next()).and_then(|s| s.parse::<u32>().ok()) else { continue };
                let ty = name.rsplit("engtype_").next().unwrap_or("").to_string();
                *per.entry((pid, ty)).or_default() += v;
            }
        }
        let mut out: HashMap<u32, f32> = HashMap::new();
        for ((pid, _), v) in per {
            let e = out.entry(pid).or_default();
            *e = e.max(v.min(100.0) as f32);
        }
        out
    }
}

impl Default for Meter {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Meter {
    fn drop(&mut self) {
        unsafe {
            let _ = PdhCloseQuery(self.query);
        }
    }
}
