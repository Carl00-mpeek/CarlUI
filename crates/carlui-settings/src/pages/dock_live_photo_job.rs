//! "Fotoğraftan hayvan yap" akışının durumu ve arka plan işi. Sorun: arka
//! planı silmek (BiRefNet) saniyeler sürer, boyut ya da yön her değiştiğinde
//! yeniden yapılamaz ve arayüzü kilitleyemez. Çözüm: fotoğraf seçilince
//! kesme işi bir kez ayrı iş parçacığında çalışır (iptal edilebilir), sonuç
//! saklanır; boyut/yön değişince yalnızca hızlı `make` + `preview` yeniden
//! çalışır (~10 ms). Durum pencere iş parçacığına ait (`thread_local`), böylece
//! çizim de iş başlatabilir/sonucu alabilir.

use carlui_ai::{
    petgen::{self, Opts, Pet},
    Image,
};
use std::{
    cell::RefCell,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};
use windows::Win32::{Foundation::HWND, Graphics::Gdi::InvalidateRect};

/// Boyut seçicideki adımlar (sanat pikseli; motor 12-64 kabul eder).
pub const SIZES: [u32; 9] = [12, 16, 20, 24, 28, 32, 40, 48, 64];
pub const DEFAULT_SIZE: u32 = 28;

/// Arka plan işinin paylaşılan durumu.
#[derive(Default)]
struct Shared {
    progress: f32,
    result: Option<Result<Image, String>>,
}

struct Job {
    cancel: Arc<AtomicBool>,
    shared: Arc<Mutex<Shared>>,
}

pub struct State {
    pub open: bool,
    pub photo: Option<PathBuf>,
    pub name: String,
    pub size: u32,
    pub faces_left: bool,
    job: Option<Job>,
    /// Seçili fotoğrafın kesilmiş hali (bir kez hesaplanır).
    cut: Option<Image>,
    /// Kesilmiş resimden yapılan hayvan ve önizlemesi (boyut/yöne göre).
    pub pet: Option<Result<Pet, String>>,
    pub preview: Option<Image>,
    /// Önizleme her değiştiğinde artar (çizimdeki bit eşlem önbelleği için).
    pub preview_gen: u64,
    /// Kesme ya da oluşturma hatası.
    pub error: Option<String>,
    /// Aynı adlı paket var: üzerine yazma onayı bekleyen klasör adı.
    pub confirm: Option<String>,
    /// Son oluşturulan paketin görünen adı (başarı iletisi).
    pub done: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self { open: false, photo: None, name: String::new(), size: DEFAULT_SIZE, faces_left: false, job: None, cut: None, pet: None, preview: None, preview_gen: 0, error: None, confirm: None, done: None }
    }
}

/// Çalışan kesim iş parçacığı sayısı (durdurulup henüz çıkmamışlar dahil).
static RUNNING: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static STATE: RefCell<State> = RefCell::new(demo_state());
    /// Model denetimi: (zaman, o andaki indirme sayısı, sonuç). Her boyamada
    /// diske ve ayar dosyasına gitmesin; indirme bitince hemen yenilenir.
    static MODEL: RefCell<Option<(Instant, usize, Result<(), String>)>> = const { RefCell::new(None) };
}

/// Yalnızca hata ayıklama derlemesinde: `CARLUI_PET_PHOTO_DEMO=<fotoğraf>`
/// akışı o fotoğrafla açık başlatır (dosya penceresi açmadan ekran görüntüsü
/// almak için). Sürüm derlemesinde hiçbir etkisi yok.
fn demo_state() -> State {
    let mut s = State::default();
    if cfg!(debug_assertions) {
        if let Some(p) = std::env::var_os("CARLUI_PET_PHOTO_DEMO").filter(|p| !p.is_empty()) {
            s.open = true;
            s.set_photo(PathBuf::from(p));
        }
    }
    s
}

pub fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

/// Arka plan silme modeli kullanıma hazır mı (motor + model kurulu)?
pub fn model_ready(fetching: usize) -> Result<(), String> {
    MODEL.with(|c| {
        let mut c = c.borrow_mut();
        match c.as_ref() {
            Some((t, n, r)) if *n == fetching && t.elapsed().as_secs() < 3 => r.clone(),
            _ => {
                let r = petgen::model().map(|_| ());
                *c = Some((Instant::now(), fetching, r.clone()));
                r
            }
        }
    })
}

impl State {
    /// İş sürüyorsa ilerlemesi (0..1).
    pub fn progress(&self) -> Option<f32> {
        self.job.as_ref().map(|j| j.shared.lock().map(|s| s.progress).unwrap_or(0.0))
    }

    pub fn has_cut(&self) -> bool {
        self.cut.is_some()
    }

    fn opts(&self) -> Opts {
        Opts { size: self.size, faces_left: self.faces_left, overwrite: false }
    }

    /// Önizleme ve paket için kullanılacak ad (boşsa yer tutucu).
    fn name_or_default(&self) -> String {
        let n = self.name.trim();
        if n.is_empty() { "pet".into() } else { n.to_string() }
    }

