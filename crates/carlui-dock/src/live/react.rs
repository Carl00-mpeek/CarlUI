//! Hayvan tepkileri: bilgisayarda olanlara karşılık veren saf mantık.
//!
//! - Müzik: çalarken arada bir dans eder (oturma şeridi döner, gövde ritimle
//!   zıplar ve hafifçe yana yatar), dans sırasında nota parçacıkları çıkar.
//! - Bildirim: irkilir, sonra başını kaldırıp bir süre yukarı (adaya) bakar.
//! - Boşta kalma: birkaç dakika hiç girdi yoksa uyur ve girdi gelene dek
//!   uyanmaz; girdi gelince gerinir.
//! - Okşama: tıklanınca sevinir (zıplar ya da yalanır), kalpler çıkar.
//!
//! Burada saat ve girdi dışarıdan verilir (testlerde belirlenimci). Ayrı
//! zamanlayıcı yoktur: hepsi canlı dock'un tek tikinde, yalnızca çalışırken
//! ilerler. Öncelik: okşama > bildirim > boşta uyku/uyanma > müzik dansı.

use super::{
    pack::Anim,
    pet::{Mode, Pet},
    rng::Rng,
};

/// Bu kadar süre girdi yoksa hayvan uyur (ms).
pub const IDLE_SLEEP_MS: u32 = 180_000;
/// Boşta denetimi en sık bu aralıkla yapılır (ms).
pub const IDLE_CHECK_MS: f32 = 1_000.0;
/// Tutulan uykunun yenilenme süresi: uyurken tik bundan seyrek olmaz,
/// ama boşta denetimi yine de saniyede bir istenir (bkz. `React::cap`).
pub const HELD_MS: f32 = 60_000.0;
/// Bir dans ne kadar sürer ve bir vuruş kaç ms.
pub const DANCE_MS: f32 = 4_800.0;
pub const BEAT_MS: f32 = 600.0;
/// Dans salınımı en çok bu sıklıkla yeniden çizilir (~10 fps).
pub const SWAY_STEP_MS: f32 = 100.0;
/// Bildirimden sonra yukarı bakma süresi.
const LOOK_UP_MS: f32 = 3_000.0;
/// Müzik çalarken iki dans arası (ms).
const DANCE_GAP: (f32, f32) = (25_000.0, 50_000.0);
/// Dans sırasında iki nota arası (ms).
const NOTE_GAP: (f32, f32) = (700.0, 1_300.0);
/// Okşanınca çıkan kalp sayısı.
pub const HEARTS: u32 = 3;

/// Boşta denetiminin kararı.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdleAct {
    Doze,
    Wake,
}

/// Saf karar: `idle_ms` son girdiden beri geçen süre; `held` hayvan şu an
/// boşta uykusunda mı; `calm` başka bir şeyle meşgul değil mi.
pub fn idle_act(idle_ms: u32, held: bool, calm: bool) -> Option<IdleAct> {
    let away = idle_ms >= IDLE_SLEEP_MS;
    match (held, away) {
        (true, false) => Some(IdleAct::Wake),
        (false, true) if calm => Some(IdleAct::Doze),
        _ => None,
    }
}

/// Dans salınımı: (yukarı kalkış, sanat pikseli; yana yatış, derece). Her
/// vuruşta bir kez zıplar, iki vuruşta bir sağa-sola yatar. Piksel sanatına
/// uysun diye tam sayıya yuvarlanır.
pub fn sway(elapsed_ms: f32) -> (i32, i32) {
    let b = (elapsed_ms.max(0.0) / BEAT_MS).fract();
    let lift = ((b * std::f32::consts::PI).sin() * 2.0).round() as i32;
    let side = ((elapsed_ms / (BEAT_MS * 2.0)).fract() * std::f32::consts::TAU).sin();
    (lift, (side * 6.0).round() as i32)
}

impl Pet {
    /// Kendi işinde değil: oturuyor, kıpırdıyor, güneşleniyor ya da kendi
    /// kendine uyuyor (tutulan uyku sayılmaz).
    pub fn calm(&self) -> bool {
        matches!(self.mode, Mode::Idle | Mode::Fidget | Mode::Bask) || (self.mode == Mode::Sleep && !self.held)
    }

