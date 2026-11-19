//! Şarjlı fare ve kulaklığın pil seviyesi.
//!
//! Dört kaynak birleştirilir:
//! - **Bluetooth** cihazlar: Windows'un kendi pil özelliği (`bt::batteries`).
//! - **ATK alıcılı fareler** (VID 373B, ör. ATK A9 Plus): üreticinin HID
//!   protokolü. Rapor 0x08, 16 baytlık gövde `[komut, 0, adr_hi, adr_lo, uzunluk,
//!   veri×10, sağlama]`; 0x04 komutu `[yüzde, şarjda, mV_hi, mV_lo]` döner.
//! - **HyperX Cloud III S Wireless** alıcısı (VID 03F0): 0x0C raporuyla
//!   `0C 02 03 01 00 06` sorusu; yanıtın 6. baytı yüzdedir (>100 = kulaklık kapalı).
//! - **Diğer HID cihazlar**: tanımlayıcısında standart "Battery Strength"
//!   (sayfa 0x06, kullanım 0x20) alanı olan her alıcı; marka bilgisi gerekmez.
//!
//! Protokoller açık kaynak gözlemlere dayanır (libatk-rs, atk-a9-air-plus-battery,
//! HyperX-Battery-Tray); kod bağımsız yazılmıştır. Okuma birkaç yüz ms sürebilir:
//! arayüz iş parçacığından çağrılmamalı.

