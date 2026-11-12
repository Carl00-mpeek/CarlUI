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

thread_local! {
    /// Konuma bağlı anahtarların içeriği: (sayfa kimliği, içeriğin sol üst köşesi).
    static PLACE: std::cell::Cell<(u64, f32, f32)> = const { std::cell::Cell::new((0, 0.0, 0.0)) };
}

/// Konuma bağlı anahtarlar (anahtar, düğme, satır vurgusu) için içeriği tanıtır:
/// `page` sayfanın kimliği, (`x`, `y`) içeriğin kaydırma ve geçişle birlikte
/// oynayan sol üst köşesi. Böylece sayfa değişince ya da kaydırınca başka bir
/// öğenin değerini devralıp "açılıp kapanma" animasyonu oynatılmaz.
pub fn place(page: u64, x: f32, y: f32) {
    PLACE.with(|c| c.set((page, x, y)));
}

/// Ekrandaki konumdan, içeriğe göre sabit anahtar üretir (bkz. `place`).
pub fn at(tag: &str, x: f32, y: f32) -> u64 {
    let (page, ox, oy) = PLACE.with(|c| c.get());
    key((tag, page, (x - ox).round() as i32, (y - oy).round() as i32))
}

/// Değeri hedefe yaklaştırır; `speed` büyüdükçe hızlı (12 ≈ 0,25 sn).
/// Animasyon hızı ayarı hızı çarpar; hareket azaltılmışsa değer hemen hedefe atlar.
pub fn to(k: u64, target: f32, speed: f32) -> f32 {
    S.with(|s| {
        let mut s = s.borrow_mut();
        if !s.on {
            return target;
        }
        let (dt, frame) = (s.dt, s.frame);
        let Some(speed) = rate(speed, carlui_core::theme::motion_scale()) else {
            s.values.insert(k, (target, frame));
            return target;
        };
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

/// Yaklaşma hızı süre çarpanına bölünür (çarpan 0,5 → iki kat hızlı);
/// çarpan 0 ise (hareket azaltılmış) `None`: değer anında hedefte.
fn rate(speed: f32, scale: f32) -> Option<f32> {
    (scale > 0.0).then(|| speed / scale)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_follows_motion_scale() {
        assert_eq!(rate(12.0, 1.0), Some(12.0));
        assert_eq!(rate(12.0, 0.5), Some(24.0));
        assert_eq!(rate(12.0, 0.0), None);
    }

    #[test]
    fn reduced_motion_jumps_to_target() {
        enable();
        frame();
        assert_eq!(to(1, 0.0, 12.0), 0.0);
        frame();
        // Normal hızda bir karede hedefe varılmaz.
        assert!(to(1, 1.0, 12.0) < 1.0);
        carlui_core::theme::set_motion(0.0);
        frame();
        assert_eq!(to(1, 1.0, 12.0), 1.0);
        assert_eq!(to(1, 0.0, 12.0), 0.0);
        carlui_core::theme::set_motion(1.0);
    }
}
