//! Uygulama simgeleri: shell'den bir kez okunur, Direct2D bitmap olarak
//! saklanır. Bitmap'ler çizim hedefine bağlıdır; hedef yeniden kurulunca
//! önbellek de boşaltılır.
//!
//! Simge çıkarmak (özellikle ilk kurulumda, Windows'un simge önbelleği boşken)
//! tanesi yüzlerce ms sürebilir. Eskiden menünün kendi iş parçacığında
//! okunuyordu: ısıtma sürerken Win tuşu sırada bekliyor, menü saniyelerce
//! geç açılıyordu. Artık okuma arka plandaki bir işçide yapılır; hazır olan
//! pikseller çizimde bitmap'e çevrilir (hızlı), okunmayanın yerine yer tutucu çizilir.

use std::{
    collections::{HashMap, HashSet},
    sync::{mpsc, Arc, Mutex},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::{Direct2D::Common::*, Direct2D::*, Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM, Imaging::*},
        System::Com::*,
        UI::WindowsAndMessaging::{PostMessageW, WM_APP},
    },
};

/// İşçi bir simgeyi bitirince menü penceresine gönderilir.
pub const WM_APP_ICON: u32 = WM_APP + 8;
const PX: i32 = 48;

/// Okunmuş simge: genişlik, yükseklik, PBGRA pikseller (yoksa `None`).
type Pixels = Option<(u32, u32, Vec<u8>)>;

pub struct Icons {
    cache: HashMap<String, Option<ID2D1Bitmap>>,
    /// İşçiye gönderilmiş, henüz dönmemiş yollar.
    pending: HashSet<String>,
    ready: Arc<Mutex<Vec<(String, Pixels)>>>,
    tx: mpsc::Sender<String>,
    /// Bu karede okunmamış (bekleyen) simge var.
    pub missed: bool,
}

impl Icons {
    pub fn new(hwnd: HWND) -> Result<Self> {
        let (tx, rx) = mpsc::channel::<String>();
        let ready: Arc<Mutex<Vec<(String, Pixels)>>> = Arc::default();
        let out = ready.clone();
        let h = hwnd.0 as isize;
        std::thread::Builder::new()
            .name("start-icons".into())
            .spawn(move || unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                let Ok(wic) = CoCreateInstance::<_, IWICImagingFactory>(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) else { return };
                for path in rx {
                    let px = read(&wic, &path);
                    out.lock().unwrap_or_else(|e| e.into_inner()).push((path, px));
                    let _ = PostMessageW(HWND(h as _), WM_APP_ICON, WPARAM(0), LPARAM(0));
                }
            })
            .map_err(|_| Error::from(E_FAIL))?;
        Ok(Self { cache: HashMap::new(), pending: HashSet::new(), ready, tx, missed: false })
    }

    /// Çizim hedefi yeniden kuruldu: bitmap'ler geçersiz (pikseller yeniden okunur).
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Yeni kareye başlarken: işçinin bitirdiklerini bu hedefte bitmap'e çevirir.
    pub fn frame(&mut self, rt: &ID2D1RenderTarget) {
        self.missed = false;
        let done = std::mem::take(&mut *self.ready.lock().unwrap_or_else(|e| e.into_inner()));
        for (path, px) in done {
            self.pending.remove(&path);
            let bmp = px.and_then(|(w, h, data)| unsafe {
                let props = D2D1_BITMAP_PROPERTIES { pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED }, dpiX: 96.0, dpiY: 96.0 };
                rt.CreateBitmap(D2D_SIZE_U { width: w, height: h }, Some(data.as_ptr() as _), w * 4, &props).ok()
            });
            self.cache.insert(path, bmp);
        }
    }

    pub fn get(&mut self, path: &str) -> Option<ID2D1Bitmap> {
        if let Some(b) = self.cache.get(path) {
            return b.clone();
        }
        self.missed = true;
        if self.pending.insert(path.to_string()) && self.tx.send(path.to_string()).is_err() {
            self.pending.remove(path);
        }
        None
    }
}

/// Simgeyi shell'den okur ve piksellerini kopyalar (işçi iş parçacığında).
fn read(wic: &IWICImagingFactory, path: &str) -> Pixels {
    let full = crate::system::expand(path);
    let conv = carlui_ui::icons::shell_icon(wic, &full, PX).ok()?;
    unsafe {
        let (mut w, mut h) = (0, 0);
        conv.GetSize(&mut w, &mut h).ok()?;
        if w == 0 || h == 0 || w > 1024 || h > 1024 {
            return None;
        }
        let mut data = vec![0u8; (w * h * 4) as usize];
        conv.CopyPixels(std::ptr::null(), w * 4, &mut data).ok()?;
        Some((w, h, data))
    }
}