    /// Kesilmiş resimden hayvanı ve önizlemesini yeniden üretir (hızlı).
    pub fn remake(&mut self) {
        let Some(cut) = &self.cut else {
            self.pet = None;
            self.preview = None;
            return;
        };
        let pet = petgen::make(cut, &self.name_or_default(), &self.opts());
        self.preview = pet.as_ref().ok().map(|p| petgen::preview(p, 1));
        self.preview_gen += 1;
        self.pet = Some(pet);
    }

    /// Süren kesme işini durdurur (sonucu beklenmez).
    pub fn stop(&mut self) {
        if let Some(j) = self.job.take() {
            j.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Yeni fotoğraf: eski iş ve sonuçlar atılır. Ad boşsa ya da önceki
    /// fotoğrafın adıysa yeni dosyanın adı önerilir.
    pub fn set_photo(&mut self, path: PathBuf) {
        self.stop();
        let stem = |p: &PathBuf| p.file_stem().map(|s| s.to_string_lossy().chars().take(32).collect::<String>()).unwrap_or_default();
        if self.name.trim().is_empty() || self.photo.as_ref().is_some_and(|p| stem(p) == self.name) {
            self.name = stem(&path);
        }
        self.photo = Some(path);
        self.cut = None;
        self.pet = None;
        self.preview = None;
        self.preview_gen += 1;
        self.error = None;
        self.confirm = None;
        self.done = None;
    }

    /// Akışı kapatır: iş durur, bellekteki resimler bırakılır.
    pub fn close(&mut self) {
        self.stop();
        let done = self.done.take();
        *self = State { done, ..State::default() };
    }

    /// Biten işin sonucunu alır; fotoğraf seçili, kesilmemiş ve hata yoksa
    /// (model de hazırsa) işi başlatır. Çizimden çağrılır.
    pub fn poll(&mut self, hwnd: HWND, model_ok: bool) {
        if let Some(j) = &self.job {
            let res = j.shared.lock().ok().and_then(|mut s| s.result.take());
            if let Some(r) = res {
                let cancelled = j.cancel.load(Ordering::Relaxed);
                self.job = None;
                match r {
                    Ok(img) => {
                        self.cut = Some(img);
                        self.remake();
                    }
                    Err(e) if !cancelled => self.error = Some(e),
                    Err(_) => {}
                }
            }
            return;
        }
        // Durdurulmuş bir kesim hâlâ çalışıyorsa (motorun tek hesabı yarıda
        // kesilemeyebilir) yenisi onu bekler: iki model aynı anda belleğe yüklenmesin.
        if self.open && model_ok && self.photo.is_some() && self.cut.is_none() && self.error.is_none() && RUNNING.load(Ordering::SeqCst) == 0 {
            self.start(hwnd);
        }
    }

    fn start(&mut self, hwnd: HWND) {
        let Some(photo) = self.photo.clone() else { return };
        let cancel = Arc::new(AtomicBool::new(false));
        let shared: Arc<Mutex<Shared>> = Arc::default();
        let (c, s) = (cancel.clone(), shared.clone());
        let h = hwnd.0 as isize;
        let wake = move || unsafe {
            let _ = InvalidateRect(HWND(h as _), None, false);
        };
        RUNNING.fetch_add(1, Ordering::SeqCst);
        std::thread::spawn(move || {
            // Durdurulan iş de çıkınca sayaç düşer ve pencere uyanır (bekleyen
            // yeniden deneme o zaman başlar).
            struct Leave<F: Fn()>(F);
            impl<F: Fn()> Drop for Leave<F> {
                fn drop(&mut self) {
                    RUNNING.fetch_sub(1, Ordering::SeqCst);
                    (self.0)();
                }
            }
            let _leave = Leave(wake);
            // Yüzde değişmedikçe pencereyi boşuna yeniden çizdirme.
            let last = AtomicU32::new(u32::MAX);
            let prog = |f: f32| {
                let pct = (f.clamp(0.0, 1.0) * 100.0) as u32;
                if let Ok(mut st) = s.lock() {
                    st.progress = f.clamp(0.0, 1.0);
                }
                if last.swap(pct, Ordering::Relaxed) != pct {
                    wake();
                }
            };
            let r = petgen::cut_out(&photo, &c, &prog);
            if let Ok(mut st) = s.lock() {
                st.result = Some(r);
            }
            wake();
        });
        self.job = Some(Job { cancel, shared });
    }

    /// Paketi yazar. `Ok(Some(klasör))` oluşturuldu; `Ok(None)` aynı adlı paket
    /// var, onay bekleniyor (`confirm`); `Err` hata.
    pub fn create(&mut self, overwrite: bool) -> Result<Option<String>, String> {
        let name = self.name.trim().to_string();
        if name.is_empty() {
            return Err(t!("Hayvana bir ad verin.", "Give the pet a name.").into());
        }
        let Some(cut) = &self.cut else {
            return Err(t!("Önce bir fotoğraf seçin ve hazırlanmasını bekleyin.", "Pick a photo first and wait for it to be prepared.").into());
        };
        // Paketteki görünen ad yazılan ad olsun (önizleme yer tutucuyla yapılmış olabilir).
        let pet = petgen::make(cut, &name, &self.opts())?;
        let folder = petgen::pack::folder_name(&name);
        let root = carlui_core::live::pets_dir();
        if !overwrite && std::fs::symlink_metadata(root.join(&folder)).is_ok() {
            self.confirm = Some(folder);
            return Ok(None);
        }
        let dir = petgen::write(&pet, &root, overwrite)?;
        let made = dir.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or(folder);
        carlui_core::info!("fotoğraftan hayvan yapıldı: {}", dir.display());
        self.confirm = None;
        Ok(Some(made))
    }
}

/// Fotoğraf seçme penceresi (WIC'in çözdüğü biçimler); iptalde `None`.
pub fn pick_photo() -> Option<PathBuf> {
    use windows::core::{w, PWSTR};
    use windows::Win32::UI::{Controls::Dialogs::*, Input::KeyboardAndMouse::GetActiveWindow};
    let mut buf = [0u16; 1024];
    let tr = carlui_core::lang::is_tr();
    unsafe {
        let mut ofn = OPENFILENAMEW {
            lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: GetActiveWindow(),
            lpstrFilter: if tr {
                w!("Fotoğraflar (*.png;*.jpg;*.jpeg;*.webp;*.bmp;*.gif;*.tif;*.tiff)\0*.png;*.jpg;*.jpeg;*.webp;*.bmp;*.gif;*.tif;*.tiff\0\0")
            } else {
                w!("Photos (*.png;*.jpg;*.jpeg;*.webp;*.bmp;*.gif;*.tif;*.tiff)\0*.png;*.jpg;*.jpeg;*.webp;*.bmp;*.gif;*.tif;*.tiff\0\0")
            },
            lpstrFile: PWSTR(buf.as_mut_ptr()),
            nMaxFile: buf.len() as u32,
            lpstrTitle: if tr { w!("Hayvanın fotoğrafı") } else { w!("Photo of the pet") },
            Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
            ..Default::default()
        };
        if !GetOpenFileNameW(&mut ofn).as_bool() {
            return None;
        }
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Saydam zeminde dolu bir elips: "kesilmiş" yapay fotoğraf.
    fn blob(w: u32, h: u32) -> Image {
        let mut bgra = vec![0u8; (w * h * 4) as usize];
        let (cx, cy, rx, ry) = (w as f32 / 2.0, h as f32 / 2.0, w as f32 * 0.4, h as f32 * 0.35);
        for y in 0..h {
            for x in 0..w {
                let (dx, dy) = ((x as f32 - cx) / rx, (y as f32 - cy) / ry);
                if dx * dx + dy * dy <= 1.0 {
                    let i = ((y * w + x) * 4) as usize;
                    bgra[i..i + 4].copy_from_slice(&[40, 120 + (x % 60) as u8, 200, 255]);
                }
            }
        }
        Image { w, h, bgra }
    }

    #[test]
    fn boyut_ve_yon_degisince_onizleme_yenilenir() {
        let mut s = State { cut: Some(blob(240, 180)), ..State::default() };
        s.remake();
        let small = s.preview.as_ref().map(|p| (p.w, p.h)).unwrap();
        assert!(matches!(s.pet, Some(Ok(_))));
        let g = s.preview_gen;
        s.size = 64;
        s.remake();
        let big = s.preview.as_ref().map(|p| (p.w, p.h)).unwrap();
        assert!(big.0 > small.0 && big.1 > small.1, "{small:?} -> {big:?}");
        assert!(s.preview_gen > g);
        s.faces_left = true;
        s.remake();
        assert!(matches!(s.pet, Some(Ok(_))));
        // Varsayılan boyut seçicide var.
        assert!(SIZES.contains(&DEFAULT_SIZE));
    }

    #[test]
    fn bos_ad_ve_kesimsiz_olusturulmaz() {
        let mut s = State::default();
        assert!(s.create(false).is_err());
        s.name = "Pamuk".into();
        assert!(s.create(false).is_err());
        s.name = "   ".into();
        s.cut = Some(blob(120, 90));
        assert!(s.create(false).is_err());
    }

    #[test]
    fn yeni_foto_adi_onerir_ve_sifirlar() {
        let mut s = State { cut: Some(blob(60, 60)), error: Some("x".into()), ..State::default() };
        s.set_photo(PathBuf::from(r"C:\x\Pamuk.jpg"));
        assert_eq!(s.name, "Pamuk");
        assert!(!s.has_cut() && s.error.is_none());
        // Kullanıcının yazdığı ad korunur.
        s.name = "Tekir".into();
        s.set_photo(PathBuf::from(r"C:\x\Boncuk.png"));
        assert_eq!(s.name, "Tekir");
        // Ad önceki fotoğrafın adıysa yenisi önerilir.
        s.name = "Boncuk".into();
        s.set_photo(PathBuf::from(r"C:\x\Zeytin.webp"));
        assert_eq!(s.name, "Zeytin");
    }
}
