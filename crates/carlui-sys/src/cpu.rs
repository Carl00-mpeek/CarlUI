//! İşlemci: mantıksal çekirdek başına kullanım çekirdeğin kendi zaman
//! sayaçlarından (NtQuerySystemInformation) iki ölçüm arasındaki farkla
//! hesaplanır; GHz ise "% Processor Performance" sayacı × taban frekansla.
//! Intel hibrit işlemcilerde hangi çekirdeğin P (performans), hangisinin E
//! (verimlilik) olduğu CPU Set bilgisinden okunur.

use std::mem::size_of;
use windows::{
    core::*,
    Win32::{
        System::{
            LibraryLoader::{GetModuleHandleW, GetProcAddress},
            Performance::*,
            Power::{CallNtPowerInformation, ProcessorInformation},
            SystemInformation::*,
            
        },
    },
};

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct PerfInfo {
    idle: i64,
    kernel: i64,
    user: i64,
    dpc: i64,
    interrupt: i64,
    count: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct PowerInfo {
    number: u32,
    max_mhz: u32,
    current_mhz: u32,
    limit_mhz: u32,
    max_idle: u32,
    current_idle: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Performance,
    Efficient,
}

#[derive(Clone, Debug)]
pub struct Core {
    /// Mantıksal işlemci numarası.
    pub index: u32,
    /// Fiziksel çekirdek (aynı çekirdeğin iki iş parçacığı aynı numarayı taşır).
    pub core: u32,
    pub kind: Kind,
    pub usage: f32,
    pub mhz: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Sample {
    pub usage: f32,
    pub mhz: f32,
    pub cores: Vec<Core>,
}

pub struct Cpu {
    prev: Vec<PerfInfo>,
    base_mhz: Vec<u32>,
    layout: Vec<(u32, Kind)>,
    query: isize,
    perf: Vec<isize>,
    perf_total: isize,
    pub name: String,
    pub physical: u32,
}

type NtQsi = unsafe extern "system" fn(u32, *mut core::ffi::c_void, u32, *mut u32) -> i32;

fn ntqsi() -> Option<NtQsi> {
    unsafe { GetProcAddress(GetModuleHandleW(w!("ntdll.dll")).ok()?, s!("NtQuerySystemInformation")).map(|f| std::mem::transmute(f)) }
}

fn times(n: usize) -> Vec<PerfInfo> {
    let mut v = vec![PerfInfo::default(); n];
    if let Some(f) = ntqsi() {
        let mut ret = 0;
        unsafe {
            f(8, v.as_mut_ptr() as *mut _, (n * size_of::<PerfInfo>()) as u32, &mut ret);
        }
    }
    v
}

pub fn logical_count() -> usize {
    unsafe {
        let mut si = SYSTEM_INFO::default();
        GetSystemInfo(&mut si);
        si.dwNumberOfProcessors as usize
    }
}

/// Windows'un CPU Set kaydı: süreçleri belirli çekirdeklere yönlendirmek
/// (SetProcessDefaultCpuSets) bu kimliklerle yapılır.
#[derive(Clone, Copy, Debug)]
pub struct CpuSet {
    pub id: u32,
    /// Mantıksal işlemci numarası.
    pub index: u32,
    /// Fiziksel çekirdek.
    pub core: u32,
    pub kind: Kind,
}

pub fn cpu_sets() -> Vec<CpuSet> {
    let mut out = Vec::new();
    unsafe {
        let mut len = 0;
        let _ = GetSystemCpuSetInformation(None, 0, &mut len, None, 0);
        let mut buf = vec![0u8; len as usize];
        if GetSystemCpuSetInformation(Some(buf.as_mut_ptr() as *mut _), len, &mut len, None, 0).as_bool() {
            let mut off = 0usize;
            let mut max_class = 0u8;
            let mut items = Vec::new();
            while off + 8 <= buf.len() {
                let size = u32::from_ne_bytes(buf[off..off + 4].try_into().unwrap()) as usize;
                if size == 0 {
                    break;
                }
                // SYSTEM_CPU_SET_INFORMATION.CpuSet: Id(4) Group(2) LogicalProcessorIndex(1)
                // CoreIndex(1) LastLevelCacheIndex(1) NumaNodeIndex(1) EfficiencyClass(1)
                let b = &buf[off + 8..off + size];
                let id = u32::from_ne_bytes(b[0..4].try_into().unwrap());
                let (lp, core, class) = (b[6] as u32, b[7] as u32, b[10]);
                max_class = max_class.max(class);
                items.push((id, lp, core, class));
                off += size;
            }
            // En yüksek verimlilik sınıfı = en güçlü çekirdek (P).
            out = items.into_iter().map(|(id, index, core, class)| CpuSet { id, index, core, kind: if class == max_class { Kind::Performance } else { Kind::Efficient } }).collect();
        }
    }
    out
}

/// (fiziksel çekirdek, tür) — mantıksal işlemci sırasıyla.
fn layout(n: usize) -> Vec<(u32, Kind)> {
    let mut out = vec![(0u32, Kind::Performance); n];
    for s in cpu_sets() {
        if (s.index as usize) < n {
            out[s.index as usize] = (s.core, s.kind);
        }
    }
    out
}

fn cpu_name() -> String {
    use windows::Win32::System::Registry::*;
    unsafe {
        let mut buf = [0u16; 128];
        let mut len = (buf.len() * 2) as u32;
        if RegGetValueW(HKEY_LOCAL_MACHINE, w!("HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0"), w!("ProcessorNameString"), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut len)).is_ok() {
            let s = String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]);
            return s.replace("(R)", "").replace("(TM)", "").replace("13th Gen ", "").replace("  ", " ").trim().to_string();
        }
    }
    t!("İşlemci", "Processor").into()
}

