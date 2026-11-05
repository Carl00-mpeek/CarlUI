//! Bu modül "son N dakikayı RAM'i doldurmadan nasıl tutarız?" sorununu
//! çözüyor: kayıt diske ~10 saniyelik MP4 segmentleri olarak yazılır, en eski
//! segmentler süre dolunca silinir (30 dakika yüksek kalitede bile RAM
//! kullanmaz). Bir sonraki segmentin yazıcısı önceden arka planda açılır;
//! geçiş anında kare kaçmaz. Biten segment ayrı bir iş parçacığında kapatılır.
//!
//! Ses görüntüyle aynı saatten (QPC) zaman damgası alır. Sessizlikte Windows
//! paket göndermez; boşluklar sessizlikle doldurulur ki ses kaymasın.

use crate::writer::{frames_to_100ns, Format, Writer, AUDIO_CH, AUDIO_RATE};
use carlui_core::{info, warn};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
};
use windows::Win32::{Graphics::Direct3D11::ID3D11Texture2D, Media::MediaFoundation::IMFDXGIDeviceManager};

/// Segment uzunluğu (100 ns).
pub const SEG: i64 = 10 * 10_000_000;
const MS: i64 = 10_000;

#[derive(Clone, Debug)]
pub struct Segment {
    pub path: PathBuf,
    /// Süre (100 ns), görüntünün ilk karesinden kesilene kadar.
    pub dur: i64,
}

struct Live {
    w: Writer,
    path: PathBuf,
    /// Segmentin başlangıcı (QPC, 100 ns).
    start: i64,
    /// Yazılan sesin sonu (segment başına göre).
    audio: i64,
}

enum Job {
    Finish(Live, i64, Option<mpsc::Sender<()>>),
}

pub struct Ring {
    pub fmt: Format,
    dm: IMFDXGIDeviceManager,
    dir: PathBuf,
    /// Tutulacak süre (100 ns).
    keep: AtomicU64,
    live: Mutex<Option<Live>>,
    next: Mutex<Option<(Writer, PathBuf)>>,
    done: Mutex<VecDeque<Segment>>,
    /// Kaydetme sürerken eski segmentler silinmez.
    holds: AtomicUsize,
    counter: AtomicU64,
    jobs: Mutex<Option<mpsc::Sender<Job>>>,
    stopped: std::sync::atomic::AtomicBool,
}

// Media Foundation nesneleri serbest iş parçacıklıdır; erişim kilitlerle sıralanır.
unsafe impl Send for Ring {}
unsafe impl Sync for Ring {}

