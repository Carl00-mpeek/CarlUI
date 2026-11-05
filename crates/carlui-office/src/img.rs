//! Bu modül "belgedeki resimleri (PNG/JPEG/GIF/BMP/TIFF) ve PDF sayfa
//! görüntülerini ekrana çizelim" sorununu çözüyor: baytlar WIC ile çözülür,
//! Direct2D bitmap'ine çevrilir. Bitmap'ler çizim hedefine bağlı olduğundan
//! önbellek hedef yeniden kurulunca boşaltılır.

use std::collections::HashMap;
use windows::{
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
            let conv = wic.CreateFormatConverter().ok()?;
            conv.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut).ok()?;
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
