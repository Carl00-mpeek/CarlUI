//! Evcil hayvanın davranışı: saf durum makinesi. Saat dışarıdan (`update`
//! ile geçen ms) ve rastgelelik tohumdan gelir; testlerde belirlenimcidir.
//!
//! Durumlar: oturma (ara sıra göz kırpar/kuyruk sallar), yürüme, uyuma,
//! gerinme, imlece bakma, uyanma (uygulama açılınca) ve paketin varsa ek
//! hareketleri (yalanma, güneşlenme, zıplama). Dururken kare değişmez: bir
//! sonraki olaya kadar tek, uzun bir bekleme istenir. Uyku şeridi birden çok
//! kareyse yavaşça döner (nefes).

use super::{
    pack::{Anim, N_ANIMS},
    rng::Rng,
};

/// Bir durumun şerit bilgisi (pakettekinin aynısı).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strip {
    pub frames: u32,
    pub fps: f32,
    /// Paket bu durumu kendi şeridiyle veriyor mu (yoksa oturuşa düşmüştür;
    /// ek hareketler yalnızca kendi şeridi varsa seçilir).
    pub own: bool,
    /// Bakış bitince şerit geriye oynatılır.
    pub reverse_out: bool,
}

impl Strip {
    fn animates(&self) -> bool {
        self.frames > 1 && self.fps > 0.0
    }

    fn frame_ms(&self) -> f32 {
        1000.0 / self.fps.max(0.1)
    }

    /// Bir kez oynatma süresi (ms); oynamayan şeritte `still` ms.
    fn once_ms(&self, still: f32) -> f32 {
        if self.animates() { self.frames as f32 * self.frame_ms() } else { still }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    /// Durgun oturuş; süre dolunca yeni bir şey seçilir.
    Idle,
    /// Oturma şeridini bir kez oynatır (göz kırpma, kuyruk).
    Fidget,
    Walk { to: f32 },
    Sleep,
    Stretch,
    /// Bakış şeridini bir kez oynatır ve son karede (başı kalkık) bekler.
    Look,
    /// Bakış bitti: şerit geriye oynar (kedi geri oturur).
    Out,
    Alert,
    Lick,
    Bask,
    /// Zıplama: 1-3. karelerde havada, ileri gider.
    Hop,
}

/// İmleç bu kadar yakınsa (piksel, ölçekli) evcil hayvan ona bakar.
pub const LOOK_RADIUS: f32 = 200.0;

/// Zıplarken havadaki kareler ve yürüyüşe göre hız katı.
const HOP_AIR: std::ops::RangeInclusive<u32> = 1..=3;
const HOP_SPEED: f32 = 3.0;

pub struct Pet {
    pub x: f32,
    /// Sağa mı bakıyor (sanat sağa bakar; sola bakınca aynalanır).
    pub right: bool,
    pub frame: u32,
    mode: Mode,
    strips: [Strip; N_ANIMS],
    /// Yürüme hızı, piksel/saniye.
    speed: f32,
    mode_left: f32,
    frame_left: f32,
    /// Son imleç/dürtmeden beri geçen süre: uzun sessizlikte uyuma olasılığı artar.
    quiet: f32,
    rng: Rng,
    dirty: bool,
}

impl Pet {
    pub fn new(strips: [Strip; N_ANIMS], speed: f32, x: f32, rng: Rng) -> Self {
        let mut p = Self { x, right: true, frame: 0, mode: Mode::Idle, strips, speed: speed.max(1.0), mode_left: 0.0, frame_left: 0.0, quiet: 0.0, rng, dirty: true };
        p.enter(Mode::Idle);
        p
    }