impl Ring {
    pub fn new(fmt: Format, dm: IMFDXGIDeviceManager, dir: PathBuf, keep_minutes: u32) -> Arc<Self> {
        let (tx, rx) = mpsc::channel::<Job>();
        let ring = Arc::new(Self {
            fmt,
            dm,
            dir,
            keep: AtomicU64::new(keep_minutes as u64 * 60 * 10_000_000),
            live: Mutex::new(None),
            next: Mutex::new(None),
            done: Mutex::new(VecDeque::new()),
            holds: AtomicUsize::new(0),
            counter: AtomicU64::new(0),
            jobs: Mutex::new(Some(tx)),
            stopped: std::sync::atomic::AtomicBool::new(false),
        });
        let weak = Arc::downgrade(&ring);
        let _ = std::thread::Builder::new().name("replay-finish".into()).spawn(move || {
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
            }
            for Job::Finish(live, end, ack) in rx {
                let dur = end - live.start;
                let frames = live.w.frames;
                let ok = unsafe { live.w.finish() };
                match ok {
                    Ok(()) if frames > 0 => {
                        if let Some(r) = weak.upgrade() {
                            r.done.lock().unwrap_or_else(|e| e.into_inner()).push_back(Segment { path: live.path, dur });
                            r.prune();
                        } else {
                            let _ = std::fs::remove_file(&live.path);
                        }
                    }
                    Ok(()) => {
                        let _ = std::fs::remove_file(&live.path);
                    }
                    Err(e) => {
                        warn!("tekrar segmenti kapatılamadı: {e}");
                        let _ = std::fs::remove_file(&live.path);
                    }
                }
                if let Some(a) = ack {
                    let _ = a.send(());
                }
            }
        });
        ring.prepare();
        ring
    }

    pub fn set_keep(&self, minutes: u32) {
        self.keep.store(minutes as u64 * 60 * 10_000_000, Ordering::SeqCst);
        self.prune();
    }

    fn path(&self) -> PathBuf {
        let n = self.counter.fetch_add(1, Ordering::SeqCst);
        self.dir.join(format!("seg-{}-{n:05}.mp4", std::process::id()))
    }

    /// Bir sonraki segmentin yazıcısını arka planda açar.
    fn prepare(self: &Arc<Self>) {
        let me = self.clone();
        let _ = std::thread::Builder::new().name("replay-prep".into()).spawn(move || unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
            let p = me.path();
            match Writer::open(&p, &me.fmt, &me.dm) {
                Ok(w) => {
                    let mut next = me.next.lock().unwrap_or_else(|e| e.into_inner());
                    // Durdurulduysa ya da başka bir hazırlık önce bitirdiyse gerek yok.
                    if me.stopped.load(Ordering::SeqCst) || next.is_some() {
                        discard(w, &p);
                    } else {
                        *next = Some((w, p));
                    }
                }
                Err(e) => {
                    warn!("tekrar yazıcısı açılamadı: {e}");
                    let _ = std::fs::remove_file(&p);
                }
            }
        });
    }

    /// Hazır yazıcıyı alır; yoksa (ilk kare ya da hazırlık yetişmediyse) hemen açar.
    unsafe fn take_next(self: &Arc<Self>) -> Option<(Writer, PathBuf)> {
        let ready = self.next.lock().unwrap_or_else(|e| e.into_inner()).take();
        let got = match ready {
            Some(x) => Some(x),
            None => {
                let p = self.path();
                match Writer::open(&p, &self.fmt, &self.dm) {
                    Ok(w) => Some((w, p)),
                    Err(e) => {
                        warn!("tekrar yazıcısı açılamadı: {e}");
                        let _ = std::fs::remove_file(&p);
                        None
                    }
                }
            }
        };
        if got.is_some() {
            self.prepare();
        }
        got
    }

    fn send(&self, job: Job) {
        if let Some(tx) = self.jobs.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = tx.send(job);
        }
    }

    /// Bir kare yazar; `now` QPC (100 ns), `dur` kare süresi.
    pub unsafe fn video(self: &Arc<Self>, tex: &ID3D11Texture2D, now: i64, dur: i64) {
        let mut live = self.live.lock().unwrap_or_else(|e| e.into_inner());
        let switch = live.as_ref().is_some_and(|l| now - l.start >= SEG);
        if switch || live.is_none() {
            // Hazırlık yetişmediyse eski segmentle devam et (kare kaçmasın).
            if switch && self.next.lock().unwrap_or_else(|e| e.into_inner()).is_none() && now - live.as_ref().unwrap().start < SEG * 2 {
            } else if let Some((w, path)) = self.take_next() {
                if let Some(old) = live.replace(Live { w, path, start: now, audio: 0 }) {
                    self.send(Job::Finish(old, now, None));
                }
            }
        }
        let Some(l) = live.as_mut() else { return };
        if let Err(e) = l.w.video(tex, now - l.start, dur) {
            warn!("kare yazılamadı: {e}");
        }
    }

    unsafe fn silence(l: &mut Live, until: i64) {
        // Bir seferde en çok 1 sn.
        while until - l.audio > 5 * MS {
            let span = (until - l.audio).min(1000 * MS);
            let frames = (span as u64 * AUDIO_RATE as u64 / 10_000_000) as usize;
            if frames == 0 {
                break;
            }
            let zeros = vec![0i16; frames * AUDIO_CH as usize];
            let _ = l.w.audio(&zeros, l.audio);
            l.audio += frames_to_100ns(frames as u64);
        }
    }

    /// 48 kHz stereo i16 ses yazar; `t`: ilk örneğin QPC zamanı (100 ns).
    pub unsafe fn audio(&self, pcm: &[i16], t: i64) {
        let mut live = self.live.lock().unwrap_or_else(|e| e.into_inner());
        let Some(l) = live.as_mut() else { return };
        let rel = t - l.start;
        let frames = (pcm.len() / AUDIO_CH as usize) as u64;
        if rel + frames_to_100ns(frames) <= 0 {
            return;
        }
        if rel > l.audio + 20 * MS {
            Self::silence(l, rel);
        }
        if let Err(e) = l.w.audio(pcm, l.audio) {
            warn!("ses yazılamadı: {e}");
        }
        l.audio += frames_to_100ns(frames);
    }

    /// Ses gelmiyorsa (sessizlik) boşluğu doldurur.
    pub unsafe fn audio_idle(&self, now: i64) {
        let mut live = self.live.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(l) = live.as_mut() {
            let until = now - l.start - 60 * MS;
            if until - l.audio > 100 * MS {
                Self::silence(l, until);
            }
        }
    }

    /// Eski segmentleri siler (kaydetme sürmüyorsa).
    fn prune(&self) {
        if self.holds.load(Ordering::SeqCst) > 0 {
            return;
        }
        let keep = self.keep.load(Ordering::SeqCst) as i64 + SEG;
        let mut done = self.done.lock().unwrap_or_else(|e| e.into_inner());
        let mut total: i64 = done.iter().map(|s| s.dur).sum();
        while done.len() > 1 && total - done[0].dur >= keep {
            let s = done.pop_front().unwrap();
            total -= s.dur;
            let _ = std::fs::remove_file(&s.path);
        }
    }

    /// Kaydetmek için: açık segmenti keser, kapanmasını bekler ve son
    /// `keep` süresini kapsayan segmentleri (eskiden yeniye) döndürür.
    /// `release` çağrılana kadar bu segmentler silinmez.
    pub fn cut(self: &Arc<Self>, now: i64) -> Vec<Segment> {
        self.holds.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        let old = self.live.lock().unwrap_or_else(|e| e.into_inner()).take();
        match old {
            Some(l) => self.send(Job::Finish(l, now, Some(tx))),
            None => {
                let _ = tx.send(());
            }
        }
        let _ = rx.recv_timeout(std::time::Duration::from_secs(10));
        let keep = self.keep.load(Ordering::SeqCst) as i64;
        let done = self.done.lock().unwrap_or_else(|e| e.into_inner());
        let mut out = Vec::new();
        let mut total = 0;
        for s in done.iter().rev() {
            if total >= keep {
                break;
            }
            total += s.dur;
            out.push(s.clone());
        }
        out.reverse();
        info!("tekrar kesildi: {} segment, {:.1} sn", out.len(), total as f64 / 1e7);
        out
    }

    pub fn release(&self) {
        self.holds.fetch_sub(1, Ordering::SeqCst);
        self.prune();
    }

    /// Kaydı bitirir ve tüm geçici dosyaları siler.
    pub fn stop(&self) {
        let live = self.live.lock().unwrap_or_else(|e| e.into_inner()).take();
        let (tx, rx) = mpsc::channel();
        if let Some(l) = live {
            self.send(Job::Finish(l, 0, Some(tx)));
            let _ = rx.recv_timeout(std::time::Duration::from_secs(10));
        }
        *self.jobs.lock().unwrap_or_else(|e| e.into_inner()) = None;
        {
            let mut next = self.next.lock().unwrap_or_else(|e| e.into_inner());
            self.stopped.store(true, Ordering::SeqCst);
            if let Some((w, p)) = next.take() {
                discard(w, &p);
            }
        }
        for s in self.done.lock().unwrap_or_else(|e| e.into_inner()).drain(..) {
            let _ = std::fs::remove_file(&s.path);
        }
        // Media Foundation dosyayı bir süre açık tutabilir: kalanları birkaç kez dene.
        let prefix = format!("seg-{}-", std::process::id());
        for _ in 0..20 {
            let left: Vec<_> = std::fs::read_dir(&self.dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&prefix)))
                .filter(|p| std::fs::remove_file(p).is_err())
                .collect();
            if left.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

/// Kullanılmamış yazıcıyı kapatıp dosyasını siler (kapatmadan dosya kilitli kalır).
fn discard(w: Writer, p: &std::path::Path) {
    let _ = unsafe { w.finish() };
    let _ = std::fs::remove_file(p);
}

/// Şu anki zaman (QPC, 100 ns); WASAPI'nin paket zamanlarıyla aynı saat.
pub fn now() -> i64 {
    use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    static FREQ: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    let f = *FREQ.get_or_init(|| {
        let mut f = 0;
        let _ = unsafe { QueryPerformanceFrequency(&mut f) };
        f.max(1)
    });
    let mut c = 0;
    let _ = unsafe { QueryPerformanceCounter(&mut c) };
    (c as i128 * 10_000_000 / f as i128) as i64
}
