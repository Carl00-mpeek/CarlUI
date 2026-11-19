//! Kar yağarken fare bir süre kıpırdamazsa imleç buzla kaplanır: imlecin
//! boyutu ve şekli aynı kalır, gövdesi buz mavisine döner, kenarı kırağılaşır,
//! içinde kristal damarları ve parıltılar belirir. Fare oynayınca (ya da
//! tıklanınca) hemen erir.
//!
//! Pencere ya da kare kare çizim yoktur: donarken imlecin görselinden buzun
//! üç aşaması bir kez hazırlanır ve `SetSystemCursor` ile imlecin yerine
//! konur; çizimi Windows yapar. Buz yokken imleç canlı dock'un tikiyle (en
//! sık `CHECK`) yoklanır. Donukken ayrı bir iş parçacığı zamanlayıcısı
//! (`WATCH`) imleci `POLL` aralığıyla izler: kıpırdayınca imleci geri koyar,
//! beklerken buzu bir sonraki aşamaya geçirir.

use carlui_core::cursor::swap::{self, Pixels, Swap};
use std::{
    cell::RefCell,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::*,
    UI::{
        Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON},
        WindowsAndMessaging::*,
    },
};

/// İmleç bu kadar kıpırdamazsa buz gelmeye başlar.
const IDLE: Duration = Duration::from_secs(15);
/// Buz yokken imleç bu sıklıkla yoklanır.
const CHECK: Duration = Duration::from_millis(400);
/// Donukken imleç bu sıklıkla yoklanır: fare oynayınca buz hemen erisin.
const POLL_MS: u32 = 50;
/// Buzun aşamaları (kaplanma oranı) ve aralarındaki süre.
const STAGES: [f32; 3] = [0.4, 0.7, 1.0];
const STAGE: Duration = Duration::from_millis(900);

/// Fare düğmesi basılı mı?
fn pressed() -> bool {
    [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON].iter().any(|k| unsafe { GetAsyncKeyState(k.0 as i32) } < 0)
}

/// Çökme olursa imleç buzlu kalmasın: dock yeniden açılınca bu işarete bakılır.
fn marker() -> std::path::PathBuf {
    carlui_core::paths::data_dir().join("frost-cursor")
}

/// Her pikselin şekle uzaklığı (piksel; 3-4 pah dönüşümü, öklide yakın).
fn distance(on: &[bool], w: usize, h: usize) -> Vec<f32> {
    const INF: u32 = u32::MAX / 4;
    let mut d: Vec<u32> = on.iter().map(|&o| if o { 0 } else { INF }).collect();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let mut v = d[i];
            if x > 0 {
                v = v.min(d[i - 1] + 3);
            }
            if y > 0 {
                v = v.min(d[i - w] + 3);
                if x > 0 {
                    v = v.min(d[i - w - 1] + 4);
                }
                if x + 1 < w {
                    v = v.min(d[i - w + 1] + 4);
                }
            }
            d[i] = v;
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            let mut v = d[i];
            if x + 1 < w {
                v = v.min(d[i + 1] + 3);
            }
            if y + 1 < h {
                v = v.min(d[i + w] + 3);
                if x + 1 < w {
                    v = v.min(d[i + w + 1] + 4);
                }
                if x > 0 {
                    v = v.min(d[i + w - 1] + 4);
                }
            }
            d[i] = v;
        }
    }
    d.into_iter().map(|v| v as f32 / 3.0).collect()
}

/// Konuma bağlı sabit gürültü (0..1).
fn hash(x: usize, y: usize, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77) ^ seed.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0xFFFF) as f32 / 65535.0
}

