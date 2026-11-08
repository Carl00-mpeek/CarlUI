//! Bu modül "bu model bu bilgisayarda rahat çalışır mı?" sorusunu çözüyor.
//! Her model için önerilen sistem (RAM, ekran kartı belleği) burada yazılı;
//! bilgisayarın RAM'i ve en büyük ekran kartının kendi belleği okunup
//! karşılaştırılır. Altında kalırsa model yine çalışır, yalnızca uzun sürebilir;
//! kullanıcıya bu yazılır, engellenmez.

use crate::catalog::Model;
use std::sync::OnceLock;

/// Önerilen sistem.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Needs {
    pub ram_gb: u32,
    /// Önerilen ekran kartı belleği; 0 = işlemci yeterli.
    pub vram_gb: u32,
    /// Ekran kartını hiç kullanmaz (yalnızca işlemci).
    pub cpu_only: bool,
}

const fn n(ram_gb: u32, vram_gb: u32) -> Needs {
    Needs { ram_gb, vram_gb, cpu_only: false }
}
const fn cpu(ram_gb: u32) -> Needs {
    Needs { ram_gb, vram_gb: 0, cpu_only: true }
}

pub fn needs(m: &Model) -> Needs {
    match m.id {
        "whisper-base" | "cmd-whisper-tiny" | "cmd-whisper-base" => n(4, 0),
        "whisper-small" | "cmd-whisper-small" => n(8, 2),
        "whisper-turbo" | "cmd-whisper-turbo" => n(16, 4),
        "birefnet-tiny" => n(8, 2),
        "birefnet-general" | "birefnet-matting" => n(16, 4),
        "esrgan-general" => n(4, 0),
        "esrgan-x2" | "esrgan-anime" => n(8, 2),
        "esrgan-x4" => n(8, 4),
        "opus-en-tr" | "opus-tr-en" => n(8, 0),
        "kokoro-fp32" | "brain-qwen3-4b" => cpu(8),
        "piper-kristin" | "piper-dfki-tr" | "kokoro-q8" => cpu(4),
        _ => n(8, 0),
    }
}

/// Bu bilgisayar: (RAM GB, en büyük ekran kartı belleği GB).
pub fn system() -> (f32, f32) {
    static S: OnceLock<(f32, f32)> = OnceLock::new();
    *S.get_or_init(|| (ram(), vram()))
}

const GB: f32 = 1024.0 * 1024.0 * 1024.0;

fn ram() -> f32 {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut s = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    if unsafe { GlobalMemoryStatusEx(&mut s) }.is_ok() {
        s.ullTotalPhys as f32 / GB
    } else {
        0.0
    }
}

fn vram() -> f32 {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE};
    let Ok(f) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else { return 0.0 };
    let mut best = 0usize;
    for i in 0.. {
        let Ok(a) = (unsafe { f.EnumAdapters1(i) }) else { break };
        if let Ok(d) = unsafe { a.GetDesc1() } {
            if d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0 {
                best = best.max(d.DedicatedVideoMemory);
            }
        }
    }
    best as f32 / GB
}

/// "Önerilen: 8 GB RAM, 2 GB ekran kartı" gibi tek satır.
pub fn summary(m: &Model) -> String {
    let n = needs(m);
    let gpu = if n.cpu_only {
        t!("yalnızca işlemci", "CPU only").to_string()
    } else if n.vram_gb == 0 {
        t!("işlemci yeterli", "CPU is enough").to_string()
    } else {
        t!(format!("{} GB+ ekran kartı", n.vram_gb), format!("{} GB+ graphics card", n.vram_gb))
    };
    t!(format!("Önerilen sistem: {} GB RAM, {gpu}", n.ram_gb), format!("Recommended: {} GB RAM, {gpu}", n.ram_gb))
}

/// Bilgisayar önerinin altındaysa kısa uyarı (işlem yine yapılır, yalnızca uzun sürebilir).
pub fn warning(m: &Model) -> Option<String> {
    warning_for(needs(m), system(), crate::runtime::gpu_allowed())
}

fn warning_for(n: Needs, (ram, vram): (f32, f32), gpu_on: bool) -> Option<String> {
    // Windows toplam RAM'i biraz eksik bildirir (ör. 16 GB → 15,8); %10 pay bırakılır.
    let low_ram = ram > 0.0 && ram < n.ram_gb as f32 * 0.9;
    let wants_gpu = !n.cpu_only && n.vram_gb > 0;
    let low_vram = wants_gpu && gpu_on && vram < n.vram_gb as f32 * 0.9;
    if wants_gpu && !gpu_on {
        return Some(t!("Ekran kartı ayarlardan kapalı; işlem uzun sürebilir.", "The graphics card is turned off in settings; this may take a long time.").into());
    }
    if !low_ram && !low_vram {
        return None;
    }
    let have = if wants_gpu { t!(format!("{ram:.0} GB RAM, {vram:.0} GB ekran kartı"), format!("{ram:.0} GB RAM, {vram:.0} GB graphics card")) } else { format!("{ram:.0} GB RAM") };
    Some(t!(format!("Bu bilgisayar sınırda ({have}); işlem uzun sürebilir."), format!("This PC is below the recommendation ({have}); this may take a long time.")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_model_has_needs() {
        for m in crate::catalog::MODELS {
            let n = needs(m);
            assert!(n.ram_gb >= 4, "{}", m.id);
            assert!(!(n.cpu_only && n.vram_gb > 0), "{}", m.id);
        }
    }

    #[test]
    fn warns_when_below() {
        assert!(warning_for(n(8, 2), (16.0, 8.0), true).is_none());
        assert!(warning_for(n(8, 2), (15.8, 8.0), true).is_none());
        assert!(warning_for(n(16, 4), (8.0, 8.0), true).is_some());
        assert!(warning_for(n(8, 4), (16.0, 2.0), true).is_some());
        assert!(warning_for(n(8, 2), (16.0, 8.0), false).is_some());
        assert!(warning_for(cpu(4), (16.0, 0.0), false).is_none());
        assert!(warning_for(n(8, 0), (16.0, 0.0), true).is_none());
    }

    #[test]
    fn reads_this_pc() {
        let (ram, _) = system();
        assert!(ram > 1.0);
    }
}
