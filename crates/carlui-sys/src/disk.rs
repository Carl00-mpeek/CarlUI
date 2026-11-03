//! Diskler: her fiziksel disk erişim hakkı istemeden (0 erişimle) açılır;
//! bu, yönetici olmadan şunlara yeter:
//! - model ve bağlantı türü (StorageDeviceProperty),
//! - sıcaklık (StorageDeviceTemperatureProperty),
//! - NVMe sağlık kaydı: yıpranma yüzdesi, yedek alan, çalışma saati, yazılan veri.
//! Etkinlik ve okuma/yazma hızı "PhysicalDisk" performans sayaçlarından,
//! doluluk diskteki bölümlerin (C:, D: …) boş alanından hesaplanır.

use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Storage::FileSystem::*,
        System::{Ioctl::*, Performance::*, IO::DeviceIoControl},
    },
};

#[derive(Clone, Debug, Default)]
pub struct Health {
    /// Ömrün kullanılan yüzdesi (NVMe "Percentage Used").
    pub used_pct: u8,
    pub spare_pct: u8,
    pub hours: u64,
    pub written_bytes: f64,
    pub critical: u8,
}

#[derive(Clone, Debug, Default)]
pub struct Disk {
    pub index: u32,
    pub model: String,
    pub bus: String,
    pub ssd: bool,
    pub volumes: Vec<String>,
    pub total: f64,
    pub free: f64,
    pub temp: Option<f32>,
    pub health: Option<Health>,
    /// 0..100 etkinlik.
    pub active: f32,
    pub read_bps: f64,
    pub write_bps: f64,
}

impl Disk {
    pub fn used_pct(&self) -> f32 {
        if self.total > 0.0 { ((self.total - self.free) / self.total * 100.0) as f32 } else { 0.0 }
    }

    /// "İyi" / "Dikkat" / "Kötü" + kısa açıklama.
    pub fn verdict(&self) -> (&'static str, String) {
        match &self.health {
            Some(h) if h.critical != 0 => ("Kötü", "Disk kritik uyarı veriyor".into()),
            Some(h) if h.used_pct >= 90 || h.spare_pct < 10 => ("Dikkat", format!("Ömrün %{}'i kullanıldı", h.used_pct)),
            Some(h) => ("İyi", format!("Kalan ömür %{}", 100u8.saturating_sub(h.used_pct))),
            None => ("İyi", "Windows sorun bildirmiyor".into()),
        }
    }
}

fn open(n: u32) -> Option<HANDLE> {
    let name = HSTRING::from(format!("\\\\.\\PhysicalDrive{n}"));
    unsafe { CreateFileW(&name, 0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None).ok() }
}

fn query(h: HANDLE, input: &[u8], out: &mut [u8]) -> bool {
    let mut r = 0;
    unsafe { DeviceIoControl(h, IOCTL_STORAGE_QUERY_PROPERTY, Some(input.as_ptr() as _), input.len() as u32, Some(out.as_mut_ptr() as _), out.len() as u32, Some(&mut r), None).is_ok() }
}

fn prop(id: i32) -> [u8; 12] {
    let mut q = [0u8; 12];
    q[..4].copy_from_slice(&id.to_ne_bytes());
    q
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_ne_bytes(b[o..o + 4].try_into().unwrap())
}

fn cstr(b: &[u8], o: usize) -> String {
    if o == 0 || o >= b.len() {
        return String::new();
    }
    let end = b[o..].iter().position(|&c| c == 0).map(|e| o + e).unwrap_or(b.len());
    String::from_utf8_lossy(&b[o..end]).trim().to_string()
}

