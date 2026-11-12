//! Fotoğraf okuma ve PNG yazma (WIC, Windows'ta hazır). Büyük fotoğraflar
//! çözülürken küçültülür (bellek düşük kalsın), telefonun EXIF yön bilgisi
//! uygulanır (yan yatmış kedi olmasın).

use crate::Image;
use std::path::Path;
use windows::{
    core::{HSTRING, PROPVARIANT},
    Win32::{Foundation::GENERIC_READ, Graphics::Imaging::*, System::Com::*},
};

/// İş parçacığında COM'u açar; yalnızca biz açtıysak kapatır.
pub struct Com(bool);

impl Com {
    pub fn init() -> Self {
        Com(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

fn factory() -> Result<IWICImagingFactory, String> {
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }.map_err(|e| format!("WIC: {e}"))
}

/// EXIF yönü (1-8); yoksa 1.
unsafe fn orientation(frame: &IWICBitmapFrameDecode) -> u16 {
    let Ok(q) = frame.GetMetadataQueryReader() else { return 1 };
    for name in ["System.Photo.Orientation", "/app1/ifd/{ushort=274}"] {
        let mut v = PROPVARIANT::default();
        if q.GetMetadataByName(&HSTRING::from(name), &mut v).is_ok() {
            if let Ok(o) = u16::try_from(&v) {
                if (1..=8).contains(&o) {
                    return o;
                }
            }
        }
    }
    1
}

fn flip_for(o: u16) -> WICBitmapTransformOptions {
    match o {
        2 => WICBitmapTransformFlipHorizontal,
        3 => WICBitmapTransformRotate180,
        4 => WICBitmapTransformFlipVertical,
        5 => WICBitmapTransformOptions(WICBitmapTransformRotate90.0 | WICBitmapTransformFlipHorizontal.0),
        6 => WICBitmapTransformRotate90,
        7 => WICBitmapTransformOptions(WICBitmapTransformRotate270.0 | WICBitmapTransformFlipHorizontal.0),
        8 => WICBitmapTransformRotate270,
        _ => WICBitmapTransformRotate0,
    }
}

/// Resmi okur; uzun kenarı `max_side`'dan büyükse orantılı küçültür.
/// Çıktı düz (premultiplied olmayan) BGRA.
pub fn load(path: &Path, max_side: u32) -> Result<Image, String> {
    let _com = Com::init();
    let bad = |e: windows::core::Error| format!("{}: {e}", t!("Resim okunamadı", "Couldn't read the image"));
    unsafe {
        let f = factory()?;
        let dec = f.CreateDecoderFromFilename(&HSTRING::from(path.as_os_str()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand).map_err(bad)?;
        let frame = dec.GetFrame(0).map_err(bad)?;
        let (mut w, mut h) = (0, 0);
        frame.GetSize(&mut w, &mut h).map_err(bad)?;
        if w == 0 || h == 0 {
            return Err(t!("Resim boş", "The image is empty").into());
        }
        let mut src: IWICBitmapSource = frame.clone().into();
        let k = (max_side.max(16) as f64 / w.max(h) as f64).min(1.0);
        if k < 1.0 {
            let (tw, th) = (((w as f64 * k).round() as u32).max(1), ((h as f64 * k).round() as u32).max(1));
            let sc = f.CreateBitmapScaler().map_err(bad)?;
            sc.Initialize(&src, tw, th, WICBitmapInterpolationModeFant).map_err(bad)?;
            src = sc.into();
        }
        let o = orientation(&frame);
        if o != 1 {
            let fr = f.CreateBitmapFlipRotator().map_err(bad)?;
            fr.Initialize(&src, flip_for(o)).map_err(bad)?;
            src = fr.into();
        }
        let conv = f.CreateFormatConverter().map_err(bad)?;
        conv.Initialize(&src, &GUID_WICPixelFormat32bppBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut).map_err(bad)?;
        let (mut w, mut h) = (0, 0);
        conv.GetSize(&mut w, &mut h).map_err(bad)?;
        let mut bgra = vec![0u8; w as usize * h as usize * 4];
        conv.CopyPixels(std::ptr::null(), w * 4, &mut bgra).map_err(bad)?;
        Ok(Image { w, h, bgra })
    }
}

/// Düz RGBA pikselleri PNG olarak yazar.
pub fn save_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
    let mut bgra = rgba.to_vec();
    for p in bgra.chunks_exact_mut(4) {
        p.swap(0, 2);
    }
    save_bgra(path, &Image { w, h, bgra })
}

/// Düz BGRA resmi PNG olarak yazar.
pub fn save_bgra(path: &Path, img: &Image) -> Result<(), String> {
    let _com = Com::init();
    let e = |e: windows::core::Error| format!("PNG yazılamadı: {e}");
    unsafe {
        let f = factory()?;
        let stream = f.CreateStream().map_err(e)?;
        stream.InitializeFromFilename(&HSTRING::from(path.as_os_str()), 0x4000_0000).map_err(e)?;
        let enc = f.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null()).map_err(e)?;
        enc.Initialize(&stream, WICBitmapEncoderNoCache).map_err(e)?;
        let (mut frame, mut bag) = (None, None);
        enc.CreateNewFrame(&mut frame, &mut bag).map_err(e)?;
        let frame = frame.ok_or("PNG karesi yok")?;
        frame.Initialize(bag.as_ref()).map_err(e)?;
        frame.SetSize(img.w, img.h).map_err(e)?;
        let mut fmt = GUID_WICPixelFormat32bppBGRA;
        frame.SetPixelFormat(&mut fmt).map_err(e)?;
        let src = f.CreateBitmapFromMemory(img.w, img.h, &GUID_WICPixelFormat32bppBGRA, img.w * 4, &img.bgra).map_err(e)?;
        frame.WriteSource(&src, std::ptr::null()).map_err(e)?;
        frame.Commit().map_err(e)?;
        enc.Commit().map_err(e)?;
    }
    Ok(())
}
