//! Resim dönüştürme. Okuma Windows Imaging Component (WIC) ile yapılır:
//! PNG, JPG, BMP, TIFF, GIF, ICO, DDS, JXR ve kuruluysa HEIC/AVIF/WebP
//! eklentileri. WIC'in okuyamadıkları (PSD, TGA, SVG, JXL, EXR …) önce FFmpeg
//! ile geçici PNG'ye çevrilir. Yazma:
//! - PNG, JPG, BMP, TIFF → WIC; GIF → FFmpeg (saydam paletle), yoksa WIC
//! - DDS (sıkıştırmasız BGRA8), TGA (32 bit), ICO (256…16 px PNG katmanları)
//!   → burada elle yazılır; her oyun/düzenleyici açabilir
//! - WebP, AVIF, JPEG XL → FFmpeg (geçici PNG üzerinden)
//! Hareketli GIF gibi çok kareli dosyalarda ilk kare (ICO'da en büyük kare) alınır.

use crate::{ffmpeg, kinds::Kind};
use std::path::PathBuf;
use windows::{
    core::*,
    Win32::{
        Foundation::GENERIC_READ,
        Graphics::Imaging::*,
        System::Com::*,
    },
};

/// Düz (premultiplied olmayan) BGRA, satırlar yukarıdan aşağı.
pub struct Pixels {
    pub w: u32,
    pub h: u32,
    pub data: Vec<u8>,
}

fn factory() -> Result<IWICImagingFactory> {
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
}

fn temp_png(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("carlui-conv-{}-{tag}.png", std::process::id()))
}

fn wic_decode(path: &str) -> Result<Pixels> {
    let f = factory()?;
    unsafe {
        let dec = f.CreateDecoderFromFilename(&HSTRING::from(path), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)?;
        // ICO/CUR gibi çok boyutlu dosyalarda en büyük kare.
        let n = dec.GetFrameCount()?.max(1);
        let mut best = (0u64, 0u32);
        if Kind::of(path) == Some(Kind::Image) && crate::kinds::ext(path) == "ico" {
            for i in 0..n {
                let fr = dec.GetFrame(i)?;
                let (mut w, mut h) = (0, 0);
                fr.GetSize(&mut w, &mut h)?;
                if w as u64 * h as u64 > best.0 {
                    best = (w as u64 * h as u64, i);
                }
            }
        }
        let frame = dec.GetFrame(best.1)?;
        let conv = f.CreateFormatConverter()?;
        conv.Initialize(&frame, &GUID_WICPixelFormat32bppBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut)?;
        let (mut w, mut h) = (0, 0);
        conv.GetSize(&mut w, &mut h)?;
        let mut data = vec![0u8; w as usize * h as usize * 4];
        conv.CopyPixels(std::ptr::null(), w * 4, &mut data)?;
        Ok(Pixels { w, h, data })
    }
}

/// Dosyayı okur; WIC başaramazsa FFmpeg ile geçici PNG'ye çevirip okur.
pub fn decode(path: &str, ff: Option<&PathBuf>) -> std::result::Result<Pixels, String> {
    if let Ok(p) = wic_decode(path) {
        return Ok(p);
    }
    let Some(ff) = ff else { return Err(t!("Bu biçimi okumak için FFmpeg gerekli", "Reading this format needs FFmpeg").into()) };
    let tmp = temp_png("in");
    let never = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let r = ffmpeg::run(ff, path, &["-frames:v".into(), "1".into(), "-pix_fmt".into(), "rgba".into()], &tmp.to_string_lossy(), &never, &|_| {});
    let out = r.and_then(|_| wic_decode(&tmp.to_string_lossy()).map_err(|e| e.message().to_string()));
    let _ = std::fs::remove_file(&tmp);
    out.map_err(|e| format!("{}: {e}", t!("Okunamadı", "Couldn't read")))
}

fn bitmap(f: &IWICImagingFactory, p: &Pixels) -> Result<IWICBitmap> {
    unsafe { f.CreateBitmapFromMemory(p.w, p.h, &GUID_WICPixelFormat32bppBGRA, p.w * 4, &p.data) }
}

