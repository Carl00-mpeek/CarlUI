//! Bu modül "bilgisayarın sesi ile mikrofon tek bir ses izinde nasıl
//! birleşir?" sorununu çözüyor. İki kaynak (geri döngü ve mikrofon) ayrı
//! aygıtlardan, ayrı zamanlarda paket gönderir; her biri kendi zaman
//! damgasıyla (QPC) bir kuyruğa yazılır. Karıştırıcı sabit bir gecikmeyle
//! (100 ms) geride yürür: o ana kadar gelen örnekleri toplar, gelmeyen yeri
//! sessizlik sayar. Böylece hiçbir şey çalmıyorken de ses izi kesintisiz olur
//! ve kaynaklar kayıt sürerken açılıp kapatılabilir (geçmiş silinmez).

use crate::{
    audio::Loopback,
    denoise::{self, Denoiser},
    ring::{self, Ring},
    writer::{frames_to_100ns, AUDIO_CH, AUDIO_RATE},
};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

const MS: i64 = 10_000;
/// Karıştırıcının geriden gelme payı: geç gelen paketler de yetişir.
const LAG: i64 = 100 * MS;
const CH: usize = AUDIO_CH as usize;

/// Kayıt sürerken değiştirilebilen ses ayarları.
#[derive(Default)]
pub struct Flags {
    pub audio: AtomicBool,
    pub mic: AtomicBool,
}

/// Kaynakların kayıttaki düzeyi (yüzde, 0–200); ayardan ve panelden yazılır.
pub static GAIN_SYS: AtomicU32 = AtomicU32::new(100);
pub static GAIN_MIC: AtomicU32 = AtomicU32::new(100);
/// Son okumadan beri görülen en yüksek tepe (düzey uygulanmış, 0–32767);
/// panel `swap(0)` ile okur.
pub static LEVEL_SYS: AtomicU32 = AtomicU32::new(0);
pub static LEVEL_MIC: AtomicU32 = AtomicU32::new(0);

pub fn gain(g: &AtomicU32) -> f32 {
    g.load(Ordering::Relaxed).min(200) as f32 / 100.0
}

/// Örneklerin düzey uygulanmış tepe değeri.
pub fn peak(pcm: &[i16], gain: f32) -> u32 {
    let m = pcm.iter().map(|&v| (v as i32).unsigned_abs()).max().unwrap_or(0);
    ((m as f32 * gain) as u32).min(32767)
}

fn frames(d: i64) -> usize {
    (d.max(0) as u64 * AUDIO_RATE as u64 / 10_000_000) as usize
}

/// Bir kaynağın zaman damgalı örnek kuyruğu (48 kHz stereo).
#[derive(Default)]
struct Track {
    q: VecDeque<i16>,
    /// Kuyruğun ilk örneğinin zamanı (QPC, 100 ns).
    t: i64,
}

impl Track {
    fn end(&self) -> i64 {
        self.t + frames_to_100ns((self.q.len() / CH) as u64)
    }

    fn push(&mut self, pcm: &[i16], t: i64) {
        if self.q.is_empty() {
            self.t = t;
            self.q.extend(pcm);
            return;
        }
        let end = self.end();
        let mut pcm = pcm;
        if t > end + 20 * MS {
            // Kaynak bir süre sustu: aradaki boşluk sessizlik.
            self.q.extend(std::iter::repeat(0).take(frames(t - end) * CH));
        } else if t < end - 20 * MS {
            // Aygıt saatleri kaydı: üst üste gelen kısım atılır.
            let skip = (frames(end - t) * CH).min(pcm.len());
            pcm = &pcm[skip..];
        }
        self.q.extend(pcm);
        // Takılan bir kaynak belleği doldurmasın (en çok 2 sn).
        let max = AUDIO_RATE as usize * CH * 2;
        if self.q.len() > max {
            let d = (self.q.len() - max) / CH * CH;
            self.q.drain(..d);
            self.t += frames_to_100ns((d / CH) as u64);
        }
    }

    /// `[from, from + n kare)` aralığını `out`'a ekler ve kuyruktan düşer.
    /// Örnekler `gain` ile çarpılır; dönüş: eklenen kısmın tepe değeri.
    fn take(&mut self, from: i64, out: &mut [i32], gain: f32) -> u32 {
        let n = out.len() / CH;
        if self.q.is_empty() {
            return 0;
        }
        if self.t < from {
            let d = (frames(from - self.t) * CH).min(self.q.len());
            self.q.drain(..d);
            self.t += frames_to_100ns((d / CH) as u64);
            if self.q.is_empty() {
                return 0;
            }
        }
        // Kuyruk `from`'dan sonra başlıyorsa baştaki kısım sessiz.
        let off = frames(self.t - from).min(n);
        let avail = (self.q.len() / CH).min(n - off);
        let mut pk = 0u32;
        for (i, v) in self.q.drain(..avail * CH).enumerate() {
            let s = (v as f32 * gain) as i32;
            pk = pk.max(s.unsigned_abs());
            out[off * CH + i] += s;
        }
        self.t += frames_to_100ns(avail as u64);
        pk.min(32767)
    }

    fn clear(&mut self) {
        self.q.clear();
    }
}