fn describe(h: HANDLE) -> (String, String, bool) {
    let mut out = [0u8; 1024];
    if !query(h, &prop(0), &mut out) {
        return ("Disk".into(), String::new(), false);
    }
    // STORAGE_DEVICE_DESCRIPTOR: … VendorIdOffset@12, ProductIdOffset@16, BusType@28
    let vendor = cstr(&out, u32_at(&out, 12) as usize);
    let product = cstr(&out, u32_at(&out, 16) as usize);
    let bus = match u32_at(&out, 28) {
        7 => "USB",
        11 => "SATA",
        17 => "NVMe",
        8 => "RAID",
        10 => "SAS",
        12 => "SD",
        _ => "",
    };
    let model = if vendor.is_empty() || product.starts_with(&vendor) { product } else { format!("{vendor} {product}") };
    // Dönme gecikmesi yoksa SSD'dir (StorageDeviceSeekPenaltyProperty).
    let mut sp = [0u8; 16];
    let ssd = bus == "NVMe" || (query(h, &prop(7), &mut sp) && sp[8] == 0);
    (model, bus.into(), ssd)
}

fn temperature(h: HANDLE) -> Option<f32> {
    let mut out = [0u8; 512];
    if !query(h, &prop(52), &mut out) {
        return None;
    }
    // STORAGE_TEMPERATURE_DATA_DESCRIPTOR başlığı 24 bayt; ilk kaydın sıcaklığı +2'de.
    let t = i16::from_ne_bytes([out[26], out[27]]);
    (1..120).contains(&t).then_some(t as f32)
}

fn nvme_health(h: HANDLE) -> Option<(Health, f32)> {
    // STORAGE_PROPERTY_QUERY (8) + STORAGE_PROTOCOL_SPECIFIC_DATA (40) + 512 bayt kayıt.
    let mut q = vec![0u8; 8 + 40 + 512];
    q[0..4].copy_from_slice(&50i32.to_ne_bytes()); // StorageDeviceProtocolSpecificProperty
    q[8..12].copy_from_slice(&3u32.to_ne_bytes()); // ProtocolTypeNvme
    q[12..16].copy_from_slice(&2u32.to_ne_bytes()); // NVMeDataTypeLogPage
    q[16..20].copy_from_slice(&2u32.to_ne_bytes()); // SMART / Health Information
    q[20..24].copy_from_slice(&0xFFFF_FFFFu32.to_ne_bytes());
    q[24..28].copy_from_slice(&40u32.to_ne_bytes());
    q[28..32].copy_from_slice(&512u32.to_ne_bytes());
    let mut out = vec![0u8; q.len()];
    if !query(h, &q, &mut out) {
        return None;
    }
    let b = &out[48..];
    let kelvin = u16::from_ne_bytes([b[1], b[2]]) as f32;
    let units = u64::from_ne_bytes(b[48..56].try_into().unwrap());
    let health = Health {
        critical: b[0],
        spare_pct: b[3],
        used_pct: b[5],
        written_bytes: units as f64 * 512_000.0,
        hours: u64::from_ne_bytes(b[128..136].try_into().unwrap()),
    };
    Some((health, kelvin - 273.15))
}

/// Bölümlerin (C:, D: …) hangi diske ait olduğu ve boyutları.
fn volumes() -> Vec<(u32, String, f64, f64)> {
    let mut out = Vec::new();
    let mask = unsafe { GetLogicalDrives() };
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let root = HSTRING::from(format!("{letter}:\\"));
        if unsafe { GetDriveTypeW(&root) } != 3 && unsafe { GetDriveTypeW(&root) } != 2 {
            continue;
        }
        let path = HSTRING::from(format!("\\\\.\\{letter}:"));
        let Ok(h) = (unsafe { CreateFileW(&path, 0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None) }) else { continue };
        let mut ext = [0u8; 64];
        let mut r = 0;
        let ok = unsafe { DeviceIoControl(h, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, None, 0, Some(ext.as_mut_ptr() as _), ext.len() as u32, Some(&mut r), None).is_ok() };
        unsafe {
            let _ = CloseHandle(h);
        }
        if !ok {
            continue;
        }
        let disk = u32_at(&ext, 8);
        let (mut free, mut total) = (0u64, 0u64);
        unsafe {
            let _ = GetDiskFreeSpaceExW(&root, None, Some(&mut total), Some(&mut free));
        }
        out.push((disk, format!("{letter}:"), total as f64, free as f64));
    }
    out
}