    pub fn dancing(&self) -> bool {
        self.mode == Mode::Dance
    }

    pub fn held(&self) -> bool {
        self.held
    }

    /// Ekrana uygulanacak salınım (dans değilse sıfır).
    pub fn sway_now(&self) -> (i32, i32) {
        if self.dancing() {
            self.sway_key
        } else {
            (0, 0)
        }
    }

    /// `update` içinden: salınım tam sayı olarak değiştiyse çizim iste.
    pub(super) fn sway_tick(&mut self) {
        let k = sway(DANCE_MS - self.mode_left);
        if k != self.sway_key {
            self.sway_key = k;
            self.dirty = true;
        }
    }

    pub(super) fn look_up(&mut self) {
        self.look_after = false;
        self.enter(Mode::Look);
        self.mode_left = LOOK_UP_MS;
    }

    fn dance(&mut self) {
        self.enter(Mode::Dance);
    }

    /// Bildirim: irkilir, sonra yukarı bakar. Bakış şeridi yoksa yalnızca irkilir.
    fn notice(&mut self) {
        self.held = false;
        self.look_after = self.own(Anim::Look);
        self.enter(Mode::Alert);
    }

    fn doze(&mut self) {
        self.held = true;
        if self.mode != Mode::Sleep {
            self.enter(Mode::Sleep);
        }
        self.mode_left = HELD_MS;
    }

    /// Girdi geldi: gerinerek uyanır (şeridi yoksa irkilerek).
    fn wake(&mut self) {
        self.held = false;
        if self.mode == Mode::Sleep {
            self.enter(if self.own(Anim::Stretch) { Mode::Stretch } else { Mode::Alert });
        }
    }

    /// Okşandı: zıplar ya da yalanır (paketin hangisi varsa), yoksa irkilir.
    fn petted(&mut self) {
        self.held = false;
        let mut opts = [Mode::Alert; 2];
        let mut n = 0;
        for (a, m) in [(Anim::Hop, Mode::Hop), (Anim::Lick, Mode::Lick)] {
            if self.own(a) {
                opts[n] = m;
                n += 1;
            }
        }
        let m = if n == 0 { Mode::Alert } else { opts[(self.rng.f() * n as f32) as usize % n] };
        self.enter(m);
    }
}

/// Bir adımın sonucu: kaç nota çıkacak ve tike kadar en çok kaç ms beklenmeli.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Out {
    pub notes: u32,
    pub cap_ms: Option<f32>,
}

pub struct React {
    music: bool,
    dance_in: f32,
    note_in: f32,
    since_check: f32,
    rng: Rng,
}

impl React {
    pub fn new(rng: Rng) -> Self {
        let mut r = Self { music: false, dance_in: 0.0, note_in: 0.0, since_check: IDLE_CHECK_MS, rng };
        r.dance_in = r.rng.range(DANCE_GAP.0, DANCE_GAP.1) / 3.0;
        r
    }

    /// Medya çalmaya başladı / durdu. İlk dans kısa süre içinde gelir.
    pub fn music(&mut self, on: bool) {
        if on && !self.music {
            self.dance_in = self.rng.range(3_000.0, 8_000.0);
        }
        self.music = on;
    }

    pub fn note(&mut self, pet: &mut Pet) {
        pet.notice();
    }

    /// Okşandı; çıkacak kalp sayısı.
    pub fn click(&mut self, pet: &mut Pet) -> u32 {
        pet.petted();
        HEARTS
    }