    /// Ekrandaki animasyon durumu.
    pub fn anim(&self) -> Anim {
        match self.mode {
            Mode::Idle | Mode::Fidget => Anim::Sit,
            Mode::Walk { .. } => Anim::Walk,
            Mode::Sleep => Anim::Sleep,
            Mode::Stretch => Anim::Stretch,
            Mode::Look | Mode::Out => Anim::Look,
            Mode::Alert => Anim::Alert,
            Mode::Lick => Anim::Lick,
            Mode::Bask => Anim::Bask,
            Mode::Hop => Anim::Hop,
        }
    }

    #[cfg(test)]
    pub fn sleeping(&self) -> bool {
        self.mode == Mode::Sleep
    }

    fn strip(&self) -> Strip {
        self.strips[self.anim() as usize]
    }

    /// Bu modda kareler ilerliyor mu? Dururken hayır; bakarken yalnızca baş
    /// kalkana (ya da inene) kadar.
    fn animating(&self) -> bool {
        let s = self.strip();
        if !s.animates() {
            return false;
        }
        match self.mode {
            Mode::Idle => false,
            Mode::Look => self.frame + 1 < s.frames,
            Mode::Out => self.frame > 0,
            _ => true,
        }
    }

    /// Döngüde oynayan modlar; ötekiler bir kez oynar ve son karede kalır.
    fn looping(&self) -> bool {
        matches!(self.mode, Mode::Walk { .. } | Mode::Sleep | Mode::Bask)
    }

    fn enter(&mut self, m: Mode) {
        self.mode = m;
        self.frame = 0;
        self.dirty = true;
        let s = self.strip();
        self.frame_left = s.frame_ms();
        self.mode_left = match m {
            Mode::Idle => self.rng.range(4_000.0, 9_000.0),
            Mode::Fidget | Mode::Lick | Mode::Hop => s.once_ms(0.0),
            Mode::Walk { .. } => 60_000.0, // güvenlik: takılırsa otur
            Mode::Sleep => self.rng.range(40_000.0, 150_000.0),
            Mode::Bask => self.rng.range(8_000.0, 20_000.0),
            Mode::Stretch => s.once_ms(1_500.0),
            Mode::Look => 20_000.0,
            Mode::Out => 0.0,
            Mode::Alert => s.once_ms(0.0).max(1_200.0),
        };
    }

    /// Bakış bitti: paket isterse baş geriye oynatılarak indirilir.
    fn leave_look(&mut self) {
        let s = self.strip();
        if s.reverse_out && s.animates() && self.frame > 0 {
            let f = self.frame;
            self.enter(Mode::Out);
            self.frame = f;
            self.mode_left = f as f32 * s.frame_ms() + 1.0;
        } else {
            self.enter(Mode::Idle);
        }
    }

    /// Oturma süresi dolunca ne yapılacağı.
    fn choose(&mut self, lo: f32, hi: f32) {
        let sleepy = if self.quiet > 120_000.0 { 0.35 } else { 0.12 };
        let r = self.rng.f();
        if r < sleepy {
            self.enter(Mode::Sleep);
        } else if r < sleepy + 0.3 && hi - lo > 8.0 {
            let mut to = self.rng.range(lo, hi);
            // Çok kısa yürüyüş anlamsız: en az bir gövde boyu.
            if (to - self.x).abs() < 24.0 {
                to = if self.x - lo > hi - self.x { lo + (self.x - lo) * 0.3 } else { hi - (hi - self.x) * 0.3 };
            }
            self.right = to >= self.x;
            self.enter(Mode::Walk { to });
        } else if r < sleepy + 0.4 && self.own(Anim::Stretch) {
            self.enter(Mode::Stretch);
        } else if let Some(m) = self.extra(r - (sleepy + 0.4)) {
            if m == Mode::Hop {
                // Yer olan yöne zıpla.
                self.right = hi - self.x > self.x - lo;
            }
            self.enter(m);
        } else if self.strips[Anim::Sit as usize].animates() && r < sleepy + 0.85 {
            self.enter(Mode::Fidget);
        } else {
            self.enter(Mode::Idle);
        }
    }

