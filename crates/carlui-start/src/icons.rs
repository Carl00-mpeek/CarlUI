//! Uygulama simgeleri: shell'den bir kez okunur, Direct2D bitmap olarak
//! saklanır. Bitmap'ler çizim hedefine bağlıdır; hedef yeniden kurulunca
//! önbellek de boşaltılır.

use std::collections::HashMap;
use windows::{
    core::*,
    Win32::{
        Graphics::{Direct2D::*, Imaging::*},
        System::Com::*,
    },
};

pub struct Icons {
    wic: IWICImagingFactory,
    cache: HashMap<String, Option<ID2D1Bitmap>>,
    /// Bu karede daha kaç simge çıkarılabilir. Simge çıkarmak yavaş bir
    /// diskte tanesi onlarca ms sürebilir; hepsi tek karede okunursa menü
    /// açılırken / listede kaydırırken donuyordu. Kalanlar sonraki karelere.
    budget: u32,
    /// Bütçe yetmediği için okunmayan simge kaldı: bir kare daha çizilmeli.
    pub missed: bool,
}

impl Icons {
    pub fn new() -> Result<Self> {
        Ok(Self { wic: unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)? }, cache: HashMap::new(), budget: u32::MAX, missed: false })
    }

    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Yeni kareye başlarken: bu karede en çok `n` yeni simge okunur.
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
        let full = crate::system::expand(path);
        let bmp = carlui_ui::icons::shell_icon(&self.wic, &full, 48).ok().and_then(|c| unsafe { rt.CreateBitmapFromWicBitmap(&c, None).ok() });
        self.cache.insert(path.to_string(), bmp.clone());
        bmp
    }
}
