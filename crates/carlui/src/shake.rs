//! Sallayınca büyüt (KDE Plasma'daki gibi): fare yerinde ileri geri
//! sallanınca imleç yumuşakça 3 katına çıkar; sallamaya devam edildikçe her
//! yeni sallamada bir kat daha büyür. Sallama bırakılınca 2 saniye büyük
//! kalır, sonra yumuşakça eski boyutuna döner.
//!
//! Hareket ham girdiyle (`WM_INPUT`) izlenir; zamanlayıcı yalnızca imleç
//! büyükken çalışır. Büyük kareler önceden hazırlanır (CarlUI imleci için
//! önbellekte, Windows şemasında ilk kullanımda ölçeklenerek); büyüme
//! sırasında sistem imleci bu karelerle değiştirilir, çizimi Windows yapar.

use carlui_core::config::CursorConfig;
use carlui_core::cursor::{self, swap};
use carlui_core::{info, warn};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use windows::Win32::{
    Foundation::HWND,
    UI::{
        Input::{KeyboardAndMouse::*, *},
        WindowsAndMessaging::*,
    },
};

/// Canlandırma karesi: ana pencereye ekranın yenileme hızında gelir.
pub const FRAME: u32 = WM_APP + 8;
/// Örnekler arası en kısa süre (yüksek hızlı farelerde gereksiz iş olmasın).
const SAMPLE: Duration = Duration::from_millis(8);
/// İlk sallamada varılan kat; sonraki her sallama bu kadar ekler.
const FIRST: f32 = 3.0;
const STEP: f32 = 1.0;
/// Son sallamadan sonra imlecin büyük kaldığı süre.
const HOLD: Duration = Duration::from_millis(2000);
/// Büyüme ve küçülme canlandırması.
const ANIM: Duration = Duration::from_millis(200);
/// Sallama bu kadar geriye bakılarak aranır.
const WINDOW_MS: u64 = 1000;
/// Kat edilen yol, hareketin kapladığı alanın köşegeninin bu katını aşmalı.
const RATIO: f32 = 4.0;
/// Köşegen en az bu kadar (96 DPI pikseli) olmalı: küçük titremeler sayılmaz.
const MIN_SPAN: f32 = 100.0;

/// İki adım aynı yöne mi gidiyor? Bir pikselden kısa adımlar her yöne uyar.
fn same_way(a: i32, b: i32) -> bool {
    (a >= -1 && b >= -1) || (a <= 1 && b <= 1)
}

/// Son bir saniyenin dönüş noktalarından sallamayı tanır: kat edilen yol,
/// hareketin kapladığı alanın köşegeninin dört katını aşarsa fare yerinde
/// ileri geri gidiyordur. Tanıyınca geçmişi siler, böylece sallama sürdükçe
/// yeniden (ve yeniden) tanınır.
#[derive(Default)]
pub struct Detector {
    pts: VecDeque<(u64, i32, i32)>,
}

impl Detector {
    /// `t` milisaniye, `unit` ekran ölçeği (96 DPI'da 1).
    pub fn push(&mut self, t: u64, x: i32, y: i32, unit: f32) -> bool {
        while self.pts.front().is_some_and(|p| t.saturating_sub(p.0) >= WINDOW_MS) {
            self.pts.pop_front();
        }
        // Aynı yönde süren hareket yeni nokta açmaz, son noktayı ileri taşır:
        // geçmişte yalnızca dönüş noktaları kalır.
        let n = self.pts.len();
        if n >= 2 {
            let (_, lx, ly) = self.pts[n - 1];
            let (_, px, py) = self.pts[n - 2];
            if same_way(lx - px, x - lx) && same_way(ly - py, y - ly) {
                self.pts[n - 1] = (t, x, y);
                return false;
            }
        }
        self.pts.push_back((t, x, y));

        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        let mut path = 0.0f32;
        let mut prev: Option<(i32, i32)> = None;
        for &(_, x, y) in &self.pts {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
            if let Some((px, py)) = prev {
                path += ((x - px) as f32).hypot((y - py) as f32);
            }
            prev = Some((x, y));
        }
        let diag = ((x1 - x0) as f32).hypot((y1 - y0) as f32);
        if diag < MIN_SPAN * unit || path <= RATIO * diag {
            return false;
        }
        self.pts.clear();
        true
    }

    pub fn clear(&mut self) {
        self.pts.clear();
    }
}

