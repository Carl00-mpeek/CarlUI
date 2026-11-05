//! Bu modül "panodaki resim hangi biçimde gelirse gelsin PNG olarak saklansın"
//! sorununu WIC ile çözüyor: PNG/JPG/BMP baytları ya da çıplak DIB (başlıksız
//! bitmap) çözülür, tek tip PNG'ye kodlanır. Ayrıca HTML içindeki base64 resim
//! için küçük bir base64 çözücü içerir.

use windows::{
    core::*,
    Win32::{
        Graphics::Imaging::*,
        System::Com::*,
        UI::Shell::SHCreateMemStream,
    },
};

pub fn wic() -> Option<IWICImagingFactory> {
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok() }
}

/// Bayt dizisindeki resmi (PNG, JPG, BMP, GIF …) çözer; ilk kare, PBGRA.
pub fn decode(wic: &IWICImagingFactory, bytes: &[u8]) -> Option<(IWICFormatConverter, u32, u32)> {
    unsafe {
        let stream = wic.CreateStream().ok()?;
        stream.InitializeFromMemory(bytes).ok()?;
        let dec = wic.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnLoad).ok()?;
        let frame = dec.GetFrame(0).ok()?;
        let conv = wic.CreateFormatConverter().ok()?;
        conv.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut).ok()?;
        let (mut w, mut h) = (0, 0);
        conv.GetSize(&mut w, &mut h).ok()?;
        (w > 0 && h > 0).then_some((conv, w, h))
    }
}

/// Kaynağı PNG baytlarına kodlar.
pub fn encode_png(wic: &IWICImagingFactory, src: &IWICBitmapSource) -> Option<Vec<u8>> {
    encode(wic, src, &GUID_ContainerFormatPng)
}

/// PNG baytlarını JPG'ye çevirir (WIC'in varsayılan kalitesiyle, ~%90).
pub fn png_to_jpg(wic: &IWICImagingFactory, png: &[u8]) -> Option<Vec<u8>> {
    let (conv, _, _) = decode(wic, png)?;
    unsafe {
        let bgr = wic.CreateFormatConverter().ok()?;
        bgr.Initialize(&conv, &GUID_WICPixelFormat24bppBGR, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut).ok()?;
        encode(wic, &bgr.cast().ok()?, &GUID_ContainerFormatJpeg)
    }
}

fn encode(wic: &IWICImagingFactory, src: &IWICBitmapSource, container: &GUID) -> Option<Vec<u8>> {
    unsafe {
        let stream = SHCreateMemStream(None)?;
        let enc = wic.CreateEncoder(container, std::ptr::null()).ok()?;
        enc.Initialize(&stream, WICBitmapEncoderNoCache).ok()?;
        let mut frame = None;
        let mut props = None;
        enc.CreateNewFrame(&mut frame, &mut props).ok()?;
        let frame = frame?;
        frame.Initialize(props.as_ref()).ok()?;
        frame.WriteSource(src, std::ptr::null()).ok()?;
        frame.Commit().ok()?;
        enc.Commit().ok()?;
        let mut stat = STATSTG::default();
        stream.Stat(&mut stat, STATFLAG_NONAME).ok()?;
        let len = stat.cbSize as usize;
        stream.Seek(0, STREAM_SEEK_SET, None).ok()?;
        let mut out = vec![0u8; len];
        let mut read = 0u32;
        stream.Read(out.as_mut_ptr() as _, len as u32, Some(&mut read)).ok().ok()?;
        out.truncate(read as usize);
        Some(out)
    }
}

/// Herhangi bir resim baytını PNG'ye çevirir; (png, genişlik, yükseklik).
pub fn to_png(wic: &IWICImagingFactory, bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let (conv, w, h) = decode(wic, bytes)?;
    // Zaten PNG ise yeniden kodlamaya gerek yok.
    if bytes.starts_with(b"\x89PNG") {
        return Some((bytes.to_vec(), w, h));
    }
    let png = encode_png(wic, &conv.cast().ok()?)?;
    Some((png, w, h))
}

/// Listede gösterilecek küçük resim: en uzun kenarı `max` piksel olan PNG.
pub fn thumb_png(wic: &IWICImagingFactory, bytes: &[u8], max: u32) -> Option<Vec<u8>> {
    let (conv, w, h) = decode(wic, bytes)?;
    let k = (max as f32 / w.max(h) as f32).min(1.0);
    let (tw, th) = (((w as f32 * k).round() as u32).max(1), ((h as f32 * k).round() as u32).max(1));
    unsafe {
        let scaler = wic.CreateBitmapScaler().ok()?;
        scaler.Initialize(&conv, tw, th, WICBitmapInterpolationModeFant).ok()?;
        encode_png(wic, &scaler.cast().ok()?)
    }
}

/// Panodaki CF_DIB / CF_DIBV5 (dosya başlığı olmayan bitmap) → BMP dosyası baytları.
pub fn dib_to_bmp(dib: &[u8]) -> Option<Vec<u8>> {
    if dib.len() < 40 {
        return None;
    }
    let u32_at = |o: usize| u32::from_le_bytes([dib[o], dib[o + 1], dib[o + 2], dib[o + 3]]);
    let header = u32_at(0) as usize;
    let bits = u16::from_le_bytes([dib[14], dib[15]]) as usize;
    let compression = u32_at(16);
    let mut colors = u32_at(32) as usize;
    if colors == 0 && bits <= 8 {
        colors = 1 << bits;
    }
    // BI_BITFIELDS + eski 40 baytlık başlık: arkasında 3 renk maskesi var.
    let masks = if header == 40 && compression == 3 { 12 } else { 0 };
    let offset = 14 + header + masks + colors * 4;
    let mut out = Vec::with_capacity(14 + dib.len());
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((14 + dib.len()) as u32).to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&(offset as u32).to_le_bytes());
    out.extend_from_slice(dib);
    Some(out)
}

/// PNG → panoya yazılacak CF_DIB (32 bit, alttan üste).
pub fn png_to_dib(wic: &IWICImagingFactory, png: &[u8]) -> Option<Vec<u8>> {
    let (conv, w, h) = decode(wic, png)?;
    let stride = w * 4;
    let mut px = vec![0u8; (stride * h) as usize];
    unsafe { conv.CopyPixels(std::ptr::null(), stride, &mut px).ok()? };
    let mut out = Vec::with_capacity(40 + px.len());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    out.extend_from_slice(&(px.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 16]);
    for row in px.chunks_exact(stride as usize).rev() {
        out.extend_from_slice(row);
    }
    Some(out)
}

pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    };
    let clean: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace() && *b != b'=').collect();
    let mut out = Vec::with_capacity(clean.len() * 3 / 4);
    for chunk in clean.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c)? << (18 - 6 * i);
        }
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..chunk.len().saturating_sub(1)]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn b64() {
        assert_eq!(super::base64_decode("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(super::base64_decode("aGk").unwrap(), b"hi");
    }
}
