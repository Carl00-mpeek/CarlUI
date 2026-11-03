//! Bu modül anlık modlu (her karede baştan çizilen) pencerelerde "geçişler
//! nasıl yumuşak olur?" sorununu şu yöntemle çözüyor: her animasyonlu değer
//! bir anahtarla (ör. anahtarın konumu) saklanır; çizim sırasında hedefe doğru
//! üstel olarak yaklaşır. Hedefe varmamış bir değer varsa `busy()` true döner
//! ve pencere bir sonraki kareyi ister (~60 fps), hepsi oturunca durur; boşta
//! hiç çizim yapılmaz. İlk kez görülen anahtar doğrudan hedefte başlar
//! (kaydırınca her şey yeniden "canlanmasın").
//!
//! Animasyon yalnızca `enable()` çağıran pencerede çalışır; diğerlerinde
//! değerler hemen hedefe atlar.

use std::{
    cell::RefCell,
    collections::HashMap,
    hash::{Hash, Hasher},
    time::Instant,
};

#[derive(Default)]
struct State {
    on: bool,
    values: HashMap<u64, (f32, u64)>,
    last: Option<Instant>,
    dt: f32,
    frame: u64,
    busy: bool,
}

thread_local! {
    static S: RefCell<State> = RefCell::new(State::default());
}

pub fn enable() {
    S.with(|s| s.borrow_mut().on = true);
}

/// Karenin başında çağrılır: geçen süre hesaplanır, eskimiş anahtarlar atılır.
pub fn frame() {
    S.with(|s| {
        let mut s = s.borrow_mut();
        let now = Instant::now();
        s.dt = s.last.map(|t| (now - t).as_secs_f32()).unwrap_or(0.016).clamp(0.0, 0.05);
        s.last = Some(now);
        s.frame += 1;
        s.busy = false;
        let f = s.frame;
        if s.values.len() > 2000 {
            s.values.retain(|_, v| f - v.1 < 120);
        }
    });
}

/// Bir sonraki kare gerekiyor mu?
pub fn busy() -> bool {
    S.with(|s| s.borrow().busy)
}

pub fn key(parts: impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    parts.hash(&mut h);
    h.finish()
}

/// Değeri hedefe yaklaştırır; `speed` büyüdükçe hızlı (12 ≈ 0,25 sn).
pub fn to(k: u64, target: f32, speed: f32) -> f32 {
    S.with(|s| {
        let mut s = s.borrow_mut();
        if !s.on {
            return target;
        }
        let (dt, frame) = (s.dt, s.frame);
        let e = s.values.entry(k).or_insert((target, frame));
        let mut v = e.0 + (target - e.0) * (1.0 - (-speed * dt).exp());
        if (target - v).abs() < 0.004 * target.abs().max(1.0) {
            v = target;
        }
        *e = (v, frame);
        let moving = v != target;
        if moving {
            s.busy = true;
        }
        v
    })
}

/// Değeri zorla ayarlar (ör. sayfa değişince içerik aşağıdan başlasın).
pub fn set(k: u64, v: f32) {
    S.with(|s| {
        let mut s = s.borrow_mut();
        let f = s.frame;
        s.values.insert(k, (v, f));
        s.busy = true;
    });
}
