//! NVIDIA ekran kartı: sürücüyle gelen `nvml.dll` üzerinden okunur (yönetici
//! gerekmez, sürücü yoksa sessizce `None`). Kütüphane bir kez yüklenir.

use std::sync::OnceLock;
use windows::{
    core::*,
    Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW},
};

#[derive(Clone, Default, Debug)]
pub struct Gpu {
    pub name: String,
    pub temp: f32,
    /// Grafik çekirdeği yükü (%).
    pub load: f32,
    pub vram_used: f64,
    pub vram_total: f64,
    pub clock_mhz: u32,
    pub mem_clock_mhz: u32,
    pub power_w: f32,
    pub fan: Option<u32>,
}

type Handle = *mut core::ffi::c_void;

#[repr(C)]
#[derive(Default)]
struct Util {
    gpu: u32,
    memory: u32,
}

#[repr(C)]
#[derive(Default)]
struct MemInfo {
    total: u64,
    free: u64,
    used: u64,
}

struct Nvml {
    dev: usize,
    temp: unsafe extern "C" fn(Handle, u32, *mut u32) -> i32,
    util: unsafe extern "C" fn(Handle, *mut Util) -> i32,
    mem: unsafe extern "C" fn(Handle, *mut MemInfo) -> i32,
    clock: unsafe extern "C" fn(Handle, u32, *mut u32) -> i32,
    power: unsafe extern "C" fn(Handle, *mut u32) -> i32,
    fan: unsafe extern "C" fn(Handle, *mut u32) -> i32,
    name: String,
}

unsafe impl Send for Nvml {}
unsafe impl Sync for Nvml {}

fn load() -> Option<Nvml> {
    unsafe {
        let lib = LoadLibraryW(w!("nvml.dll")).ok()?;
        macro_rules! f {
            ($n:literal) => {
                std::mem::transmute(GetProcAddress(lib, s!($n))?)
            };
        }
        let init: unsafe extern "C" fn() -> i32 = f!("nvmlInit_v2");
        if init() != 0 {
            return None;
        }
        let count: unsafe extern "C" fn(*mut u32) -> i32 = f!("nvmlDeviceGetCount_v2");
        let mut n = 0;
        if count(&mut n) != 0 || n == 0 {
            return None;
        }
        let by_index: unsafe extern "C" fn(u32, *mut Handle) -> i32 = f!("nvmlDeviceGetHandleByIndex_v2");
        let mut dev: Handle = std::ptr::null_mut();
        if by_index(0, &mut dev) != 0 {
            return None;
        }
        let get_name: unsafe extern "C" fn(Handle, *mut u8, u32) -> i32 = f!("nvmlDeviceGetName");
        let mut buf = [0u8; 96];
        let name = if get_name(dev, buf.as_mut_ptr(), buf.len() as u32) == 0 {
            let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            String::from_utf8_lossy(&buf[..end]).trim_start_matches("NVIDIA ").to_string()
        } else {
            "NVIDIA".into()
        };
        Some(Nvml {
            dev: dev as usize,
            temp: f!("nvmlDeviceGetTemperature"),
            util: f!("nvmlDeviceGetUtilizationRates"),
            mem: f!("nvmlDeviceGetMemoryInfo"),
            clock: f!("nvmlDeviceGetClockInfo"),
            power: f!("nvmlDeviceGetPowerUsage"),
            fan: f!("nvmlDeviceGetFanSpeed"),
            name,
        })
    }
}

static NVML: OnceLock<Option<Nvml>> = OnceLock::new();

/// Birinci NVIDIA kartın anlık durumu.
pub fn read() -> Option<Gpu> {
    let n = NVML.get_or_init(load).as_ref()?;
    let d = n.dev as Handle;
    unsafe {
        let mut t = 0;
        if (n.temp)(d, 0, &mut t) != 0 {
            return None;
        }
        let mut u = Util::default();
        let _ = (n.util)(d, &mut u);
        let mut m = MemInfo::default();
        let _ = (n.mem)(d, &mut m);
        let (mut c, mut mc, mut p, mut fan) = (0, 0, 0, 0);
        let _ = (n.clock)(d, 0, &mut c);
        let _ = (n.clock)(d, 2, &mut mc);
        let _ = (n.power)(d, &mut p);
        let fan_ok = (n.fan)(d, &mut fan) == 0;
        Some(Gpu {
            name: n.name.clone(),
            temp: t as f32,
            load: u.gpu as f32,
            vram_used: m.used as f64,
            vram_total: m.total as f64,
            clock_mhz: c,
            mem_clock_mhz: mc,
            power_w: p as f32 / 1000.0,
            fan: fan_ok.then_some(fan),
        })
    }
}