/// Bir yakalama aygıtı: istenince açılır, aygıt değişince yeniden kurulur.
struct Source {
    mic: bool,
    cap: Option<Loopback>,
    retry: Instant,
    checked: Instant,
    track: Track,
    /// Yalnızca mikrofonda, gürültü engelleme açıksa kullanılır.
    dn: Option<Denoiser>,
}

impl Source {
    fn new(mic: bool) -> Self {
        let past = Instant::now() - Duration::from_secs(10);
        Self { mic, cap: None, retry: past, checked: Instant::now(), track: Track::default(), dn: None }
    }

    unsafe fn poll(&mut self, want: bool) {
        if !want {
            if self.cap.take().is_some() {
                info!("{} kaydı kapandı", self.name());
            }
            self.track.clear();
            self.dn = None;
            return;
        }
        if self.cap.is_none() {
            if self.retry.elapsed() < Duration::from_secs(3) {
                return;
            }
            self.retry = Instant::now();
            match Loopback::open(self.mic) {
                Ok(c) => {
                    info!("{} kaydı açık", self.name());
                    self.cap = Some(c);
                    self.checked = Instant::now();
                }
                Err(e) => {
                    warn!("{} açılamadı: {e}", self.name());
                    return;
                }
            }
        }
        // Gürültü engelleme açılıp kapanınca süzgeç baştan kurulur.
        let want_dn = self.mic && denoise::ON.load(Ordering::Relaxed);
        if want_dn != self.dn.is_some() {
            self.dn = want_dn.then(Denoiser::default);
        }
        let track = &mut self.track;
        let dn = &mut self.dn;
        let ok = self.cap.as_mut().unwrap().read(|pcm, t| match dn {
            Some(d) => d.process(pcm, t, |p, t| track.push(p, t)),
            None => track.push(pcm, t),
        });
        if ok.is_err() {
            self.cap = None;
            self.dn = None;
            return;
        }
        // Varsayılan aygıt değiştiyse (kulaklık takıldı) yenisine geç.
        if self.checked.elapsed() >= Duration::from_secs(3) {
            self.checked = Instant::now();
            if self.cap.as_ref().is_some_and(|c| crate::audio::default_id(self.mic).is_some_and(|id| id != c.id)) {
                info!("varsayılan {} değişti", self.name());
                self.cap = None;
                self.retry = Instant::now() - Duration::from_secs(10);
            }
        }
    }

    fn name(&self) -> &'static str {
        if self.mic {
            "mikrofon"
        } else {
            "bilgisayar sesi"
        }
    }
}

/// Ses iş parçacığı: iki kaynağı okur, karıştırır, kayda yazar.
pub unsafe fn run(stop: Arc<AtomicBool>, cur: Arc<Mutex<Option<Arc<Ring>>>>, flags: Arc<Flags>) {
    let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
    let mut sys = Source::new(false);
    let mut mic = Source::new(true);
    let mut out_t = ring::now() - LAG;
    let mut buf: Vec<i32> = Vec::new();
    let mut pcm: Vec<i16> = Vec::new();
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(10));
        sys.poll(flags.audio.load(Ordering::Relaxed));
        mic.poll(flags.mic.load(Ordering::Relaxed));
        let horizon = ring::now() - LAG;
        let n = frames(horizon - out_t);
        if n == 0 {
            continue;
        }
        if n > AUDIO_RATE as usize {
            // Uzun bir duraklama (uyku): geçmişi doldurmaya çalışma.
            out_t = horizon;
            continue;
        }
        buf.clear();
        buf.resize(n * CH, 0);
        LEVEL_SYS.fetch_max(sys.track.take(out_t, &mut buf, gain(&GAIN_SYS)), Ordering::Relaxed);
        LEVEL_MIC.fetch_max(mic.track.take(out_t, &mut buf, gain(&GAIN_MIC)), Ordering::Relaxed);
        pcm.clear();
        pcm.extend(buf.iter().map(|&v| v.clamp(i16::MIN as i32, i16::MAX as i32) as i16));
        if let Some(r) = cur.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            r.audio(&pcm, out_t);
        }
        out_t += frames_to_100ns(n as u64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_aligns() {
        let mut t = Track::default();
        // 10 ms (480 kare) 1'ler, 1 sn'de başlıyor.
        t.push(&vec![1; 480 * CH], 10_000_000);
        let mut out = vec![0i32; 960 * CH];
        // 5 ms önceden iste: ilk 240 kare sessiz, sonra 480 kare 1, sonra sessiz.
        t.take(10_000_000 - 50_000, &mut out, 1.0);
        let ones = out.iter().filter(|&&v| v == 1).count() / CH;
        assert_eq!(ones, 480);
        assert_eq!(out[0], 0);
        assert_eq!(out[240 * CH], 1);
        assert!(t.q.is_empty());
    }

    #[test]
    fn track_gap_and_drop_old() {
        let mut t = Track::default();
        t.push(&vec![2; 480 * CH], 0);
        // 50 ms boşluk: sessizlikle dolar.
        t.push(&vec![3; 480 * CH], 600_000);
        assert_eq!(t.q.len() / CH, 480 + 2400 + 480);
        let mut out = vec![0i32; 480 * CH];
        // İlk 60 ms geçmişte kaldı: yalnızca son paket gelir.
        t.take(600_000, &mut out, 1.0);
        assert!(out.iter().all(|&v| v == 3));
    }
}