    /// `dt` ms ilerletir. `idle` son girdiden beri geçen ms'yi verir; en çok
    /// saniyede bir çağrılır.
    pub fn step(&mut self, pet: &mut Pet, dt: f32, idle: impl FnOnce() -> Option<u32>) -> Out {
        let dt = if dt.is_finite() { dt.clamp(0.0, 600_000.0) } else { 0.0 };
        let mut out = Out::default();
        self.since_check += dt;
        if self.since_check >= IDLE_CHECK_MS {
            self.since_check = 0.0;
            // Müzik/video çalarken girdi olmasa da kullanıcı oradadır (izliyor,
            // dinliyor): boşta uykusu yok; tutulan uyku da müzikle biter.
            let idle = if self.music { Some(0) } else { idle() };
            if let Some(ms) = idle {
                match idle_act(ms, pet.held(), pet.calm()) {
                    Some(IdleAct::Doze) => pet.doze(),
                    Some(IdleAct::Wake) => pet.wake(),
                    None => {}
                }
            }
        }
        if pet.held() {
            // Uyurken yalnızca girdi dönüşü beklenir: saniyede bir bakılır.
            out.cap_ms = Some(IDLE_CHECK_MS - self.since_check);
            return out;
        }
        if self.music {
            self.dance_in -= dt;
            if self.dance_in <= 0.0 {
                if matches!(pet.mode, Mode::Idle | Mode::Fidget | Mode::Bask) {
                    pet.dance();
                    self.note_in = 0.0;
                    self.dance_in = self.rng.range(DANCE_GAP.0, DANCE_GAP.1);
                } else if pet.mode == Mode::Sleep {
                    // Kendi kendine uyuyorsa (boşta uykusu değil; o yukarıda
                    // döndü) müzik onu uyandırır: gerinir, sonra dans eder.
                    pet.wake();
                    self.dance_in = 2_500.0;
                } else {
                    // Meşgul: biraz sonra yeniden dene.
                    self.dance_in = 2_000.0;
                }
            }
            if !pet.dancing() {
                out.cap_ms = Some(self.dance_in.max(1.0));
            }
        }
        if pet.dancing() {
            self.note_in -= dt;
            if self.note_in <= 0.0 {
                out.notes = 1;
                self.note_in = self.rng.range(NOTE_GAP.0, NOTE_GAP.1);
            }
            out.cap_ms = Some(self.note_in.max(1.0));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::pet::Strip;

    fn pet(own_all: bool) -> Pet {
        let s = |frames, fps| Strip { frames, fps, own: own_all, reverse_out: false };
        let sit = Strip { frames: 4, fps: 8.0, own: true, reverse_out: false };
        Pet::new([sit, s(6, 10.0), s(1, 0.0), s(4, 6.0), s(4, 9.0), s(2, 6.0), s(6, 7.7), s(4, 2.2), s(6, 14.0)], 30.0, 200.0, Rng::new(3))
    }

    #[test]
    fn bosta_esigi() {
        assert_eq!(idle_act(0, false, true), None);
        assert_eq!(idle_act(IDLE_SLEEP_MS - 1, false, true), None);
        assert_eq!(idle_act(IDLE_SLEEP_MS, false, true), Some(IdleAct::Doze));
        // Meşgulken (ör. irkilirken) uyumaz.
        assert_eq!(idle_act(IDLE_SLEEP_MS, false, false), None);
        // Uyurken girdi geldi: uyanır; hâlâ uzaktaysa uyumaya devam.
        assert_eq!(idle_act(500, true, false), Some(IdleAct::Wake));
        assert_eq!(idle_act(IDLE_SLEEP_MS + 5, true, false), None);
    }

    #[test]
    fn uzaktayken_uyur_girdiyle_gerinir() {
        let mut p = pet(true);
        p.enter(Mode::Idle);
        let mut r = React::new(Rng::new(1));
        r.step(&mut p, 1_000.0, || Some(IDLE_SLEEP_MS + 10));
        assert!(p.held() && p.anim() == Anim::Sleep);
        // Uzun süre geçse de uyku bitmez.
        for _ in 0..300 {
            let o = r.step(&mut p, 1_000.0, || Some(IDLE_SLEEP_MS + 10));
            p.update(1_000.0, 0.0, 400.0);
            assert!(o.cap_ms.unwrap() <= IDLE_CHECK_MS);
        }
        assert_eq!(p.anim(), Anim::Sleep);
        r.step(&mut p, 1_000.0, || Some(20));
        assert!(!p.held());
        assert_eq!(p.anim(), Anim::Stretch);
    }

    #[test]
    fn denetim_saniyede_en_cok_bir() {
        let mut p = pet(true);
        let mut r = React::new(Rng::new(2));
        let mut calls = 0;
        for _ in 0..100 {
            r.step(&mut p, 33.0, || {
                calls += 1;
                Some(0)
            });
        }
        // 3.3 sn: ilk adım + en çok 3 denetim.
        assert!(calls <= 4, "{calls}");
    }

    #[test]
    fn muzikte_dans_eder_ve_nota_cikar() {
        let mut p = pet(true);
        p.enter(Mode::Idle);
        let mut r = React::new(Rng::new(4));
        r.music(true);
        let (mut danced, mut notes) = (false, 0);
        for _ in 0..400 {
            let o = r.step(&mut p, 100.0, || Some(0));
            notes += o.notes;
            danced |= p.dancing();
            if p.dancing() {
                assert!(o.cap_ms.unwrap() <= NOTE_GAP.1);
            } else if p.anim() != Anim::Sit {
                p.enter(Mode::Idle);
            }
            if p.dancing() {
                p.update(100.0, 0.0, 400.0);
            }
        }
        assert!(danced && notes > 0);
        // Müzik yokken dans yok.
        let mut q = pet(true);
        q.enter(Mode::Idle);
        let mut r = React::new(Rng::new(4));
        for _ in 0..2_000 {
            r.step(&mut q, 100.0, || Some(0));
            assert!(!q.dancing());
        }
    }

    #[test]
    fn muzik_uyutmaz_ve_uyandirir() {
        let mut p = pet(true);
        p.enter(Mode::Sleep);
        let mut r = React::new(Rng::new(5));
        r.music(true);
        for _ in 0..100 {
            r.step(&mut p, 100.0, || Some(0));
        }
        assert_eq!(p.anim(), Anim::Stretch, "kendi uykusu: müzik uyandırır");
        // Müzik çalarken girdi olmasa da boşta uykusuna geçmez (video izleniyor).
        let mut q = pet(true);
        q.enter(Mode::Idle);
        let mut r = React::new(Rng::new(5));
        r.music(true);
        for _ in 0..300 {
            r.step(&mut q, 100.0, || Some(IDLE_SLEEP_MS + 1));
            q.update(100.0, 0.0, 400.0);
            assert!(!q.held());
        }
        // Boşta uykusundayken müzik başlarsa uyanır.
        let mut s = pet(true);
        s.enter(Mode::Idle);
        let mut r = React::new(Rng::new(6));
        r.step(&mut s, 1_000.0, || Some(IDLE_SLEEP_MS + 1));
        assert!(s.held());
        r.music(true);
        r.step(&mut s, 1_000.0, || Some(IDLE_SLEEP_MS + 1));
        assert!(!s.held() && s.anim() == Anim::Stretch);
    }

    #[test]
    fn bildirimde_irkilir_sonra_yukari_bakar() {
        let mut p = pet(true);
        let mut r = React::new(Rng::new(5));
        p.enter(Mode::Sleep);
        p.held = true;
        r.note(&mut p);
        assert_eq!(p.anim(), Anim::Alert);
        assert!(!p.held());
        // İrkilme en az 1.2 sn sürer.
        p.update(1_300.0, 0.0, 400.0);
        assert_eq!(p.anim(), Anim::Look);
        // Bakış şeridi olmayan pakette yalnızca irkilir.
        let mut q = pet(false);
        r.note(&mut q);
        q.update(1_300.0, 0.0, 400.0);
        assert_ne!(q.anim(), Anim::Look);
    }

    #[test]
    fn okşaninca_sevinir() {
        let mut p = pet(true);
        let mut r = React::new(Rng::new(6));
        assert_eq!(r.click(&mut p), HEARTS);
        assert!(matches!(p.anim(), Anim::Hop | Anim::Lick));
        let mut q = pet(false);
        r.click(&mut q);
        assert_eq!(q.anim(), Anim::Alert);
    }

    #[test]
    fn salinim_sinirli_ve_ritimli() {
        let mut seen_up = false;
        for i in 0..200 {
            let (lift, tilt) = sway(i as f32 * 37.0);
            assert!((0..=2).contains(&lift));
            assert!((-6..=6).contains(&tilt));
            seen_up |= lift == 2;
        }
        assert!(seen_up);
        assert_eq!(sway(0.0).0, 0);
    }
}
