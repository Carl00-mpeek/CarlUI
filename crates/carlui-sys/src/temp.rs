//! CPU sıcaklığı. Intel işlemcide sıcaklık yalnızca çekirdeğin kendi model
//! kayıtlarında (MSR) durur ve bunlar ancak çekirdek sürücüsüyle okunur.
//! Burada imzalı, açık kaynak PawnIO sürücüsü kullanılır (LibreHardwareMonitor
//! ve FanControl'ün de kullandığı); eski WinRing0 gibi Windows'un engellediği
//! bir sürücü kullanılmaz.
//!
//! Okumayı yönetici olarak çalışan `carlui-sensors.exe` yapar ve sonucu
//! paylaşılan belleğe yazar; ada ve görev yöneticisi yetkisiz olarak oradan
//! okur (`read`). Sürücü ya da yardımcı yoksa `None` döner, arayüz "—" gösterir.

use std::mem::size_of;
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Security::{Authorization::*, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES},
        System::{
            LibraryLoader::{GetProcAddress, LoadLibraryW},
            Memory::*,
            SystemInformation::GetTickCount64,
            Threading::{GetCurrentThread, SetThreadAffinityMask},
        },
    },
};

pub const SHARED_NAME: PCWSTR = w!("Local\\CarlUI.Temps");
const MAGIC: u32 = 0x4341_5254;
const MAX: usize = 64;

#[derive(Clone, Debug, Default)]
pub struct Temps {
    pub package: f32,
    /// Fiziksel çekirdek başına (çekirdek numarası sırasıyla).
    pub cores: Vec<f32>,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Shared {
    magic: u32,
    count: u32,
    tick: u64,
    package: f32,
    cores: [f32; MAX],
}

const PAWNIO_DLL: &str = "C:\\Program Files\\PawnIO\\PawnIOLib.dll";

/// PawnIO kurulu mu (okuyucunun yükleyeceği kitaplık yerinde mi)?
pub fn driver_installed() -> bool {
    std::path::Path::new(PAWNIO_DLL).exists()
}

/// İşlemci Intel mi? Okuyucu yalnızca Intel'in sıcaklık kayıtlarını bilir.
pub fn intel() -> bool {
    #[allow(unused_unsafe)]
    let id = unsafe { std::arch::x86_64::__cpuid(0) };
    (id.ebx, id.edx, id.ecx) == (0x756E_6547, 0x4965_6E69, 0x6C65_746E) // "GenuineIntel"
}

/// Paylaşılan bellekten son ölçüm (5 sn'den eskiyse yok sayılır).
pub fn read() -> Option<Temps> {
    unsafe {
        let h = OpenFileMappingW(FILE_MAP_READ.0, false, SHARED_NAME).ok()?;
        let v = MapViewOfFile(h, FILE_MAP_READ, 0, 0, size_of::<Shared>());
        let out = if v.Value.is_null() {
            None
        } else {
            let s = *(v.Value as *const Shared);
            let _ = UnmapViewOfFile(v);
            (s.magic == MAGIC && GetTickCount64().saturating_sub(s.tick) < 5000 && s.package > 0.0)
                .then(|| Temps { package: s.package, cores: s.cores[..(s.count as usize).min(MAX)].to_vec() })
        };
        let _ = CloseHandle(h);
        out
    }
}

/// Yönetici tarafı: ölçümü paylaşılan belleğe yazar.
pub struct Publisher {
    map: HANDLE,
    view: MEMORY_MAPPED_VIEW_ADDRESS,
}

impl Publisher {
    pub fn new() -> Option<Self> {
        unsafe {
            // Yetkisiz süreçler okuyabilsin: herkes okur, bütünlük etiketi düşük.
            let mut sd = PSECURITY_DESCRIPTOR::default();
            ConvertStringSecurityDescriptorToSecurityDescriptorW(w!("D:(A;;GR;;;WD)(A;;GA;;;BA)(A;;GA;;;SY)S:(ML;;NW;;;LW)"), SDDL_REVISION_1, &mut sd, None).ok()?;
            let sa = SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: sd.0, bInheritHandle: false.into() };
            let map = CreateFileMappingW(INVALID_HANDLE_VALUE, Some(&sa), PAGE_READWRITE, 0, size_of::<Shared>() as u32, SHARED_NAME);
            let _ = LocalFree(HLOCAL(sd.0));
            let map = map.ok()?;
            let view = MapViewOfFile(map, FILE_MAP_WRITE, 0, 0, size_of::<Shared>());
            if view.Value.is_null() {
                let _ = CloseHandle(map);
                return None;
            }
            Some(Self { map, view })
        }
    }

