//! Sallayınca büyüt: fare hızlıca ileri geri sallanınca imleç birkaç kat
//! büyür, sallama bitince eski boyutuna döner.
//!
//! Hareket ham girdiyle (`WM_INPUT`) izlenir; pencere ya da zamanlayıcı
//! yalnızca imleç büyükken çalışır. Büyük kareler önceden hazırlanır (CarlUI
//! imleci için önbellekte, Windows şeması için şemanın kendi dosyası); büyüme
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

/// Ana pencerenin büyüme zamanlayıcısı (7 karşılamanın).
pub const TIMER: usize = 8;
const TICK_MS: u32 = 16;
/// Örnekler arası en kısa süre (yüksek hızlı farelerde gereksiz iş olmasın).
const SAMPLE: Duration = Duration::from_millis(8);
/// Son sallamadan sonra imlecin büyük kaldığı süre.
const HOLD: Duration = Duration::from_millis(650);
/// Sallama bu kadar geriye bakılarak aranır.
const WINDOW_MS: u64 = 500;

/// Son yarım saniyenin konumlarından sallamayı tanır: kat edilen yol, hareketin
/// kapladığı alanın köşegeninin birkaç katıysa fare yerinde ileri geri gidiyordur.
#[derive(Default)]
pub struct Detector {
    pts: VecDeque<(u64, i32, i32)>,
}

impl Detector {
    /// `t` milisaniye, `unit` ekran ölçeği (96 DPI'da 1).
    pub fn push(&mut self, t: u64, x: i32, y: i32, unit: f32) -> bool {
        if self.pts.back().is_some_and(|&(_, px, py)| px == x && py == y) {
            return false;
        }
        self.pts.push_back((t, x, y));
        while self.pts.front().is_some_and(|p| t.saturating_sub(p.0) > WINDOW_MS) {
            self.pts.pop_front();
        }
        if self.pts.len() < 6 {
            return false;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        let mut path = 0.0f32;
        let mut prev: Option<(i32, i32)> = None;
        for &(_, x, y) in &self.pts {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
            if let Some((px, py)) = prev {
                path += (((x - px) as f32).powi(2) + ((y - py) as f32).powi(2)).sqrt();
            }
            prev = Some((x, y));
        }
        let diag = (((x1 - x0) as f32).powi(2) + ((y1 - y0) as f32).powi(2)).sqrt();
        diag >= 40.0 * unit && path >= 300.0 * unit && path >= 3.0 * diag
    }

    pub fn clear(&mut self) {
        self.pts.clear();
    }
}

/// Büyümüş imleç: değiştirilen rol ve kareleri (0 = asıl boyut).
struct Grow {
    swap: swap::Swap,
    frames: Vec<windows::Win32::UI::WindowsAndMessaging::HCURSOR>,
    step: usize,
}

impl Grow {
    fn begin(cfg: &CursorConfig) -> Option<Grow> {
        let (_, c) = swap::shown()?;
        let id = swap::role(c)?;
        let frames = cursor::grow_frames(cfg, id, c);
        if frames.is_empty() {
            return None;
        }
        let swap = swap::Swap::begin(id)?;
        Some(Grow { swap, frames, step: 0 })
    }

    fn set(&mut self, step: usize) {
        self.step = step.min(self.frames.len());
        if self.step > 0 {
            self.swap.show(self.frames[self.step - 1]);
        }
    }
}

impl Drop for Grow {
    fn drop(&mut self) {
        for &f in &self.frames {
            unsafe {
                let _ = DestroyCursor(f);
            }
        }
        // `swap` ardından düşer ve asıl imleci geri koyar.
    }
}

struct State {
    on: bool,
    unit: f32,
    start: Instant,
    last: Option<Instant>,
    shook: Option<Instant>,
    det: Detector,
    grow: Option<Grow>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State {
        on: false,
        unit: 1.0,
        start: Instant::now(),
        last: None,
        shook: None,
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
            s.grow = None;
            s.det.clear();
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
            }
            if s.grow.is_some() {
                unsafe { SetTimer(hwnd, TIMER, TICK_MS, None) };
            }
        }
    });
}

/// Zamanlayıcı: sallama sürüyorsa bir kare büyüt, bittiyse bir kare küçült.
pub fn tick(hwnd: HWND, cfg: &CursorConfig) {
    STATE.with_borrow_mut(|s| {
        let holding = s.shook.is_some_and(|t| t.elapsed() < HOLD);
        // İmleç başka role geçti (ör. metin alanının üstüne geldi): onu büyüt.
        if holding {
            let now_id = swap::shown().and_then(|(_, c)| swap::role(c));
            if let (Some(id), Some(g)) = (now_id, s.grow.as_ref()) {
                if id != g.swap.id() {
                    let step = g.step;
                    s.grow = None;
                    s.grow = Grow::begin(cfg);
                    if let Some(g) = s.grow.as_mut() {
                        g.set(step);
                    }
                }
            }
        }
        let Some(g) = s.grow.as_mut() else {
            unsafe {
                let _ = KillTimer(hwnd, TIMER);
            }
            return;
        };
        if holding {
            if g.step < g.frames.len() {
                g.set(g.step + 1);
            }
        } else if g.step > 1 {
            g.set(g.step - 1);
        } else {
            s.grow = None;
            unsafe {
                let _ = KillTimer(hwnd, TIMER);
            }
        }
    });
}

/// İmleci hemen asıl boyutuna döndürür (ayar değişti, imleç yeniden uygulanacak, kapanış).
pub fn stop() {
    STATE.with_borrow_mut(|s| {
        s.grow = None;
        s.shook = None;
        s.det.clear();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(d: &mut Detector, pts: impl IntoIterator<Item = (u64, i32, i32)>) -> bool {
        let mut hit = false;
        for (t, x, y) in pts {
            hit |= d.push(t, x, y, 1.0);
        }
        hit
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
        assert!(run(&mut Detector::default(), zigzag(6, 160, 80)));
    }

    #[test]
    fn duz_hizli_hareket_sallama_degil() {
        let v: Vec<_> = (0..60).map(|i| (i * 8, 100 + i as i32 * 25, 400 + i as i32 * 3)).collect();
        assert!(!run(&mut Detector::default(), v));
    }

    #[test]
    fn yavas_ileri_geri_sallama_degil() {
        assert!(!run(&mut Detector::default(), zigzag(6, 160, 700)));
    }

    #[test]
    fn kucuk_titreme_sallama_degil() {
        assert!(!run(&mut Detector::default(), zigzag(30, 6, 24)));
    }
}