/// Saydamlığı beyaz zemine oturtur (JPG/BMP saydamlık taşımaz; yoksa siyah olur).
fn flatten(p: &Pixels) -> Pixels {
    let mut d = p.data.clone();
    for px in d.chunks_exact_mut(4) {
        let a = px[3] as u32;
        for c in &mut px[..3] {
            *c = ((*c as u32 * a + 255 * (255 - a)) / 255) as u8;
        }
        px[3] = 255;
    }
    Pixels { w: p.w, h: p.h, data: d }
}

fn wic_encode(path: &str, container: &GUID, p: &Pixels) -> Result<()> {
    let f = factory()?;
    unsafe {
        let stream = f.CreateStream()?;
        stream.InitializeFromFilename(&HSTRING::from(path), 0x4000_0000)?; // GENERIC_WRITE
        let enc = f.CreateEncoder(container, std::ptr::null())?;
        enc.Initialize(&stream, WICBitmapEncoderNoCache)?;
        let mut frame = None;
        let mut bag = None;
        enc.CreateNewFrame(&mut frame, &mut bag)?;
        let frame = frame.ok_or_else(|| Error::from(windows::Win32::Foundation::E_FAIL))?;
        frame.Initialize(bag.as_ref())?;
        frame.SetSize(p.w, p.h)?;
        let src = bitmap(&f, p)?;
        if *container == GUID_ContainerFormatGif {
            // GIF 256 renk: görüntüden en iyi palet, hata yayma ile.
            let pal = f.CreatePalette()?;
            pal.InitializeFromBitmap(&src, 256, true)?;
            let mut fmt = GUID_WICPixelFormat8bppIndexed;
            frame.SetPixelFormat(&mut fmt)?;
            frame.SetPalette(&pal)?;
            let conv = f.CreateFormatConverter()?;
            conv.Initialize(&src, &GUID_WICPixelFormat8bppIndexed, WICBitmapDitherTypeErrorDiffusion, &pal, 0.0, WICBitmapPaletteTypeCustom)?;
            frame.WriteSource(&conv, std::ptr::null())?;
        } else {
            let mut fmt = GUID_WICPixelFormat32bppBGRA;
            frame.SetPixelFormat(&mut fmt)?;
            if fmt == GUID_WICPixelFormat32bppBGRA {
                frame.WriteSource(&src, std::ptr::null())?;
            } else {
                let conv = f.CreateFormatConverter()?;
                conv.Initialize(&src, &fmt, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut)?;
                frame.WriteSource(&conv, std::ptr::null())?;
            }
        }
        frame.Commit()?;
        enc.Commit()?;
    }
    Ok(())
}

fn scaled(p: &Pixels, size: u32) -> Result<Pixels> {
    // Kareye sığdır (ortala, kenarlar saydam), sonra Fant ile küçült.
    let side = p.w.max(p.h);
    let mut sq = vec![0u8; side as usize * side as usize * 4];
    let (ox, oy) = ((side - p.w) / 2, (side - p.h) / 2);
    for y in 0..p.h {
        let s = (y * p.w * 4) as usize;
        let d = (((y + oy) * side + ox) * 4) as usize;
        sq[d..d + (p.w * 4) as usize].copy_from_slice(&p.data[s..s + (p.w * 4) as usize]);
    }
    let sq = Pixels { w: side, h: side, data: sq };
    if side == size {
        return Ok(sq);
    }
    let f = factory()?;
    unsafe {
        let src = bitmap(&f, &sq)?;
        let sc = f.CreateBitmapScaler()?;
        sc.Initialize(&src, size, size, WICBitmapInterpolationModeHighQualityCubic)?;
        let mut data = vec![0u8; (size * size * 4) as usize];
        sc.CopyPixels(std::ptr::null(), size * 4, &mut data)?;
        Ok(Pixels { w: size, h: size, data })
    }
}