    pub fn publish(&self, t: &Temps) {
        let mut s = Shared { magic: MAGIC, count: t.cores.len().min(MAX) as u32, tick: unsafe { GetTickCount64() }, package: t.package, cores: [0.0; MAX] };
        s.cores[..s.count as usize].copy_from_slice(&t.cores[..s.count as usize]);
        unsafe {
            std::ptr::write_volatile(self.view.Value as *mut Shared, s);
        }
    }
}

impl Drop for Publisher {
    fn drop(&mut self) {
        unsafe {
            let _ = UnmapViewOfFile(self.view);
            let _ = CloseHandle(self.map);
        }
    }
}

type PioOpen = unsafe extern "system" fn(*mut HANDLE) -> HRESULT;
type PioLoad = unsafe extern "system" fn(HANDLE, *const u8, usize) -> HRESULT;
type PioExec = unsafe extern "system" fn(HANDLE, PCSTR, *const u64, usize, *mut u64, usize, *mut usize) -> HRESULT;

/// PawnIO sürücüsü + Intel MSR modülüyle doğrudan okuyucu (yönetici gerekir).
pub struct Msr {
    h: HANDLE,
    exec: PioExec,
    tjmax: f32,
    /// Fiziksel çekirdek başına okunacak mantıksal işlemci.
    probes: Vec<u32>,
}

unsafe impl Send for Msr {}

const IA32_THERM_STATUS: u64 = 0x19C;
const IA32_PACKAGE_THERM_STATUS: u64 = 0x1B1;
const MSR_TEMPERATURE_TARGET: u64 = 0x1A2;

impl Msr {
    /// `module`: PawnIO'nun imzalı IntelMSR modülü (IntelMSR.bin).
    pub fn open(module: &[u8]) -> std::result::Result<Self, String> {
        unsafe {
            let lib = LoadLibraryW(&HSTRING::from(PAWNIO_DLL)).map_err(|_| t!("PawnIO kurulu değil", "PawnIO is not installed").to_string())?;
            let get = |n: PCSTR| GetProcAddress(lib, n).ok_or_else(|| "PawnIOLib eksik".to_string());
            let open: PioOpen = std::mem::transmute(get(s!("pawnio_open"))?);
            let load: PioLoad = std::mem::transmute(get(s!("pawnio_load"))?);
            let exec: PioExec = std::mem::transmute(get(s!("pawnio_execute"))?);
            let mut h = HANDLE::default();
            open(&mut h).ok().map_err(|e| format!("PawnIO açılamadı: {e}"))?;
            load(h, module.as_ptr(), module.len()).ok().map_err(|e| format!("MSR modülü yüklenemedi: {e}"))?;
            let mut probes: Vec<(u32, u32)> = crate::cpu::Cpu::new().kinds().iter().enumerate().map(|(i, k)| (k.0, i as u32)).collect();
            probes.sort();
            probes.dedup_by_key(|p| p.0);
            let mut me = Self { h, exec, tjmax: 100.0, probes: probes.into_iter().map(|p| p.1).collect() };
            if let Some(v) = me.read_msr(MSR_TEMPERATURE_TARGET) {
                let t = ((v >> 16) & 0xFF) as f32;
                if (60.0..=130.0).contains(&t) {
                    me.tjmax = t;
                }
            }
            Ok(me)
        }
    }

    fn read_msr(&self, msr: u64) -> Option<u64> {
        let mut out = 0u64;
        let mut n = 0usize;
        unsafe { (self.exec)(self.h, s!("ioctl_read_msr"), &msr, 1, &mut out, 1, &mut n).ok().ok().map(|_| out) }
    }

    fn on_cpu(&self, cpu: u32, msr: u64) -> Option<u64> {
        unsafe {
            let th = GetCurrentThread();
            let old = SetThreadAffinityMask(th, 1usize << cpu);
            let v = self.read_msr(msr);
            if old != 0 {
                SetThreadAffinityMask(th, old);
            }
            v
        }
    }

    fn decode(&self, v: u64) -> Option<f32> {
        // Bit 31: okuma geçerli; 22:16 TjMax'a uzaklık.
        (v & (1 << 31) != 0).then(|| self.tjmax - ((v >> 16) & 0x7F) as f32)
    }

    pub fn read(&self) -> Option<Temps> {
        let cores: Vec<f32> = self.probes.iter().map(|&c| self.on_cpu(c, IA32_THERM_STATUS).and_then(|v| self.decode(v)).unwrap_or(0.0)).collect();
        let package = self.on_cpu(0, IA32_PACKAGE_THERM_STATUS).and_then(|v| self.decode(v)).unwrap_or_else(|| cores.iter().copied().fold(0.0, f32::max));
        (package > 0.0).then_some(Temps { package, cores })
    }
}

impl Drop for Msr {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.h);
        }
    }
}