/// Yavaş başlayıp yavaş biten geçiş (0..1 → 0..1).
fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// Büyüklüğün canlandırması: `from`'dan `to`'ya `ANIM` sürede.
#[derive(Clone, Copy)]
struct Anim {
    start: Instant,
    from: f32,
    to: f32,
}

impl Anim {
    fn at(&self, now: Instant) -> f32 {
        let t = now.saturating_duration_since(self.start).as_secs_f32() / ANIM.as_secs_f32();
        self.from + (self.to - self.from) * ease(t)
    }

    fn done(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.start) >= ANIM
    }
}

/// Bir sallamadan sonra varılacak kat: ilk sallamada `FIRST`, sonra her
/// sallamada bir kat fazlası, `max`'ı geçmeden.
fn next_target(target: f32, max: f32) -> f32 {
    let k = if target <= 1.0 { FIRST } else { target + STEP };
    k.min(max)
}

/// Büyümüş imleç: değiştirilen rol, kareleri ve o an gösterilen kademe.
struct Grow {
    swap: swap::Swap,
    frames: cursor::Grower,
    shown: Option<usize>,
}

impl Grow {
    fn begin(cfg: &CursorConfig) -> Option<Grow> {
        let (_, c) = swap::shown()?;
        let id = swap::role(c)?;
        let frames = cursor::Grower::new(cfg, id, c)?;
        let swap = swap::Swap::begin(id)?;
        Some(Grow { swap, frames, shown: None })
    }

    /// `k` katına en yakın kademeyi gösterir (değiştiyse).
    fn show(&mut self, k: f32) {
        let i = self.frames.level(k);
        if self.shown != Some(i) {
            if let Some(f) = self.frames.frame(i) {
                self.swap.show(f);
            }
            self.shown = Some(i);
        }
    }
}

struct State {
    on: bool,
    unit: f32,
    start: Instant,
    last: Option<Instant>,
    shook: Option<Instant>,
    /// Varılmak istenen kat (1 = asıl boyut) ve oraya giden canlandırma.
    target: f32,
    anim: Option<Anim>,
    det: Detector,
    grow: Option<Grow>,
}

impl State {
    fn now_k(&self, now: Instant) -> f32 {
        self.anim.map_or(self.target, |a| a.at(now))
    }

    fn animate(&mut self, to: f32, now: Instant) {
        if self.target != to {
            let from = self.now_k(now);
            self.anim = Some(Anim { start: now, from, to });
            self.target = to;
        }
    }

    fn reset(&mut self) {
        self.grow = None;
        self.shook = None;
        self.target = 1.0;
        self.anim = None;
        self.det.clear();
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State {
        on: false,
        unit: 1.0,
        start: Instant::now(),
        last: None,
        shook: None,
        target: 1.0,
        anim: None,
        det: Detector::default(),
        grow: None,
    });
}

fn pressed() -> bool {
    [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON].iter().any(|k| unsafe { GetAsyncKeyState(k.0 as i32) } < 0)
}

/// Fare hareketini dinlemeyi açar/kapatır (ayar kapalıyken ve tam ekranda
/// oyun varken hiç mesaj gelmesin).
pub fn listen(hwnd: HWND, on: bool) {
    STATE.with_borrow_mut(|s| {
        if !on {
            s.reset();
        }
        if s.on == on {
            return;
        }
        let dev = RAWINPUTDEVICE {
            usUsagePage: 1,
            usUsage: 2,
            dwFlags: if on { RIDEV_INPUTSINK } else { RIDEV_REMOVE },
            hwndTarget: if on { hwnd } else { HWND::default() },
        };
        match unsafe { RegisterRawInputDevices(&[dev], std::mem::size_of::<RAWINPUTDEVICE>() as u32) } {
            Ok(()) => {
                s.on = on;
                s.unit = (cursor::system_size() as f32 / 32.0).clamp(1.0, 3.0);
                info!("sallayınca büyüt: {}", if on { "açık" } else { "kapalı" });
            }
            Err(e) => warn!("sallama dinlenemiyor: {e}"),
        }
    });
}