fn write_ico(path: &str, p: &Pixels) -> std::result::Result<(), String> {
    let max = p.w.max(p.h);
    let sizes: Vec<u32> = [256u32, 128, 64, 48, 32, 24, 16].into_iter().filter(|s| *s <= max.max(16)).collect();
    let mut images = Vec::new();
    for (i, s) in sizes.iter().enumerate() {
        let px = scaled(p, *s).map_err(|e| e.message().to_string())?;
        let tmp = temp_png(&format!("ico{i}"));
        wic_encode(&tmp.to_string_lossy(), &GUID_ContainerFormatPng, &px).map_err(|e| e.message().to_string())?;
        let bytes = std::fs::read(&tmp).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&tmp);
        images.push((*s, bytes));
    }
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (s, b) in &images {
        let d = if *s >= 256 { 0 } else { *s as u8 };
        out.extend_from_slice(&[d, d, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(b.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += b.len() as u32;
    }
    for (_, b) in &images {
        out.extend_from_slice(b);
    }
    std::fs::write(path, out).map_err(|e| e.to_string())
}

fn write_dds(path: &str, p: &Pixels) -> std::result::Result<(), String> {
    let mut h = Vec::with_capacity(128 + p.data.len());
    let u = |h: &mut Vec<u8>, v: u32| h.extend_from_slice(&v.to_le_bytes());
    h.extend_from_slice(b"DDS ");
    u(&mut h, 124); // dwSize
    u(&mut h, 0x1 | 0x2 | 0x4 | 0x8 | 0x1000); // CAPS | HEIGHT | WIDTH | PITCH | PIXELFORMAT
    u(&mut h, p.h);
    u(&mut h, p.w);
    u(&mut h, p.w * 4); // pitch
    u(&mut h, 0); // depth
    u(&mut h, 0); // mip
    for _ in 0..11 {
        u(&mut h, 0);
    }
    // DDS_PIXELFORMAT: RGB + ALPHAPIXELS, 32 bit, BGRA maskeleri
    u(&mut h, 32);
    u(&mut h, 0x40 | 0x1);
    u(&mut h, 0);
    u(&mut h, 32);
    u(&mut h, 0x00FF_0000);
    u(&mut h, 0x0000_FF00);
    u(&mut h, 0x0000_00FF);
    u(&mut h, 0xFF00_0000);
    u(&mut h, 0x1000); // caps: TEXTURE
    for _ in 0..4 {
        u(&mut h, 0);
    }
    h.extend_from_slice(&p.data);
    std::fs::write(path, h).map_err(|e| e.to_string())
}

fn write_tga(path: &str, p: &Pixels) -> std::result::Result<(), String> {
    let mut h = vec![0u8, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    h.extend_from_slice(&(p.w as u16).to_le_bytes());
    h.extend_from_slice(&(p.h as u16).to_le_bytes());
    h.push(32);
    h.push(0x28); // 8 bit alfa, sol üst köşeden
    h.extend_from_slice(&p.data);
    std::fs::write(path, h).map_err(|e| e.to_string())
}

/// `src` → `dst` (uzantı `ext`).
pub fn convert(src: &str, dst: &str, ext: &str, ff: Option<&PathBuf>) -> std::result::Result<(), String> {
    let p = decode(src, ff)?;
    if p.w > 65535 || p.h > 65535 {
        return Err(t!("Resim çok büyük", "Image is too large").into());
    }
    let wic = |c: &GUID, p: &Pixels| wic_encode(dst, c, p).map_err(|e| e.message().to_string());
    let r = match ext {
        "png" => wic(&GUID_ContainerFormatPng, &p),
        "jpg" => wic(&GUID_ContainerFormatJpeg, &flatten(&p)),
        "bmp" => wic(&GUID_ContainerFormatBmp, &flatten(&p)),
        "tiff" => wic(&GUID_ContainerFormatTiff, &p),
        // GIF: FFmpeg varsa saydamlığı koruyan paletle; yoksa WIC (beyaz zemin).
        "gif" if ff.is_none() => wic(&GUID_ContainerFormatGif, &flatten(&p)),
        "ico" => write_ico(dst, &p),
        "dds" => write_dds(dst, &p),
        "tga" => write_tga(dst, &p),
        _ => {
            let Some(ff) = ff else { return Err(t!("Bu biçim için FFmpeg gerekli", "This format needs FFmpeg").into()) };
            let tmp = temp_png("out");
            let r = wic_encode(&tmp.to_string_lossy(), &GUID_ContainerFormatPng, &p).map_err(|e| e.message().to_string()).and_then(|_| {
                let never = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                ffmpeg::run(ff, &tmp.to_string_lossy(), &ffmpeg::args_for(Kind::Image, ext), dst, &never, &|_| {})
            });
            let _ = std::fs::remove_file(&tmp);
            r
        }
    };
    if r.is_err() {
        let _ = std::fs::remove_file(dst);
    }
    r
}