    /// Ek hareketler (yalanma, güneşlenme, zıplama) `EXTRA` genişliğindeki
    /// olasılık dilimini paketin sahip olduklarına eşit böler; `u` dilim
    /// içindeki konum. Biri öbürünü gölgelemesin diye hepsi aynı dilimde.
    fn extra(&self, u: f32) -> Option<Mode> {
        const EXTRA: f32 = 0.15;
        if !(0.0..EXTRA).contains(&u) {
            return None;
        }
        let own: Vec<Mode> = [(Anim::Lick, Mode::Lick), (Anim::Bask, Mode::Bask), (Anim::Hop, Mode::Hop)].into_iter().filter(|(a, _)| self.own(*a)).map(|(_, m)| m).collect();
        let i = ((u / EXTRA) * own.len() as f32) as usize;
        own.get(i.min(own.len().saturating_sub(1))).copied()
    }

    fn own(&self, a: Anim) -> bool {
        self.strips[a as usize].own
    }

    /// `dt` ms ilerletir. `lo..hi`: yürünebilecek aralık (dock'un üst kenarı).
    pub fn update(&mut self, dt: f32, lo: f32, hi: f32) {
        let dt = if dt.is_finite() { dt.clamp(0.0, 600_000.0) } else { 0.0 };
        let (lo, hi) = if hi >= lo { (lo, hi) } else { (hi, lo) };
        self.quiet += dt;
        if self.animating() {
            self.frame_left -= dt;
            let s = self.strip();
            // Uzun duraklamadan sonra yüzlerce kare atlamayalım.
            if self.frame_left < -2_000.0 {
                self.frame_left = 0.0;
            }
            while self.frame_left <= 0.0 {
                self.frame_left += s.frame_ms();
                let next = self.frame + 1;
                // Döngüdekiler döner; bir kez oynayanlar son karede kalır.
                self.frame = if self.mode == Mode::Out {
                    self.frame.saturating_sub(1)
                } else if self.looping() {
                    next % s.frames
                } else {
                    next.min(s.frames - 1)
                };
                self.dirty = true;
            }
        }
        if self.mode == Mode::Hop && HOP_AIR.contains(&self.frame) {
            let before = self.x;
            let step = self.speed * HOP_SPEED * dt / 1000.0;
            self.x += if self.right { step } else { -step };
            if self.x.round() != before.round() {
                self.dirty = true;
            }
        }
        if let Mode::Walk { to } = self.mode {
            let to = to.clamp(lo, hi);
            let step = self.speed * dt / 1000.0;
            let before = self.x;
            if (to - self.x).abs() <= step {
                self.x = to;
                self.enter(Mode::Idle);
            } else {
                self.x += step.copysign(to - self.x);
            }
            if self.x.round() != before.round() {
                self.dirty = true;
            }
        }
        let clamped = self.x.clamp(lo, hi);
        if clamped != self.x {
            self.x = clamped;
            self.dirty = true;
        }
        self.mode_left -= dt;
        if self.mode_left <= 0.0 {
            match self.mode {
                Mode::Idle => self.choose(lo, hi),
                Mode::Sleep if self.own(Anim::Stretch) => self.enter(Mode::Stretch),
                Mode::Look => self.leave_look(),
                _ => self.enter(Mode::Idle),
            }
        }
    }

    /// İmleç konumu (aynı eksende); `None` uzakta/dışarıda.
    pub fn cursor(&mut self, cx: Option<f32>, radius: f32) {
        let near = cx.filter(|c| (c - self.x).abs() < radius);
        match (self.mode, near) {
            (Mode::Idle | Mode::Fidget | Mode::Look | Mode::Out, Some(c)) => {
                self.quiet = 0.0;
                let right = c >= self.x;
                if self.mode == Mode::Out {
                    // Başını indirirken imleç geri geldi: kaldığı kareden yeniden kalksın.
                    let f = self.frame;
                    self.enter(Mode::Look);
                    self.frame = f;
                } else if self.mode != Mode::Look {
                    self.enter(Mode::Look);
                } else if self.mode_left < 15_000.0 {
                    // Hâlâ yakında: bakmayı sürdür.
                    self.mode_left = 20_000.0;
                }
                if right != self.right {
                    self.right = right;
                    self.dirty = true;
                }
            }
            (Mode::Look, None) => self.leave_look(),
            _ => {}
        }
    }

