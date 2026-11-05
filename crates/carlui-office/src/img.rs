//! Bu modül "belgedeki resimleri (PNG/JPEG/GIF/BMP/TIFF) ve PDF sayfa
//! görüntülerini ekrana çizelim" sorununu çözüyor: baytlar WIC ile çözülür,
//! Direct2D bitmap'ine çevrilir. Bitmap'ler çizim hedefine bağlı olduğundan
//! önbellek hedef yeniden kurulunca boşaltılır.

use std::collections::HashMap;
use windows::{
    core::Interface,
    Win32::{
        Graphics::{Direct2D::*, Imaging::*},
        System::Com::*,
    },
};

thread_local! {
    static WIC: Option<IWICImagingFactory> = unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok() };
}

/// Baytları 32 bit BGRA (önceden çarpılmış) WIC bitmap kaynağına çözer.
pub fn decode(bytes: &[u8]) -> Option<IWICFormatConverter> {
    WIC.with(|w| {
        let wic = w.as_ref()?;
        unsafe {
            let stream = wic.CreateStream().ok()?;
            stream.InitializeFromMemory(bytes).ok()?;
            let dec = wic.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand).ok()?;
            let frame = dec.GetFrame(0).ok()?;
            // Başlığında dev boyut yazan resim (ör. 60000×60000) belleği
            // tüketmesin: piksel sınırını aşan resim küçültülerek çözülür.
            let (mut w, mut h) = (0u32, 0u32);
            frame.GetSize(&mut w, &mut h).ok()?;
            let px = w as u64 * h as u64;
            let side = crate::guard::SIDE as f64;
            let too_big = px > crate::guard::IMAGE_PIXELS || w > crate::guard::SIDE || h > crate::guard::SIDE;
            let src: IWICBitmapSource = if too_big {
                let k = (crate::guard::IMAGE_PIXELS as f64 / px as f64).sqrt().min(side / w as f64).min(side / h as f64);
                let sc = wic.CreateBitmapScaler().ok()?;
                sc.Initialize(&frame, ((w as f64 * k) as u32).max(1), ((h as f64 * k) as u32).max(1), WICBitmapInterpolationModeFant).ok()?;
                sc.cast().ok()?
            } else {
                frame.cast().ok()?
            };
            let conv = wic.CreateFormatConverter().ok()?;
            conv.Initialize(&src,&GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut).ok()?;
            Some(conv)
        }
    })
}

pub fn to_d2d(rt: &ID2D1RenderTarget, bytes: &[u8]) -> Option<ID2D1Bitmap> {
    let src = decode(bytes)?;
    unsafe { rt.CreateBitmapFromWicBitmap(&src, None).ok() }
}

/// Ada göre önbellek; çözülemeyen resim için None saklanır (her karede yeniden denenmesin).
#[derive(Default)]
pub struct Cache {
    map: HashMap<String, Option<ID2D1Bitmap>>,
}

impl Cache {
    pub fn get(&mut self, rt: &ID2D1RenderTarget, key: &str, load: impl FnOnce() -> Option<Vec<u8>>) -> Option<ID2D1Bitmap> {
        if let Some(b) = self.map.get(key) {
            return b.clone();
        }
        let b = load().and_then(|bytes| to_d2d(rt, &bytes));
        self.map.insert(key.to_string(), b.clone());
        b
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }
}