/// `WM_INPUT`: fare oynadı.
pub fn input(hwnd: HWND, cfg: &CursorConfig) {
    STATE.with_borrow_mut(|s| {
        let now = Instant::now();
        if s.last.is_some_and(|l| now - l < SAMPLE) {
            return;
        }
        s.last = Some(now);
        // Sürüklerken (pencere, seçim) sallama sayılmaz.
        let Some((p, _)) = swap::shown().filter(|_| !pressed()) else {
            s.det.clear();
            return;
        };
        let t = (now - s.start).as_millis() as u64;
        if !s.det.push(t, p.x, p.y, s.unit) {
            return;
        }
        s.shook = Some(now);
        if s.grow.is_none() {
            s.grow = Grow::begin(cfg);
            if s.grow.is_none() {
                info!("sallama: bu imleç büyütülemedi");
                return;
            }
            s.target = 1.0;
            s.anim = None;
            clock::start(hwnd);
        }
        let max = s.grow.as_ref().map_or(1.0, |g| g.frames.max());
        let to = next_target(s.target, max);
        s.animate(to, now);
    });
}

/// `FRAME`: canlandırmayı ilerletir, sallama bittiyse küçültür.
pub fn tick(cfg: &CursorConfig) {
    clock::done();
    STATE.with_borrow_mut(|s| {
        let now = Instant::now();
        if s.grow.is_none() {
            clock::stop();
            return;
        }
        if s.target > 1.0 && s.shook.is_none_or(|t| now - t >= HOLD) {
            s.animate(1.0, now);
        }
        let k = s.now_k(now);
        // İmleç başka role geçti (ör. metin alanının üstüne geldi): onu büyüt.
        let now_id = swap::shown().and_then(|(_, c)| swap::role(c));
        if let (Some(id), Some(g)) = (now_id, s.grow.as_ref()) {
            if id != g.swap.id() && s.target > 1.0 {
                s.grow = None;
                s.grow = Grow::begin(cfg);
            }
        }
        let done = s.anim.is_none_or(|a| a.done(now));
        if done {
            s.anim = None;
        }
        if done && s.target <= 1.0 {
            s.reset();
            clock::stop();
            return;
        }
        match s.grow.as_mut() {
            Some(g) => {
                g.show(k);
                // Büyük beklerken küçülmenin karelerini hazırla: küçülürken
                // dosya okunup kare kaçmasın.
                if done {
                    let i = g.frames.level(k);
                    g.frames.warm(i);
                }
            }
            None => {
                s.reset();
                clock::stop();
            }
        }
    });
}

/// İmleci hemen asıl boyutuna döndürür (ayar değişti, imleç yeniden uygulanacak, kapanış).
pub fn stop() {
    clock::stop();
    STATE.with_borrow_mut(|s| s.reset());
}

