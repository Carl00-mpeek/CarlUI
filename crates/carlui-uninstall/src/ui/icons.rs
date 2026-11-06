//! Uygulama simgeleri. Simge okumak yavaş olabildiği için her karede en çok
//! birkaç tanesi okunur; bitmap'ler çizim hedefine bağlı olduğundan hedef
//! değişince önbellek boşaltılır.

use std::collections::HashMap;
use windows::{
    core::*,
    Win32::{
        Foundation::GENERIC_READ,
        Graphics::{Direct2D::*, Imaging::*},
        System::Com::*,
    },
};

/// Mağaza uygulamasının simgesi bir resim dosyasıdır. Kabuktan istenirse resmin
/// kendisi değil, PNG dosya türünün simgesi gelir.
fn is_image(path: &str) -> bool {
    path.rsplit('.').next().is_some_and(|e| ["png", "jpg", "jpeg"].iter().any(|x| e.eq_ignore_ascii_case(x)))
}

fn decode(wic: &IWICImagingFactory, path: &str) -> Result<IWICFormatConverter> {
    unsafe {
        let frame = wic.CreateDecoderFromFilename(&HSTRING::from(path), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)?.GetFrame(0)?;
        let conv = wic.CreateFormatConverter()?;
        conv.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut)?;
        Ok(conv)
    }
}

pub struct Icons {
    wic: IWICImagingFactory,
    cache: HashMap<String, Option<ID2D1Bitmap>>,
    budget: u32,
    /// Bütçe yetmediği için okunmayan simge kaldı: bir kare daha çizilmeli.
    pub missed: bool,
}

impl Icons {
    pub fn new() -> Result<Self> {
        Ok(Self { wic: unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)? }, cache: HashMap::new(), budget: 0, missed: false })
    }

    pub fn clear(&mut self) {
        self.cache.clear();
    }

    pub fn frame(&mut self, n: u32) {
        self.budget = n;
        self.missed = false;
    }

    pub fn get(&mut self, rt: &ID2D1RenderTarget, path: &str) -> Option<ID2D1Bitmap> {
        if let Some(b) = self.cache.get(path) {
            return b.clone();
        }
        if self.budget == 0 {
            self.missed = true;
            return None;
        }
        self.budget -= 1;
        let image = if is_image(path) { decode(&self.wic, path) } else { carlui_ui::icons::shell_icon(&self.wic, path, 48) };
        let bmp = image.ok().and_then(|c| unsafe { rt.CreateBitmapFromWicBitmap(&c, None).ok() });
        self.cache.insert(path.to_string(), bmp.clone());
        bmp
    }
}