/// Kristal damarları: altıgen buzun üç yönünde ince, kesik çizgiler.
fn vein(x: f32, y: f32, unit: f32) -> f32 {
    let mut v: f32 = 0.0;
    for (i, a) in [0.0f32, 1.047, 2.094].into_iter().enumerate() {
        let t = (x * a.cos() + y * a.sin()) / (5.5 * unit) + i as f32 * 0.37;
        let r = (t - t.round()).abs();
        // Çizginin kesikli olması: çizgi boyunca ikinci bir dalga.
        let along = (-x * a.sin() + y * a.cos()) / (4.0 * unit) + i as f32;
        if (along - along.floor()) < 0.6 {
            v = v.max((1.0 - r / 0.12).clamp(0.0, 1.0));
        }
    }
    v
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// İmleci `k` oranında buzla kaplar (0: hiç, 1: tamamen). Boyut, şekil ve
/// sıcak nokta aynı kalır; saydam piksellere dokunulmaz. Buz kenardan
/// içeri doğru ilerler.
fn coat(p: &Pixels, k: f32, seed: u32) -> Pixels {
    let (w, h) = (p.w, p.h);
    let off: Vec<bool> = p.on().iter().map(|o| !o).collect();
    // Görünen her pikselin kenara (saydama) uzaklığı.
    let d = distance(&off, w, h);
    let deep = d.iter().zip(&off).filter(|(_, o)| !**o).map(|(v, _)| *v).fold(1.0f32, f32::max);
    let unit = (w.max(h) as f32 / 32.0).max(0.75);
    let front = k * (deep + 1.0);
    let mut out = p.clone();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if off[i] {
                continue;
            }
            // Kenar (d≈1) önce donar; ön içeri ilerledikçe gövde kaplanır.
            let f = ((front - (d[i] - 1.0)) / 1.5).clamp(0.0, 1.0);
            if f <= 0.0 {
                continue;
            }
            let n = hash(x, y, seed);
            let (r, g, b) = if d[i] <= 1.2 * unit {
                // Dış hat: koyu buz mavisi, açık zeminde de seçilsin.
                (96.0, 150.0, 205.0)
            } else if d[i] <= 2.2 * unit {
                // Kırağı: kenarın hemen içi, en beyaz yer.
                (236.0, 246.0, 255.0)
            } else {
                // Gövde: buz mavisi, damarlar ve kırık beyaz lekeler.
                let v = vein(x as f32, y as f32, unit);
                let base = mix(196.0, 222.0, n);
                (mix(base - 18.0, 248.0, v), mix(base + 12.0, 252.0, v), mix(240.0, 255.0, v))
            };
            // Parıltı: tam donunca ara ara saf beyaz noktalar.
            let (r, g, b) = if k >= 1.0 && n > 0.94 && d[i] > 1.2 * unit { (255.0, 255.0, 255.0) } else { (r, g, b) };
            let t = f * 0.92;
            let q = &mut out.px[i * 4..i * 4 + 4];
            q[0] = mix(q[0] as f32, b, t).round() as u8;
            q[1] = mix(q[1] as f32, g, t).round() as u8;
            q[2] = mix(q[2] as f32, r, t).round() as u8;
        }
    }
    out
}

/// Donmuş imleç: değiştirilen rol, aşamaların imleçleri ve izleme zamanlayıcısı.
struct Frozen {
    timer: usize,
    at: (i32, i32),
    /// Donduğu andaki imleç tutamağı (rolün paylaşılan tutamağı).
    cursor: HCURSOR,
    swap: Option<Swap>,
    stages: Vec<HCURSOR>,
    stage: usize,
    since: Instant,
}

impl Drop for Frozen {
    fn drop(&mut self) {
        unsafe {
            let _ = KillTimer(None, self.timer);
        }
        if let Some(s) = self.swap.take() {
            s.end();
        }
        for c in self.stages.drain(..) {
            unsafe {
                let _ = DestroyCursor(c);
            }
        }
        let _ = std::fs::remove_file(marker());
    }
}

thread_local! {
    static FROZEN: RefCell<Option<Frozen>> = const { RefCell::new(None) };
}

fn thaw() {
    // Önce dışarı al: `Drop` çalışırken hücre ödünç alınmış olmasın.
    let f = FROZEN.with(|c| c.borrow_mut().take());
    drop(f);
}

fn frozen() -> bool {
    FROZEN.with(|c| c.borrow().is_some())
}

extern "system" fn watch_proc(_: HWND, _: u32, _: usize, _: u32) {
    let now = swap::shown();
    let melt = FROZEN.with(|c| {
        let mut c = c.borrow_mut();
        let Some(f) = c.as_mut() else { return false };
        match now {
            Some((p, h)) if (p.x, p.y) == f.at && h == f.cursor && !pressed() => {
                if f.stage + 1 < f.stages.len() && f.since.elapsed() >= STAGE {
                    f.stage += 1;
                    f.since = Instant::now();
                    if let Some(s) = f.swap.as_ref() {
                        s.show(f.stages[f.stage]);
                    }
                }
                false
            }
            _ => true,
        }
    });
    if melt {
        thaw();
    }
}

pub struct Frost {
    checked: Option<Instant>,
    last: Option<(i32, i32)>,
    still: Instant,
    seed: u32,
}

