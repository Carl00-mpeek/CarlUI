//! Bu modül "kablosuz kulaklığın şarjı ne kadar kaldı?" sorununu şu yöntemle
//! çözüyor: Windows, pil bilgisini bildiren Bluetooth aygıtları (kulaklık,
//! kulak içi, oyun kumandası) için aygıt yöneticisinde bir pil özelliği
//! tutar (`DEVPKEY_Bluetooth_Battery`, Ayarlar > Bluetooth'taki yüzdeyle
//! aynı). Bağlı aygıtlar bu özellikle taranır. Tarama birkaç on ms sürdüğü
//! için sonuç 30 sn önbellekte tutulur.

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use windows::{
    core::GUID,
    Win32::Devices::{DeviceAndDriverInstallation::*, Properties::*},
};

const BATTERY: DEVPROPKEY = DEVPROPKEY { fmtid: GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5), pid: 2 };

static CACHE: Mutex<Option<(Instant, Vec<(String, u8)>)>> = Mutex::new(None);

fn prop(set: HDEVINFO, dev: &SP_DEVINFO_DATA, key: &DEVPROPKEY, buf: &mut [u8]) -> Option<(DEVPROPTYPE, usize)> {
    let mut ty = DEVPROPTYPE::default();
    let mut need = 0u32;
    unsafe { SetupDiGetDevicePropertyW(set, dev, key, &mut ty, Some(buf), Some(&mut need), 0).ok()? };
    Some((ty, need as usize))
}

fn scan() -> Vec<(String, u8)> {
    let mut out = Vec::new();
    unsafe {
        let Ok(set) = SetupDiGetClassDevsW(None, None, None, DIGCF_PRESENT | DIGCF_ALLCLASSES) else { return out };
        let mut i = 0;
        loop {
            let mut dev = SP_DEVINFO_DATA { cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32, ..Default::default() };
            if SetupDiEnumDeviceInfo(set, i, &mut dev).is_err() {
                break;
            }
            i += 1;
            let mut b = [0u8; 4];
            let Some((ty, _)) = prop(set, &dev, &BATTERY, &mut b) else { continue };
            if ty != DEVPROP_TYPE_BYTE || b[0] > 100 {
                continue;
            }
            let mut nb = [0u8; 512];
            let name = match prop(set, &dev, &DEVPKEY_Device_FriendlyName, &mut nb) {
                Some((_, n)) => {
                    let w: Vec<u16> = nb[..n.min(nb.len())].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
                    String::from_utf16_lossy(&w)
                }
                None => String::new(),
            };
            // Aynı aygıt birden çok profilde (eller serbest, ses) görünür: tek kez.
            let short = name.trim_end_matches(" Hands-Free").trim_end_matches(" Hands-Free AG").trim_end_matches(" Stereo").to_string();
            if !out.iter().any(|(n, _)| *n == short) {
                out.push((short, b[0]));
            }
        }
        let _ = SetupDiDestroyDeviceInfoList(set);
    }
    out
}

/// Pil bildiren bağlı Bluetooth aygıtları: (ad, yüzde).
pub fn batteries() -> Vec<(String, u8)> {
    let mut g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if g.as_ref().is_none_or(|(t, _)| t.elapsed() > Duration::from_secs(30)) {
        *g = Some((Instant::now(), scan()));
    }
    g.as_ref().map(|(_, v)| v.clone()).unwrap_or_default()
}