    /// Bir uygulama açıldı: uyuyorsa uyanır, değilse irkilir.
    pub fn poke(&mut self) {
        self.quiet = 0.0;
        if self.mode != Mode::Alert {
            self.enter(Mode::Alert);
        }
    }

    /// Bir sonraki görsel değişikliğe kadar ms. Uyurken bu uyanma anıdır
    /// (onlarca saniye): arada hiç çizim yapılmaz.
    pub fn next_ms(&self) -> f32 {
        let mut ms = self.mode_left.max(1.0);
        if self.animating() {
            ms = ms.min(self.frame_left.max(1.0));
        }
        if let Mode::Walk { .. } = self.mode {
            // Kare değişmese de konum akar: yürüme şeridinin hızında güncelle.
            let fps = self.strip().fps.clamp(8.0, 12.0);
            ms = ms.min(1000.0 / fps);
        }
        ms
    }

    /// Son çağrıdan beri görünüş değişti mi?
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(frames: u32, fps: f32) -> Strip {
        Strip { frames, fps, own: true, reverse_out: false }
    }

    fn strips() -> [Strip; N_ANIMS] {
        [
            s(4, 8.0),  // sit
            s(6, 10.0), // walk
            s(1, 0.0),  // sleep
            s(4, 6.0),  // stretch
            Strip { reverse_out: true, ..s(4, 9.0) }, // look
            s(2, 6.0),  // alert
            s(6, 7.7),  // lick
            s(4, 2.2),  // bask
            s(6, 14.0), // hop
        ]
    }

    fn pet(seed: u32) -> Pet {
        Pet::new(strips(), 30.0, 200.0, Rng::new(seed))
    }

    #[test]
    fn ayni_tohum_ayni_davranis() {
        let (mut a, mut b) = (pet(5), pet(5));
        for i in 0..20_000 {
            let dt = 50.0 + (i % 7) as f32 * 30.0;
            a.update(dt, 0.0, 400.0);
            b.update(dt, 0.0, 400.0);
            assert_eq!((a.anim(), a.frame, a.x.to_bits(), a.right), (b.anim(), b.frame, b.x.to_bits(), b.right));
        }
    }

    #[test]
    fn uzun_calisma_tum_durumlar_ve_sinirlar() {
        let mut p = pet(1);
        let mut seen = std::collections::HashSet::new();
        let mut r = Rng::new(77);
        for i in 0..200_000 {
            // Gerçek zamanlayıcı gibi: istenen beklemeyi kullan.
            let dt = p.next_ms().min(5_000.0);
            p.update(dt, 10.0, 390.0);
            if i % 500 == 0 {
                p.cursor(Some(r.range(0.0, 400.0)), LOOK_RADIUS);
            } else if i % 500 == 50 {
                p.cursor(None, LOOK_RADIUS);
            }
            if i % 3000 == 0 {
                p.poke();
            }
            assert!(p.x.is_finite() && (10.0..=390.0).contains(&p.x));
            assert!(p.frame < strips()[p.anim() as usize].frames);
            seen.insert(p.anim());
        }
        for a in crate::live::pack::ANIMS {
            assert!(seen.contains(&a), "{a:?} hiç görülmedi");
        }
    }

    #[test]
    fn uyurken_cizim_yok() {
        let mut p = pet(2);
        p.enter(Mode::Sleep);
        p.take_dirty();
        let wait = p.next_ms();
        assert!(wait >= 40_000.0, "uyurken tek uzun bekleme: {wait}");
        // Uyanma anından önceki her güncellemede kare değişmez.
        let mut t = 0.0;
        while t + 1_000.0 < wait {
            p.update(1_000.0, 0.0, 400.0);
            t += 1_000.0;
            assert!(p.sleeping());
            assert!(!p.take_dirty());
        }
    }

