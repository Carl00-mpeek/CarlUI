//! Bu modül "PDF sayfalarını hızlı ve az bellekle ekrana çizelim" sorununu
//! çözüyor: Windows'un kendi PDF motoru (Windows.Data.Pdf, Edge ile aynı
//! ailenin) kullanılır; ek kitaplık yok. Sayfa çizimi pencereyi dondurmasın
//! diye belge açıkken tek bir yardımcı iş parçacığında yapılır; belge
//! kapanınca iş parçacığı da biter. İstenen iş listesi her karede yenilenir:
//! kaydırılıp geçilen sayfa için boşuna çizim yapılmaz.

pub mod edit;
mod ops;
pub mod view;

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Condvar, Mutex,
    },
    thread::JoinHandle,
};
use windows::{
    core::Interface,
    Data::Pdf::{PdfDocument, PdfPageRenderOptions},
    Graphics::Imaging::BitmapEncoder,
    Storage::Streams::{DataReader, DataWriter, IRandomAccessStream, InMemoryRandomAccessStream},
    Win32::{
        Foundation::HWND,
        Graphics::Gdi::InvalidateRect,
        System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED},
    },
};

fn load(bytes: &[u8]) -> windows::core::Result<PdfDocument> {
    let s = InMemoryRandomAccessStream::new()?;
    let w = DataWriter::CreateDataWriter(&s)?;
    w.WriteBytes(bytes)?;
    w.StoreAsync()?.get()?;
    w.DetachStream()?;
    s.Seek(0)?;
    let rs: IRandomAccessStream = s.cast()?;
    PdfDocument::LoadFromStreamAsync(&rs)?.get()
}

/// Sayfa boyutları (DIP, sayfanın kendi döndürmesi uygulanmış).
pub fn sizes(bytes: &[u8]) -> Result<Vec<(f32, f32)>, String> {
    let doc = load(bytes).map_err(|e| {
        if e.code().0 as u32 == 0x8007_052B || e.message().to_lowercase().contains("password") {
            carlui_core::t!("Bu PDF parola korumalı; açılamıyor.", "This PDF is password protected and can't be opened.").to_string()
        } else {
            format!("{}: {}", carlui_core::t!("PDF açılamadı", "Couldn't open the PDF"), e.message())
        }
    })?;
    let n = doc.PageCount().map_err(|e| e.message())?;
    let mut out = Vec::with_capacity(n as usize);
    for i in 0..n {
        let s = doc.GetPage(i).and_then(|p| p.Size()).map(|s| (s.Width, s.Height)).unwrap_or((816.0, 1056.0));
        out.push((s.0.max(1.0), s.1.max(1.0)));
    }
    if out.is_empty() {
        return Err(carlui_core::t!("PDF'de sayfa yok.", "The PDF has no pages.").into());
    }
    Ok(out)
}

/// Sayfayı `width` piksel genişliğinde BMP olarak çizer.
fn render(doc: &PdfDocument, page: usize, width: u32) -> windows::core::Result<Vec<u8>> {
    let p = doc.GetPage(page as u32)?;
    // Çok ince uzun bir sayfa (1 × 200000 pt) dev bir bitmap istetmesin.
    let s = p.Size()?;
    let ratio = (s.Height.max(1.0) / s.Width.max(1.0)) as f64;
    // Kenarlar da Direct2D sınırında kalsın; saçma oranlı sayfa biraz basık çizilir.
    let side = crate::guard::SIDE;
    let most = ((crate::guard::PIXELS as f64 / ratio).sqrt() as u32).clamp(1, side);
    let w = width.clamp(1, most);
    let h = ((w as f64 * ratio).round() as u32).clamp(1, side);
    let o = PdfPageRenderOptions::new()?;
    o.SetDestinationWidth(w)?;
    o.SetDestinationHeight(h)?;
    o.SetIsIgnoringHighContrast(true)?;
    // BMP: sıkıştırma yok, PNG'ye göre çok daha hızlı yazılır ve çözülür.
    o.SetBitmapEncoderId(BitmapEncoder::BmpEncoderId()?)?;
    let s = InMemoryRandomAccessStream::new()?;
    p.RenderWithOptionsToStreamAsync(&s, &o)?.get()?;
    let n = s.Size()? as u32;
    let r = DataReader::CreateDataReader(&s.GetInputStreamAt(0)?)?;
    r.LoadAsync(n)?.get()?;
    let mut buf = vec![0u8; n as usize];
    r.ReadBytes(&mut buf)?;
    Ok(buf)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Job {
    pub page: usize,
    pub width: u32,
    pub thumb: bool,
}

#[derive(Default)]
struct Queue {
    jobs: Vec<Job>,
    quit: bool,
}

type Shared = Arc<(Mutex<Queue>, Condvar)>;

/// Arka planda sayfa çizen iş parçacığı (yalnızca belge açıkken yaşar).
pub struct Renderer {
    q: Shared,
    busy: Arc<AtomicBool>,
    rx: mpsc::Receiver<(Job, Option<Vec<u8>>)>,
    th: Option<JoinHandle<()>>,
}

impl Renderer {
    pub fn start(bytes: Arc<Vec<u8>>, hwnd: isize) -> Self {
        let q: Shared = Arc::default();
        let busy = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let (q2, b2) = (q.clone(), busy.clone());
        let th = std::thread::Builder::new()
            .name("pdf".into())
            .spawn(move || {
                unsafe {
                    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                }
                let doc = load(&bytes).ok();
                drop(bytes);
                loop {
                    let job = {
                        let (m, cv) = &*q2;
                        let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
                        while g.jobs.is_empty() && !g.quit {
                            g = cv.wait(g).unwrap_or_else(|e| e.into_inner());
                        }
                        if g.quit {
                            break;
                        }
                        b2.store(true, Ordering::SeqCst);
                        g.jobs.remove(0)
                    };
                    let out = doc.as_ref().and_then(|d| render(d, job.page, job.width).ok());
                    let gone = tx.send((job, out)).is_err();
                    // Pencere yeniden çizilsin (başka iş parçacığından güvenli).
                    unsafe {
                        let _ = InvalidateRect(HWND(hwnd as _), None, false);
                    }
                    b2.store(false, Ordering::SeqCst);
                    if gone {
                        break;
                    }
                }
                drop(doc);
                unsafe { CoUninitialize() };
            })
            .ok();
        Renderer { q, busy, rx, th }
    }

    /// İstenen işleri (öncelik sırasıyla) bildirir; eski liste atılır.
    pub fn want(&self, jobs: Vec<Job>) {
        let (m, cv) = &*self.q;
        let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
        if g.jobs != jobs {
            g.jobs = jobs;
            cv.notify_one();
        }
    }

    pub fn results(&self) -> Vec<(Job, Option<Vec<u8>>)> {
        self.rx.try_iter().collect()
    }

    /// Bekleyen ya da süren iş var mı.
    pub fn working(&self) -> bool {
        self.busy.load(Ordering::SeqCst) || !self.q.0.lock().map(|g| g.jobs.is_empty()).unwrap_or(true)
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        {
            let (m, cv) = &*self.q;
            let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
            g.quit = true;
            g.jobs.clear();
            cv.notify_one();
        }
        if let Some(t) = self.th.take() {
            let _ = t.join();
        }
    }
}