/// Diskleri ve donanım bilgisini bulur (bir kez; etkinlik `Meter` ile).
pub fn list() -> Vec<Disk> {
    let vols = volumes();
    let mut out = Vec::new();
    for n in 0..16 {
        let Some(h) = open(n) else { continue };
        let (model, bus, ssd) = describe(h);
        let mut d = Disk { index: n, model, bus, ssd, ..Default::default() };
        if let Some((hl, t)) = nvme_health(h) {
            d.health = Some(hl);
            d.temp = (t > 0.0 && t < 120.0).then_some(t);
        }
        if d.temp.is_none() {
            d.temp = temperature(h);
        }
        unsafe {
            let _ = CloseHandle(h);
        }
        for (disk, letter, total, free) in &vols {
            if *disk == n {
                d.volumes.push(letter.clone());
                d.total += total;
                d.free += free;
            }
        }
        out.push(d);
    }
    out
}

/// Disk etkinliği sayaçları ("PhysicalDisk(*)").
pub struct Meter {
    query: isize,
    idle: isize,
    read: isize,
    write: isize,
}

impl Meter {
    pub fn new() -> Self {
        let mut m = Self { query: 0, idle: 0, read: 0, write: 0 };
        unsafe {
            if PdhOpenQueryW(None, 0, &mut m.query) == 0 {
                let _ = PdhAddEnglishCounterW(m.query, h!("\\PhysicalDisk(*)\\% Idle Time"), 0, &mut m.idle);
                let _ = PdhAddEnglishCounterW(m.query, h!("\\PhysicalDisk(*)\\Disk Read Bytes/sec"), 0, &mut m.read);
                let _ = PdhAddEnglishCounterW(m.query, h!("\\PhysicalDisk(*)\\Disk Write Bytes/sec"), 0, &mut m.write);
                let _ = PdhCollectQueryData(m.query);
            }
        }
        m
    }

    /// Disk numarası → değer.
    fn array(c: isize) -> Vec<(u32, f64)> {
        let mut out = Vec::new();
        unsafe {
            let (mut size, mut count) = (0u32, 0u32);
            let _ = PdhGetFormattedCounterArrayW(c, PDH_FMT(PDH_FMT_DOUBLE.0 | 0x8000), &mut size, &mut count, None);
            if size == 0 {
                return out;
            }
            let mut buf = vec![0u8; size as usize];
            if PdhGetFormattedCounterArrayW(c, PDH_FMT(PDH_FMT_DOUBLE.0 | 0x8000), &mut size, &mut count, Some(buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W)) != 0 {
                return out;
            }
            let items = std::slice::from_raw_parts(buf.as_ptr() as *const PDH_FMT_COUNTERVALUE_ITEM_W, count as usize);
            for it in items {
                let name = it.szName.to_string().unwrap_or_default();
                if let Some(n) = name.split_whitespace().next().and_then(|s| s.parse::<u32>().ok()) {
                    out.push((n, it.FmtValue.Anonymous.doubleValue));
                }
            }
        }
        out
    }

    pub fn update(&mut self, disks: &mut [Disk]) {
        unsafe {
            let _ = PdhCollectQueryData(self.query);
        }
        let idle = Self::array(self.idle);
        let read = Self::array(self.read);
        let write = Self::array(self.write);
        for d in disks.iter_mut() {
            let get = |v: &[(u32, f64)]| v.iter().find(|x| x.0 == d.index).map(|x| x.1);
            d.active = get(&idle).map(|i| (100.0 - i).clamp(0.0, 100.0) as f32).unwrap_or(0.0);
            d.read_bps = get(&read).unwrap_or(0.0);
            d.write_bps = get(&write).unwrap_or(0.0);
        }
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