    #[test]
    fn dururken_bir_sonraki_olaya_kadar_bekler() {
        let mut p = pet(3);
        p.enter(Mode::Idle);
        assert!(p.next_ms() >= 4_000.0);
    }

    #[test]
    fn imlece_bakar_ve_birakir() {
        let mut p = pet(4);
        p.enter(Mode::Idle);
        p.cursor(Some(150.0), LOOK_RADIUS);
        assert_eq!(p.anim(), Anim::Look);
        assert!(!p.right);
        p.cursor(Some(260.0), LOOK_RADIUS);
        assert!(p.right);
        p.cursor(Some(900.0), LOOK_RADIUS);
        assert_eq!(p.anim(), Anim::Sit);
    }

    #[test]
    fn uygulama_acilinca_uyanir() {
        let mut p = pet(6);
        p.enter(Mode::Sleep);
        // Uyurken imleç uyandırmaz.
        p.cursor(Some(200.0), LOOK_RADIUS);
        assert!(p.sleeping());
        p.poke();
        assert_eq!(p.anim(), Anim::Alert);
        p.update(p.next_ms() + 1.0, 0.0, 400.0);
        p.update(p.next_ms() + 1.0, 0.0, 400.0);
        assert!(!p.sleeping());
    }

    #[test]
    fn bakis_kalir_ve_geri_oynar() {
        let mut p = pet(9);
        p.enter(Mode::Idle);
        p.cursor(Some(220.0), LOOK_RADIUS);
        for _ in 0..10 {
            p.update(120.0, 0.0, 400.0);
        }
        // Baş kalktı ve son karede bekliyor: kare ilerlemiyor.
        assert_eq!((p.anim(), p.frame), (Anim::Look, 3));
        assert!(p.next_ms() > 1_000.0);
        // İmleç gitti: geriye oynar, sonra oturur.
        p.cursor(None, LOOK_RADIUS);
        assert_eq!((p.mode, p.frame), (Mode::Out, 3));
        let mut last = 3;
        for _ in 0..10 {
            if p.mode != Mode::Out {
                break;
            }
            p.update(p.next_ms(), 0.0, 400.0);
            if p.mode == Mode::Out {
                assert!(p.frame <= last);
                last = p.frame;
            }
        }
        // Geri oynama bitince oturur (sonraki seçime kadar).
        assert_eq!((p.mode, p.anim()), (Mode::Idle, Anim::Sit));
    }

    #[test]
    fn ziplama_havadayken_ilerler() {
        let mut p = pet(10);
        p.right = true;
        p.enter(Mode::Hop);
        let x0 = p.x;
        let mut moved_on_ground = false;
        while p.mode == Mode::Hop {
            let (f, x) = (p.frame, p.x);
            p.update(20.0, 0.0, 400.0);
            if !HOP_AIR.contains(&f) && !HOP_AIR.contains(&p.frame) && p.x != x {
                moved_on_ground = true;
            }
        }
        assert!(p.x > x0 + 5.0, "zıplayınca ileri gider: {} → {}", x0, p.x);
        assert!(!moved_on_ground);
    }

    #[test]
    fn yuruyus_hedefe_varir() {
        let mut p = pet(8);
        p.right = false;
        p.enter(Mode::Walk { to: 300.0 });
        for _ in 0..200 {
            p.update(100.0, 0.0, 400.0);
            if p.anim() != Anim::Walk {
                break;
            }
            assert!(p.next_ms() <= 125.0, "yürürken 8-12 fps");
        }
        assert_eq!(p.x, 300.0);
        // Aralık daralırsa içeride kalır.
        p.update(10.0, 0.0, 120.0);
        assert_eq!(p.x, 120.0);
    }
}