use std::time::{Duration, Instant};
use windows::{
    core::*,
    Win32::{
        Devices::{DeviceAndDriverInstallation::*, HumanInterfaceDevice::*},
        Foundation::*,
        Storage::FileSystem::*,
        System::{Threading::*, IO::*},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Mouse,
    Headset,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reading {
    pub kind: Kind,
    pub name: String,
    /// `None`: cihaz bağlı ama kapalı / cevap vermiyor.
    pub percent: Option<u8>,
    pub charging: bool,
}

// ---- HID yardımcıları ------------------------------------------------------

struct Iface {
    path: String,
    page: u16,
    usage: u16,
    in_len: usize,
    out_len: usize,
}

/// Verilen üreticinin HID arayüzleri (sayfa/kullanım ve rapor uzunluklarıyla).
fn interfaces(vid: &str) -> Vec<Iface> {
    let mut out = Vec::new();
    unsafe {
        let g = HidD_GetHidGuid();
        let Ok(set) = SetupDiGetClassDevsW(Some(&g), None, None, DIGCF_PRESENT | DIGCF_DEVICEINTERFACE) else { return out };
        let mut i = 0;
        loop {
            let mut ifd = SP_DEVICE_INTERFACE_DATA { cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32, ..Default::default() };
            if SetupDiEnumDeviceInterfaces(set, None, &g, i, &mut ifd).is_err() {
                break;
            }
            i += 1;
            // DevicePath değişken uzunlukta: u32 hizalı tampon.
            let mut buf = vec![0u32; 512];
            let d = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*d).cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            if SetupDiGetDeviceInterfaceDetailW(set, &ifd, Some(d), (buf.len() * 4) as u32, None, None).is_err() {
                continue;
            }
            let p = (*d).DevicePath.as_ptr();
            let mut n = 0;
            while *p.add(n) != 0 {
                n += 1;
            }
            let path = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
            if !path.to_lowercase().contains(vid) {
                continue;
            }
            let Some(h) = open(&path, 0) else { continue };
            let mut pp = PHIDP_PREPARSED_DATA::default();
            let mut caps = HIDP_CAPS::default();
            if HidD_GetPreparsedData(h, &mut pp).as_bool() {
                let _ = HidP_GetCaps(pp, &mut caps);
                let _ = HidD_FreePreparsedData(pp);
            }
            let _ = CloseHandle(h);
            out.push(Iface {
                path,
                page: caps.UsagePage,
                usage: caps.Usage,
                in_len: caps.InputReportByteLength as usize,
                out_len: caps.OutputReportByteLength as usize,
            });
        }
        let _ = SetupDiDestroyDeviceInfoList(set);
    }
    out
}

/// `access` 0: yalnızca özellik sorgusu; değilse okuma/yazma (örtüşmeli).
fn open(path: &str, access: u32) -> Option<HANDLE> {
    let flags = if access == 0 { FILE_FLAGS_AND_ATTRIBUTES(0) } else { FILE_FLAG_OVERLAPPED };
    unsafe { CreateFileW(&HSTRING::from(path), access, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, flags, None).ok() }
}

/// Açık bir HID tutamacı; düşerken kapanır.
struct Dev(HANDLE);

impl Drop for Dev {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

impl Dev {
    fn open(path: &str) -> Option<Self> {
        open(path, (GENERIC_READ | GENERIC_WRITE).0).map(Dev)
    }

    /// Örtüşmeli G/Ç: `ms` içinde bitmezse iptal edilir. Aktarılan bayt sayısı.
    fn io(&self, buf: &mut [u8], write: bool, ms: u32) -> Option<usize> {
        unsafe {
            let ev = CreateEventW(None, true, false, None).ok()?;
            let mut o = OVERLAPPED { hEvent: ev, ..Default::default() };
            let _ = if write { WriteFile(self.0, Some(buf), None, Some(&mut o)) } else { ReadFile(self.0, Some(buf), None, Some(&mut o)) };
            let mut got = 0;
            let ok = WaitForSingleObject(ev, ms) == WAIT_OBJECT_0 && GetOverlappedResult(self.0, &o, &mut got, false).is_ok();
            if !ok {
                let _ = CancelIoEx(self.0, Some(&o));
                let _ = GetOverlappedResult(self.0, &o, &mut got, true);
            }
            let _ = CloseHandle(ev);
            ok.then_some(got as usize)
        }
    }

    fn write(&self, data: &[u8]) -> bool {
        let mut b = data.to_vec();
        self.io(&mut b, true, 500).is_some()
    }

    fn read(&self, len: usize, ms: u32) -> Option<Vec<u8>> {
        let mut b = vec![0u8; len.max(1)];
        let n = self.io(&mut b, false, ms)?;
        b.truncate(n);
        Some(b)
    }

    /// Kuyrukta bekleyen eski raporları atar.
    fn drain(&self, len: usize) {
        for _ in 0..32 {
            if self.read(len, 15).is_none() {
                return;
            }
        }
    }

    /// `req` gönderilir, `parse` bir sonuç verene kadar `ms` boyunca okunur.
    fn ask<T>(&self, req: &[u8], in_len: usize, ms: u64, parse: impl Fn(&[u8]) -> Option<T>) -> Option<T> {
        if !self.write(req) {
            return None;
        }
        let end = Instant::now() + Duration::from_millis(ms);
        while let Some(left) = end.checked_duration_since(Instant::now()) {
            let raw = self.read(in_len, left.as_millis().max(1) as u32)?;
            if let Some(v) = parse(&raw) {
                return Some(v);
            }
        }
        None
    }
}

// ---- ATK fareleri ------------------------------------------------------------

const ATK_VID: &str = "vid_373b";
const ATK_REPORT: u8 = 0x08;
const ATK_POWER: u8 = 0x04;

fn atk_sum(body: &[u8]) -> u8 {
    let s = body.iter().fold(ATK_REPORT as u32, |a, &b| a + b as u32);
    0x55u8.wrapping_sub((s & 0xFF) as u8)
}

/// Rapor kimliği başta, 16 baytlık gövde; tampon arayüzün rapor boyuna tamamlanır.
fn atk_request(cmd: u8, out_len: usize) -> Vec<u8> {
    let mut r = vec![0u8; out_len.max(17)];
    r[0] = ATK_REPORT;
    r[1] = cmd;
    r[16] = atk_sum(&r[1..16]);
    r
}

/// Güç yanıtı: (yüzde, şarjda).
fn atk_parse(raw: &[u8]) -> Option<(u8, bool)> {
    // Windows okumada rapor kimliği ilk bayttır.
    let body = raw.strip_prefix(&[ATK_REPORT])?.get(..16)?;
    if body[0] != ATK_POWER || body[1] != 0 || atk_sum(&body[..15]) != body[15] {
        return None;
    }
    let pct = body[5];
    (pct <= 100).then_some((pct, body[6] != 0))
}

fn atk() -> Option<Reading> {
    let all = interfaces(ATK_VID);
    if all.is_empty() {
        return None;
    }
    // Komutlara yalnızca üreticinin kendi koleksiyonu cevap verir.
    let rank = |i: &Iface| match (i.page, i.usage) {
        (0xFF02, 2) => 0,
        (0xFF04, 2) => 1,
        (0xFF05, 1) => 2,
        (p, _) if p >= 0xFF00 => 5,
        _ => 99,
    };
    let mut cands: Vec<&Iface> = all.iter().filter(|i| rank(i) < 99 && i.out_len >= 17 && i.in_len >= 17).collect();
    cands.sort_by_key(|i| rank(i));
    let mut reading = Reading { kind: Kind::Mouse, name: "Mouse".into(), percent: None, charging: false };
    for i in cands {
        let Some(d) = Dev::open(&i.path) else { continue };
        d.drain(i.in_len);
        // Uyuklayan fare ilk soruyu kaçırabilir: birkaç kez sorulur.
        for _ in 0..3 {
            if let Some((p, c)) = d.ask(&atk_request(ATK_POWER, i.out_len), i.in_len, 500, atk_parse) {
                reading.percent = Some(p);
                reading.charging = c;
                return Some(reading);
            }
        }
    }
    Some(reading)
}

// ---- HyperX kulaklık ---------------------------------------------------------

const HYPERX_VID: &str = "vid_03f0";
const HYPERX_PIDS: [&str; 2] = ["pid_06be", "pid_02cc"];
const HYPERX_ASK: [u8; 6] = [0x0C, 0x02, 0x03, 0x01, 0x00, 0x06];

/// `Some(None)`: kulaklık kapalı; `None`: başka bir rapor.
fn hyperx_parse(raw: &[u8]) -> Option<Option<u8>> {
    if raw.len() < 7 || raw[..6] != HYPERX_ASK {
        return None;
    }
    Some((raw[6] <= 100).then_some(raw[6]))
}

fn hyperx() -> Option<Reading> {
    let all = interfaces(HYPERX_VID);
    let i = all.iter().find(|i| i.page == 0x01C0 && i.usage == 1 && HYPERX_PIDS.iter().any(|p| i.path.to_lowercase().contains(p)))?;
    let mut reading = Reading { kind: Kind::Headset, name: "HyperX Cloud III S".into(), percent: None, charging: false };
    if let Some(d) = Dev::open(&i.path) {
        d.drain(i.in_len);
        let mut req = vec![0u8; i.out_len.max(HYPERX_ASK.len())];
        req[..6].copy_from_slice(&HYPERX_ASK);
        reading.percent = d.ask(&req, i.in_len, 1500, hyperx_parse).flatten();
    }
    Some(reading)
}

// ---- standart HID pil alanı ----------------------------------------------------

/// HID standardı: Generic Device Controls (0x06) / Battery Strength (0x20).
const GDC_PAGE: u16 = 0x06;
const BATTERY_STRENGTH: u16 = 0x20;

/// Üreticiye özel protokolü bilinmeyen cihazlar için: HID tanımlayıcısında
/// standart "pil gücü" alanı bildiren her arayüzden değer okunur (özellik ya da
/// giriş raporu). Bunu bildiren 2.4 GHz alıcılar ve oyun kolları kendiliğinden görünür.
fn generic(skip: &[&str]) -> Vec<Reading> {
    let mut out: Vec<Reading> = Vec::new();
    for i in interfaces("") {
        let lower = i.path.to_lowercase();
        if skip.iter().any(|v| lower.contains(v)) {
            continue;
        }
        let Some(h) = open(&i.path, 0) else { continue };
        let found = unsafe { generic_one(h) };
        unsafe {
            let _ = CloseHandle(h);
        }
        if let Some((pct, name)) = found {
            if !out.iter().any(|r| r.name == name) {
                let kind = if (i.page, i.usage) == (1, 2) { Kind::Mouse } else { kind_of(&name) };
                out.push(Reading { kind, name, percent: Some(pct), charging: false });
            }
        }
    }
    out
}

/// Arayüzün pil gücü alanı varsa (yüzde, ürün adı).
unsafe fn generic_one(h: HANDLE) -> Option<(u8, String)> {
    let mut pp = PHIDP_PREPARSED_DATA::default();
    if !HidD_GetPreparsedData(h, &mut pp).as_bool() {
        return None;
    }
    let mut caps = HIDP_CAPS::default();
    let _ = HidP_GetCaps(pp, &mut caps);
    let mut result = None;
    'types: for (t, len) in [(HidP_Feature, caps.FeatureReportByteLength), (HidP_Input, caps.InputReportByteLength)] {
        if len == 0 {
            continue;
        }
        let mut v = vec![HIDP_VALUE_CAPS::default(); 64];
        let mut n = v.len() as u16;
        if HidP_GetValueCaps(t, v.as_mut_ptr(), &mut n, pp).is_err() {
            continue;
        }
        for c in &v[..n as usize] {
            let hit = c.UsagePage == GDC_PAGE
                && if c.IsRange.as_bool() {
                    (c.Anonymous.Range.UsageMin..=c.Anonymous.Range.UsageMax).contains(&BATTERY_STRENGTH)
                } else {
                    c.Anonymous.NotRange.Usage == BATTERY_STRENGTH
                };
            if !hit {
                continue;
            }
            let mut buf = vec![0u8; len as usize];
            buf[0] = c.ReportID;
            let ok = if t == HidP_Feature {
                HidD_GetFeature(h, buf.as_mut_ptr() as _, len as u32).as_bool()
            } else {
                HidD_GetInputReport(h, buf.as_mut_ptr() as _, len as u32).as_bool()
            };
            if !ok {
                continue;
            }
            let mut val = 0u32;
            if HidP_GetUsageValue(t, GDC_PAGE, c.LinkCollection, BATTERY_STRENGTH, &mut val, pp, &buf).is_err() {
                continue;
            }
            result = Some((scale(val, c.LogicalMin, c.LogicalMax), product_name(h)));
            break 'types;
        }
    }
    let _ = HidD_FreePreparsedData(pp);
    result
}

/// Ham değer mantıksal aralığa göre yüzdeye çevrilir (çoğu 0..100, bazısı 0..255).
fn scale(val: u32, lo: i32, hi: i32) -> u8 {
    let (lo, hi) = (lo as i64, if hi <= lo { 100 } else { hi as i64 });
    ((val as i64 - lo).clamp(0, hi - lo) * 100 / (hi - lo)) as u8
}

fn product_name(h: HANDLE) -> String {
    let mut b = [0u16; 128];
    let ok = unsafe { HidD_GetProductString(h, b.as_mut_ptr() as _, 256).as_bool() };
    let n = b.iter().position(|&x| x == 0).unwrap_or(b.len());
    let s = String::from_utf16_lossy(&b[..n]).trim().to_string();
    if ok && !s.is_empty() { s } else { "HID".into() }
}

// ---- birleşik ----------------------------------------------------------------

fn kind_of(name: &str) -> Kind {
    let n = name.to_lowercase();
    if ["mouse", "fare", "mx ", "g pro", "viper", "deathadder"].iter().any(|k| n.contains(k)) {
        Kind::Mouse
    } else if ["head", "bud", "kulak", "airpods", "earphone", "wh-", "wf-", "cloud", "arctis"].iter().any(|k| n.contains(k)) {
        Kind::Headset
    } else {
        Kind::Other
    }
}

/// Tüm bilinen cihazların pil durumu (yavaş: arka planda çağır). Önce fareler,
/// sonra kulaklıklar, sonra diğerleri.
pub fn read_all() -> Vec<Reading> {
    let mut out: Vec<Reading> = Vec::new();
    out.extend(atk());
    out.extend(hyperx());
    // Bluetooth cihazlar aşağıda Windows'tan gelir; HID tarafında ikinci kez sayılmasın.
    out.extend(generic(&[ATK_VID, HYPERX_VID, "00001124-0000-1000-8000-00805f9b34fb", "00001812-0000-1000-8000-00805f9b34fb"]));
    for (name, pct) in crate::bt::batteries() {
        out.push(Reading { kind: kind_of(&name), name, percent: Some(pct), charging: false });
    }
    out.sort_by_key(|r| r.kind as u8);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atk_istegi_saglamali() {
        let r = atk_request(ATK_POWER, 64);
        assert_eq!(r.len(), 64);
        assert_eq!(&r[..2], &[0x08, 0x04]);
        assert_eq!(r[16], 0x49);
    }

    #[test]
    fn atk_yaniti_okunur() {
        let mut body = [0u8; 16];
        body[0] = ATK_POWER;
        body[5] = 73;
        body[6] = 1;
        body[15] = atk_sum(&body[..15]);
        let mut raw = vec![ATK_REPORT];
        raw.extend_from_slice(&body);
        raw.resize(64, 0);
        assert_eq!(atk_parse(&raw), Some((73, true)));
        raw[2] = 1; // bozuk
        assert_eq!(atk_parse(&raw), None);
    }

    #[test]
    fn hyperx_yaniti() {
        assert_eq!(hyperx_parse(&[0x0C, 2, 3, 1, 0, 6, 71, 0x0F]), Some(Some(71)));
        assert_eq!(hyperx_parse(&[0x0C, 2, 3, 1, 0, 6, 0xFF]), Some(None));
        assert_eq!(hyperx_parse(&[0x0C, 2, 3, 1, 0, 7, 50]), None);
    }

    #[test]
    fn olcek() {
        assert_eq!(scale(50, 0, 100), 50);
        assert_eq!(scale(255, 0, 255), 100);
        assert_eq!(scale(7, 0, 0), 7);
    }

    #[test]
    fn ad_turu() {
        assert_eq!(kind_of("Logitech MX Master 3"), Kind::Mouse);
        assert_eq!(kind_of("Galaxy Buds2"), Kind::Headset);
        assert_eq!(kind_of("Xbox Controller"), Kind::Other);
    }
}