/// Canlandırma saati: imleç büyükken ekranların en yüksek yenileme hızında
/// ana pencereye `FRAME` gönderir. `SetTimer` ~15,6 ms çözünürlüklü ve
/// titrek (gerçekte ~64 Hz); 144/240 Hz ekranda küçülme takılır gibi görünür.
/// Önceki kare işlenmeden yenisi gönderilmez, ana kuyruk dolmaz.
mod clock {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering::*};
    use std::sync::OnceLock;
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::Graphics::Gdi::*;

    static RUN: AtomicBool = AtomicBool::new(false);
    static PENDING: AtomicBool = AtomicBool::new(false);
    static TARGET: AtomicIsize = AtomicIsize::new(0);
    static PERIOD_NS: AtomicU64 = AtomicU64::new(16_666_667);
    static THREAD: OnceLock<std::thread::Thread> = OnceLock::new();

    /// Yenileme hızına göre kare süresi (60-500 Hz arası; 0/1 "varsayılan" demek).
    pub fn frame_period(hz: u32) -> Duration {
        let hz = if hz <= 1 { 60 } else { hz.clamp(60, 500) };
        Duration::from_nanos(1_000_000_000 / hz as u64)
    }

    /// Ekranların en yüksek yenileme hızı.
    fn max_hz() -> u32 {
        carlui_core::win::monitors()
            .into_iter()
            .filter_map(|m| unsafe {
                let name: Vec<u16> = carlui_core::win::device_name(m.handle).encode_utf16().chain([0]).collect();
                let mut d = DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
                EnumDisplaySettingsW(windows::core::PCWSTR(name.as_ptr()), ENUM_CURRENT_SETTINGS, &mut d)
                    .as_bool()
                    .then_some(d.dmDisplayFrequency)
            })
            .max()
            .unwrap_or(60)
    }

    pub fn start(hwnd: HWND) {
        let hz = max_hz();
        PERIOD_NS.store(frame_period(hz).as_nanos() as u64, Relaxed);
        TARGET.store(hwnd.0 as isize, Relaxed);
        PENDING.store(false, Relaxed);
        RUN.store(true, Release);
        THREAD.get_or_init(|| std::thread::Builder::new().name("shake-clock".into()).spawn(run).expect("saat").thread().clone()).unpark();
    }

    pub fn stop() {
        RUN.store(false, Release);
    }

    /// Kare işlendi: sıradaki gönderilebilir.
    pub fn done() {
        PENDING.store(false, Release);
    }

    fn run() {
        let mut next = Instant::now();
        loop {
            if !RUN.load(Acquire) {
                std::thread::park();
                next = Instant::now();
                continue;
            }
            let period = Duration::from_nanos(PERIOD_NS.load(Relaxed));
            next += period;
            let now = Instant::now();
            if next <= now {
                next = now + period;
            }
            std::thread::sleep(next - now);
            if RUN.load(Acquire) && !PENDING.swap(true, AcqRel) {
                let h = HWND(TARGET.load(Relaxed) as _);
                if unsafe { PostMessageW(h, FRAME, WPARAM(0), LPARAM(0)) }.is_err() {
                    PENDING.store(false, Release);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(d: &mut Detector, pts: impl IntoIterator<Item = (u64, i32, i32)>) -> usize {
        pts.into_iter().filter(|&(t, x, y)| d.push(t, x, y, 1.0)).count()
    }

    /// İleri geri gidiş: `legs` kez `amp` piksel, her biri `ms` sürede.
    fn zigzag(legs: u64, amp: i32, ms: u64) -> Vec<(u64, i32, i32)> {
        let mut v = Vec::new();
        let n = (ms / 8).max(1);
        for leg in 0..legs {
            for i in 0..=n {
                let f = i as f32 / n as f32;
                let x = if leg % 2 == 0 { (amp as f32 * f) as i32 } else { (amp as f32 * (1.0 - f)) as i32 };
                v.push((leg * ms + i * 8, 500 + x, 300 + (i as i32 % 3)));
            }
        }
        v
    }

    #[test]
    fn hizli_ileri_geri_sallama_taninir() {
        assert!(count(&mut Detector::default(), zigzag(6, 200, 80)) >= 1);
    }

    #[test]
    fn sallama_surdukce_tekrar_tekrar_taninir() {
        // 3 saniye boyunca saniyede ~6 gidiş: her saniye en az bir kez.
        assert!(count(&mut Detector::default(), zigzag(18, 200, 160)) >= 3);
    }

    #[test]
    fn duz_hizli_hareket_sallama_degil() {
        let v: Vec<_> = (0..60).map(|i| (i * 8, 100 + i as i32 * 25, 400 + i as i32 * 3)).collect();
        assert_eq!(count(&mut Detector::default(), v), 0);
    }

    #[test]
    fn yavas_ileri_geri_sallama_degil() {
        assert_eq!(count(&mut Detector::default(), zigzag(6, 200, 700)), 0);
    }

    #[test]
    fn kucuk_titreme_sallama_degil() {
        assert_eq!(count(&mut Detector::default(), zigzag(40, 60, 40)), 0);
    }

    #[test]
    fn her_sallamada_bir_kat_buyur() {
        let mut k = 1.0;
        let mut seen = Vec::new();
        for _ in 0..5 {
            k = next_target(k, 6.0);
            seen.push(k);
        }
        assert_eq!(seen, [3.0, 4.0, 5.0, 6.0, 6.0]);
        assert_eq!(next_target(1.0, 2.5), 2.5);
    }

    #[test]
    fn kare_suresi_ekranin_yenileme_hizinda() {
        use clock::frame_period;
        assert_eq!(frame_period(144), Duration::from_nanos(6_944_444));
        assert_eq!(frame_period(240), Duration::from_nanos(4_166_666));
        // Bilinmeyen (0/1) ve çok düşük hız 60 Hz, aşırı yüksek 500 Hz sayılır.
        assert_eq!(frame_period(0), frame_period(60));
        assert_eq!(frame_period(1), frame_period(60));
        assert_eq!(frame_period(30), frame_period(60));
        assert_eq!(frame_period(1000), Duration::from_millis(2));
    }

    #[test]
    fn gecis_yumusak_ve_uclarda_tam() {
        assert_eq!(ease(0.0), 0.0);
        assert_eq!(ease(1.0), 1.0);
        assert!(ease(0.1) < 0.1 && ease(0.9) > 0.9);
        assert!((ease(0.5) - 0.5).abs() < 1e-6);
    }
}