impl Frost {
    pub fn new() -> Self {
        // Önceki dock buzlu imleçle çöktüyse kullanıcının imlecini geri yükle
        // (CarlUI imleci açıksa ana süreç onu yeniden uygular).
        if marker().exists() {
            carlui_core::cursor::restore();
            let _ = std::fs::remove_file(marker());
        }
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(7);
        Self { checked: None, last: None, still: Instant::now(), seed }
    }

    pub fn step(&mut self, now: Instant) {
        if frozen() || self.checked.is_some_and(|t| now.duration_since(t) < CHECK) {
            return;
        }
        self.checked = Some(now);
        let shown = swap::shown();
        let pos = shown.map(|(p, _)| (p.x, p.y));
        if pos != self.last || pressed() {
            self.last = pos;
            self.still = now;
            return;
        }
        if now.duration_since(self.still) < IDLE {
            return;
        }
        let Some((p, c)) = shown else { return };
        if !self.freeze(p, c) {
            // Bu imleç donmuyor (uygulamanın kendi imleci, okunamadı): fare
            // kıpırdayana kadar yeniden deneme.
            self.still = now + Duration::from_secs(3600);
        }
    }

    fn freeze(&mut self, p: POINT, c: HCURSOR) -> bool {
        let Some(id) = swap::role(c) else { return false };
        let Some(px) = swap::pixels(c) else { return false };
        self.seed = self.seed.wrapping_add(1);
        let stages: Vec<HCURSOR> = STAGES.iter().filter_map(|&k| swap::make(&coat(&px, k, self.seed))).collect();
        let destroy = |v: Vec<HCURSOR>| {
            for h in v {
                unsafe {
                    let _ = DestroyCursor(h);
                }
            }
        };
        if stages.len() != STAGES.len() {
            destroy(stages);
            return false;
        }
        let Some(swap) = Swap::begin(id) else {
            destroy(stages);
            return false;
        };
        let _ = std::fs::write(marker(), b"");
        swap.show(stages[0]);
        let timer = unsafe { SetTimer(None, 0, POLL_MS, Some(watch_proc)) };
        let f = Frozen { timer, at: (p.x, p.y), cursor: c, swap: Some(swap), stages, stage: 0, since: Instant::now() };
        if timer == 0 {
            drop(f);
            return false;
        }
        FROZEN.with(|cell| *cell.borrow_mut() = Some(f));
        true
    }
}

impl Drop for Frost {
    fn drop(&mut self) {
        thaw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uzaklik_sekle_gore_artar() {
        let (w, h) = (9, 9);
        let mut on = vec![false; w * h];
        on[4 * w + 4] = true;
        let d = distance(&on, w, h);
        assert_eq!(d[4 * w + 4], 0.0);
        assert!((d[4 * w + 7] - 3.0).abs() < 1e-4);
        // Köşegen: 3·4/3 = 4 (gerçek 4,24).
        assert!((d[7 * w + 7] - 4.0).abs() < 1e-4);
        assert!(d[0] > d[2 * w + 2]);
    }

    /// 32×32 tuvalin ortasında 20×20 koyu kare.
    fn square() -> Pixels {
        let (w, h) = (32, 32);
        let mut px = vec![0u8; w * h * 4];
        for y in 6..26 {
            for x in 6..26 {
                px[(y * w + x) * 4..][..4].copy_from_slice(&[30, 28, 28, 255]);
            }
        }
        Pixels { w, h, hot: (6, 6), px }
    }

    #[test]
    fn buz_boyutu_ve_sekli_degistirmez() {
        let p = square();
        let c = coat(&p, 1.0, 3);
        assert_eq!((c.w, c.h, c.hot), (p.w, p.h, p.hot));
        for (a, b) in p.px.chunks_exact(4).zip(c.px.chunks_exact(4)) {
            assert_eq!(a[3], b[3]);
            if a[3] == 0 {
                assert_eq!(a, b);
            }
        }
        // Gövde koyudan buz rengine döner.
        let mid = &c.px[(16 * 32 + 16) * 4..][..4];
        assert!(mid[0] > 150 && mid[2] > 120, "{mid:?}");
    }

    #[test]
    fn buz_kenardan_iceri_ilerler() {
        let p = square();
        let light = coat(&p, 0.4, 3);
        let edge = &light.px[(6 * 32 + 16) * 4..][..4];
        let mid = &light.px[(16 * 32 + 16) * 4..][..4];
        assert_ne!(edge[0], 30);
        assert_eq!(mid[0], 30);
    }
}
