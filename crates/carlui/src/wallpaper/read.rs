//! Birincil ekranın duvar kâğıdını bulur ve küçültülmüş piksellerini okur.
//!
//! Sıra: IDesktopWallpaper (ekran başına duvar kâğıdı; sol üst köşesi 0,0
//! olan ekran birincildir) → boşsa slayt gösterisi açıkken Windows'un o an
//! gösterdiği kopya (TranscodedWallpaper), kapalıyken düz arka plan rengi.
//! COM kullanılamazsa eski SPI_GETDESKWALLPAPER yolu.

use std::{path::PathBuf, time::SystemTime};
use windows::{
    core::{HSTRING, PWSTR},
    Win32::{
        Foundation::GENERIC_READ,
        Graphics::{Gdi::GetSysColor, Gdi::COLOR_DESKTOP, Imaging::*},
        System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL, CLSCTX_INPROC_SERVER},
        UI::{
            Shell::{DesktopWallpaper, IDesktopWallpaper, DSS_ENABLED},
            WindowsAndMessaging::{SystemParametersInfoW, SPI_GETDESKWALLPAPER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS},
        },
    },
};

/// Renk hesabı için en uzun kenar (piksel).
const SAMPLE: u32 = 64;

/// Duvar kâğıdının imzası: değişmediyse resim yeniden çözülmez.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Image { path: PathBuf, modified: Option<SystemTime>, len: u64 },
    /// Düz renk (0xRRGGBB).
    Solid(u32),
    /// Bulunamadı / okunamadı: palete düşülür.
    Unknown,
}

fn image(path: PathBuf) -> Option<Source> {
    let m = std::fs::metadata(&path).ok().filter(|m| m.is_file() && m.len() > 0)?;
    Some(Source::Image { modified: m.modified().ok(), len: m.len(), path })
}

/// Windows'un o an gösterdiği duvar kâğıdının kopyası (slayt gösterisinde de güncel).
fn transcoded() -> Option<Source> {
    let dir = std::env::var_os("APPDATA")?;
    image(PathBuf::from(dir).join(r"Microsoft\Windows\Themes\TranscodedWallpaper"))
}

/// COLORREF (0x00BBGGRR) → 0xRRGGBB.
fn rgb(c: u32) -> u32 {
    (c & 0xFF) << 16 | (c & 0xFF00) | (c >> 16 & 0xFF)
}

/// COM'un ayırdığı yazıyı alır ve belleğini bırakır.
unsafe fn take(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    let s = p.to_string().unwrap_or_default();
    CoTaskMemFree(Some(p.0 as _));
    s
}

pub fn current() -> Source {
    desktop_wallpaper().unwrap_or_else(legacy)
}

fn desktop_wallpaper() -> Option<Source> {
    unsafe {
        let dw: IDesktopWallpaper = CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL).ok()?;
        let n = dw.GetMonitorDevicePathCount().ok()?;
        let mut first = None;
        let mut primary = None;
        for i in 0..n {
            let Ok(p) = dw.GetMonitorDevicePathAt(i) else { continue };
            let id = HSTRING::from(take(p));
            // Bağlı olmayan ekranların dikdörtgeni alınamaz; atlanır.
            let Ok(r) = dw.GetMonitorRECT(&id) else { continue };
            if r.left <= 0 && r.top <= 0 && r.right > 0 && r.bottom > 0 {
                primary = Some(id);
                break;
            }
            first.get_or_insert(id);
        }
        let id = primary.or(first)?;
        let path = take(dw.GetWallpaper(&id).ok()?);
        if !path.is_empty() {
            if let Some(s) = image(PathBuf::from(&path)) {
                return Some(s);
            }
        }
        let slideshow = dw.GetStatus().map(|s| s.0 & DSS_ENABLED.0 != 0).unwrap_or(false);
        if slideshow || !path.is_empty() {
            // Slayt gösterisi ya da yolu artık olmayan resim: Windows'un kopyası.
            return Some(transcoded().unwrap_or(Source::Unknown));
        }
        Some(dw.GetBackgroundColor().map(|c| Source::Solid(rgb(c.0))).unwrap_or(Source::Unknown))
    }
}

fn legacy() -> Source {
    let mut buf = [0u16; 520];
    let ok = unsafe { SystemParametersInfoW(SPI_GETDESKWALLPAPER, buf.len() as u32, Some(buf.as_mut_ptr() as _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)).is_ok() };
    let len = buf.iter().position(|&c| c == 0).unwrap_or(0);
    if ok && len > 0 {
        if let Some(s) = image(PathBuf::from(String::from_utf16_lossy(&buf[..len]))).or_else(transcoded) {
            return s;
        }
    }
    if ok && len == 0 {
        return Source::Solid(rgb(unsafe { GetSysColor(COLOR_DESKTOP) }));
    }
    Source::Unknown
}

/// Resmi en uzun kenarı `SAMPLE` olacak şekilde küçültüp 32 bit BGRA okur.
pub fn decode(path: &std::path::Path) -> Option<(Vec<u8>, u32, u32)> {
    unsafe {
        let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let dec = wic.CreateDecoderFromFilename(&HSTRING::from(path.as_os_str()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand).ok()?;
        let frame = dec.GetFrame(0).ok()?;
        let (mut w, mut h) = (0, 0);
        frame.GetSize(&mut w, &mut h).ok()?;
        if w == 0 || h == 0 {
            return None;
        }
        let k = (SAMPLE as f32 / w.max(h) as f32).min(1.0);
        let (tw, th) = (((w as f32 * k).round() as u32).max(1), ((h as f32 * k).round() as u32).max(1));
        let scaler = wic.CreateBitmapScaler().ok()?;
        scaler.Initialize(&frame, tw, th, WICBitmapInterpolationModeFant).ok()?;
        let conv = wic.CreateFormatConverter().ok()?;
        conv.Initialize(&scaler, &GUID_WICPixelFormat32bppBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut).ok()?;
        let mut px = vec![0u8; (tw * th * 4) as usize];
        conv.CopyPixels(std::ptr::null(), tw * 4, &mut px).ok()?;
        Some((px, tw, th))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorref_to_rgb() {
        assert_eq!(rgb(0x00CC8844), 0x4488CC);
    }
}