impl Cpu {
    pub fn new() -> Self {
        let n = logical_count();
        let mut base = vec![PowerInfo::default(); n];
        unsafe {
            let _ = CallNtPowerInformation(ProcessorInformation, None, 0, Some(base.as_mut_ptr() as *mut _), (n * size_of::<PowerInfo>()) as u32);
        }
        let layout = layout(n);
        let mut physical: Vec<u32> = layout.iter().map(|l| l.0).collect();
        physical.sort();
        physical.dedup();
        let mut query = 0;
        let mut perf = Vec::new();
        let mut perf_total = 0;
        unsafe {
            if PdhOpenQueryW(None, 0, &mut query) == 0 {
                for i in 0..n {
                    let mut c = 0;
                    let path = HSTRING::from(format!("\\Processor Information(0,{i})\\% Processor Performance"));
                    let _ = PdhAddEnglishCounterW(query, &path, 0, &mut c);
                    perf.push(c);
                }
                let _ = PdhAddEnglishCounterW(query, h!("\\Processor Information(_Total)\\% Processor Performance"), 0, &mut perf_total);
                let _ = PdhCollectQueryData(query);
            }
        }
        Self {
            prev: times(n),
            base_mhz: base.iter().map(|b| b.max_mhz).collect(),
            layout,
            query,
            perf,
            perf_total,
            name: cpu_name(),
            physical: physical.len() as u32,
        }
    }

    /// Önceki çağrıdan bu yana kullanım (ilk çağrı kurulumdan bu yana).
    pub fn sample(&mut self) -> Sample {
        let n = self.prev.len();
        let now = times(n);
        let mut cores = Vec::with_capacity(n);
        let (mut busy_all, mut total_all) = (0i64, 0i64);
        let fmt = |c: isize| -> f64 {
            let mut v = PDH_FMT_COUNTERVALUE::default();
            if c != 0 && unsafe { PdhGetFormattedCounterValue(c, PDH_FMT(PDH_FMT_DOUBLE.0 | 0x8000), None, &mut v) } == 0 {
                unsafe { v.Anonymous.doubleValue }
            } else {
                0.0
            }
        };
        unsafe {
            let _ = PdhCollectQueryData(self.query);
        }
        for i in 0..n {
            let (a, b) = (self.prev[i], now[i]);
            let total = (b.kernel - a.kernel) + (b.user - a.user);
            let idle = b.idle - a.idle;
            let busy = (total - idle).max(0);
            busy_all += busy;
            total_all += total;
            let usage = if total > 0 { busy as f32 / total as f32 * 100.0 } else { 0.0 };
            let perf = self.perf.get(i).map(|&c| fmt(c)).unwrap_or(0.0);
            let base = self.base_mhz.get(i).copied().unwrap_or(0) as f64;
            let (core, kind) = self.layout.get(i).copied().unwrap_or((i as u32, Kind::Performance));
            cores.push(Core { index: i as u32, core, kind, usage: usage.clamp(0.0, 100.0), mhz: (perf / 100.0 * base) as f32 });
        }
        self.prev = now;
        let base = self.base_mhz.iter().copied().max().unwrap_or(0) as f64;
        let mut mhz = (fmt(self.perf_total) / 100.0 * base) as f32;
        if mhz <= 0.0 {
            mhz = cores.iter().map(|c| c.mhz).fold(0.0, f32::max);
        }
        Sample { usage: if total_all > 0 { (busy_all as f32 / total_all as f32 * 100.0).clamp(0.0, 100.0) } else { 0.0 }, mhz, cores }
    }

    pub fn kinds(&self) -> &[(u32, Kind)] {
        &self.layout
    }
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Cpu {
    fn drop(&mut self) {
        unsafe {
            let _ = PdhCloseQuery(self.query);
        }
    }
}
