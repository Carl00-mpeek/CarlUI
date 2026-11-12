//! Yılanın davranışı: saf durum makinesi (saat dışarıdan, rastgelelik
//! tohumdan). Birimler sanat pikseli; "dünya" x'i tünek penceresinin sol
//! kenarından, y = 0 dock'un üst kenarı (aşağı pozitif).
//!
//! Yılan hep yerel +x'e gider; `dir` = -1 iken ekranda aynalanır. Dönmek için
//! "makara" hilesi: gövde bir noktada (P) kıvrımın içine sarılır (CoilIn),
//! bir süre kıvrık durur (Coiled), sonra seçilen yönde P'den yeniden açılır
//! (Uncoil). Böylece hiçbir karede gövde kendi üstünden dönmez.
//!
//! Ara sıra dock'un üst kenarına 2-3 tur sarılır (bkz. `path::Wrap`), ara
//! sıra yolda kıvrılır; imleç yakınsa durup boynunu kaldırır ve başını imlece
//! çevirir (en yakın eğim karesi).

use super::{
    art::{body_ops, Face, Op, CELL, COIL_H, COIL_W, HEAD_PIVOT, HEAD_W},
    path::{self, Track, Wrap, BEADS, LEN, STEP},
};
use crate::live::rng::Rng;

/// Tuval boyutu (sanat pikseli) ve dock kenarının üstünde kalan kısmı.
pub const CW: i32 = 224;
pub const CH: i32 = 72;
pub const UP: i32 = 48;

/// Kayma hızı (iz boyunca, px/sn); sarılırken iki katı.
const SPEED: f32 = 16.0;
/// Kıvrımın merkezi, giriş noktasının bu kadar ilerisinde.
const COIL_AHEAD: f32 = 10.0;
/// Bitişe en az bu kadar kala kıvrılır (kıvrımın sağ yarısı + pay).
const END_PAD: f32 = 40.0;
/// İmleç bu kutudaysa yılan ona bakar (baştan).
const LOOK_X: f32 = 70.0;
const LOOK_Y: (f32, f32) = (-90.0, 60.0);
/// Kıvrık yılanın başı (kıvrım merkezine göre, yerel).
const COIL_HEAD: (f32, f32) = (14.0, -30.0);
/// Nefes karesi aralığı.
const BREATH: f32 = 0.45;
const BLINK: f32 = 0.15;
const TONGUE_STEP: f32 = 0.08;
const TONGUE: [u8; 5] = [0, 1, 2, 1, 0];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    /// Kıvrımdan açılıyor: iz konumu < 0 olan boncuklar hâlâ kıvrımda.
    Uncoil,
    Glide,
    /// `at`'ten ileri giden boncuklar kıvrıma sarılmış (görünmez).
    CoilIn { at: f32 },
    Coiled { left: f32 },
    /// Dar dock: tam açılacak yer yok; baş `to`'ya kadar çıkar, `hold` saniye
    /// bekler, sonra geri sarılır (`Retract`). Kuyruk kıvrımda kalır.
    Peek { to: f32, hold: f32 },
    Retract,
}

/// Dar dockta başın kenara bırakacağı pay ve değmesi için en kısa çıkış.
const PEEK_PAD: f32 = 10.0;
const PEEK_MIN: f32 = 24.0;

pub struct Brain {
    pub mode: Mode,
    /// +1 sağa, -1 sola.
    pub dir: f32,
    /// Yerel x = 0'ın dünya x'i (tam sayı; tuval pikselle hizalı kalsın).
    pub ox: f32,
    pub track: Track,
    pub head: f32,
    /// Boyun kalkıklığı 0..1.
    pub raise: f32,
    /// Kıvrım merkezinin dünya x'i.
    coil_c: f32,
    breath: u8,
    breath_t: f32,
    /// Kıvrıkken baş imlece doğru kalkık.
    up: bool,
    blink_t: f32,
    tongue_t: f32,
    /// Dil çıkarma sırası: (adım, kalan süre).
    tongue: Option<(usize, f32)>,
    /// Kırpma süresi.
    blinking: f32,
    wrap_t: f32,
    coil_t: f32,
    /// İmleç (yerel, baştan ya da kıvrık başa göre): bakış için.
    look: Option<(f32, f32)>,
    rng: Rng,
}

/// Bir karenin çizimi: tuval x'i, yön ve ön/arka çizim listeleri.
#[derive(Debug, PartialEq)]
pub struct Frame {
    pub dir: f32,
    pub ox: f32,
    /// Tuvalin sol kenarının yerel x'i.
    pub x0: i32,
    pub front: Vec<Op>,
    pub back: Vec<Op>,
}

impl Brain {
    /// `x` dünya konumunda kıvrık başlar; kısa süre sonra açılır.
    pub fn new(x: f32, mut rng: Rng) -> Self {
        let dir = if rng.f() < 0.5 { 1.0 } else { -1.0 };
        let mut b = Self {
            mode: Mode::Coiled { left: rng.range(1.0, 3.0) },
            dir,
            ox: x.round(),
            track: Track::default(),
            head: 0.0,
            raise: 0.0,
            coil_c: x,
            breath: 0,
            breath_t: BREATH,
            up: false,
            blink_t: 0.0,
            tongue_t: 0.0,
            tongue: None,
            blinking: 0.0,
            wrap_t: 0.0,
            coil_t: 0.0,
            look: None,
            rng,
        };
        b.blink_t = b.rng.range(3.0, 7.0);
        b.tongue_t = b.rng.range(4.0, 9.0);
        b.wrap_t = b.rng.range(8.0, 20.0);
        b.coil_t = b.rng.range(25.0, 50.0);
        b
    }

    fn world(&self, lx: f32) -> f32 {
        self.ox + self.dir * lx
    }

    fn head_pt(&self) -> path::Pt {
        self.track.at(self.head)
    }

    /// Başın dünya x'i.
    pub fn head_x(&self) -> f32 {
        self.world(self.head_pt().x)
    }

    /// Bitişe olan uzaklık (gidiş yönünde).
    fn room(&self, x: f32, lo: f32, hi: f32) -> f32 {
        if self.dir > 0.0 { hi - x } else { x - lo }
    }

    pub fn face(&self) -> Face {
        if let Some((i, _)) = self.tongue {
            Face::Tongue(TONGUE[i])
        } else if self.blinking > 0.0 {
            Face::Blink
        } else {
            Face::Open
        }
    }

    pub fn coiled(&self) -> bool {
        matches!(self.mode, Mode::Coiled { .. })
    }

    /// Dürtme (yeni pencere): kıvrıksa hemen açılır, değilse dil çıkarır.
    pub fn poke(&mut self) {
        if let Mode::Coiled { left } = &mut self.mode {
            *left = left.min(0.3);
        }
        self.tongue = Some((0, TONGUE_STEP));
    }

    fn start_coil(&mut self) {
        let at = self.head;
        let p = self.track.at(at).x;
        self.coil_c = self.world(p + COIL_AHEAD);
        self.raise = 0.0;
        self.mode = Mode::CoilIn { at };
    }

    fn start_uncoil(&mut self, dir: f32) {
        self.dir = dir;
        self.ox = (self.coil_c - COIL_AHEAD * dir).round();
        self.track = Track::default();
        self.head = 0.0;
        self.mode = Mode::Uncoil;
    }

    /// Bakışın çıktığı nokta (dünya): kıvrıksa kıvrımın başı, değilse baş.
    pub fn look_from(&self) -> (f32, f32) {
        if self.coiled() {
            (self.coil_c + self.dir * COIL_HEAD.0, COIL_HEAD.1)
        } else {
            let p = self.head_pt();
            (self.world(p.x), p.y - path::RAISE * self.raise)
        }
    }

    /// Yalnız geliştirme (`debug.rs`): ilk fırsatta helezona girsin, bu
    /// arada kıvrılmaya kalkmasın.
    /// Kıvrıksa geniş yana hemen açılır (ortadan açılan yılanın önünde
    /// helezon için yer kalmaz); kayarken ilk fırsatta helezona girer.
    pub fn force_wrap(&mut self, lo: f32, hi: f32) {
        if self.coiled() {
            let dir = if hi - self.coil_c >= self.coil_c - lo { 1.0 } else { -1.0 };
            self.start_uncoil(dir);
        } else if self.mode == Mode::Glide && self.track.wrap.is_none() {
            self.wrap_t = 0.0;
            self.coil_t = self.coil_t.max(20.0);
        }
    }

    /// İmleç dünya konumunu (varsa) yerel bakış vektörüne çevirir.
    fn update_look(&mut self, cursor: Option<(f32, f32)>) {
        let from = self.look_from();
        self.look = cursor.and_then(|(cx, cy)| {
            let (dx, dy) = (cx - from.0, cy - from.1);
            (dx.abs() < LOOK_X && cy > LOOK_Y.0 && cy < LOOK_Y.1).then_some((dx * self.dir, dy))
        });
    }

    /// Yüz ifadesi zamanlayıcıları.
    fn faces(&mut self, dt: f32) {
        if let Some((i, t)) = self.tongue.as_mut() {
            *t -= dt;
            if *t <= 0.0 {
                *i += 1;
                *t = TONGUE_STEP;
                if *i >= TONGUE.len() {
                    self.tongue = None;
                    self.tongue_t = self.rng.range(4.0, 9.0);
                }
            }
        } else {
            self.tongue_t -= dt;
            if self.tongue_t <= 0.0 {
                self.tongue = Some((0, TONGUE_STEP));
            }
        }
        if self.blinking > 0.0 {
            self.blinking -= dt;
        } else {
            self.blink_t -= dt;
            if self.blink_t <= 0.0 {
                self.blinking = BLINK;
                self.blink_t = self.rng.range(3.0, 7.0);
            }
        }
    }

    /// `dt` saniye ilerletir. `lo..hi` başın dünya aralığı; `cursor` imlecin
    /// dünya konumu. Dönüş: bir sonraki tike kadar beklenecek ms.
    pub fn step(&mut self, dt: f32, lo: f32, hi: f32, cursor: Option<(f32, f32)>) -> f32 {
        let dt = dt.clamp(0.0, 0.25);
        self.faces(dt);
        self.update_look(cursor);
        match self.mode {
            Mode::Coiled { left } => {
                // Dock daralmışsa kıvrım aralığa çekilir.
                self.coil_c = self.coil_c.clamp(lo.min(hi), hi.max(lo));
                self.breath_t -= dt;
                if self.breath_t <= 0.0 {
                    self.breath = (self.breath + 1) % 4;
                    self.breath_t = BREATH;
                }
                self.up = false;
                if let Some((dx, dy)) = self.look {
                    // Arkadaki imlece dönmek için yalnızca aynalamak yeter.
                    if dx < -6.0 {
                        self.dir = -self.dir;
                    }
                    self.up = (-dy).atan2(dx.abs()) > std::f32::consts::FRAC_PI_4;
                }
                let left = left - dt;
                // Açılırken baş bir boy ilerler; o yanda bir boy + pay yer olmalı.
                let need = LEN + END_PAD;
                let (rr, ll) = (hi - self.coil_c + COIL_AHEAD, self.coil_c - lo + COIL_AHEAD);
                let (r, l) = (rr >= need, ll >= need);
                if left > 0.0 {
                    self.mode = Mode::Coiled { left };
                } else if !(r || l) {
                    // Dar dock: geniş yana sığdığı kadar başını çıkarıp geri sarılır
                    // (sonsuza dek kıvrık durmasın).
                    let (dir, room) = if rr >= ll { (1.0, rr) } else { (-1.0, ll) };
                    let to = (room - PEEK_PAD).min(LEN - STEP);
                    if to >= PEEK_MIN {
                        self.up = false;
                        self.start_uncoil(dir);
                        self.mode = Mode::Peek { to, hold: self.rng.range(1.5, 4.0) };
                    } else {
                        self.mode = Mode::Coiled { left: 3.0 };
                    }
                } else {
                    // Boş tarafa; iki yan da boşsa imlece ya da rastgele.
                    let dir = if !r {
                        -1.0
                    } else if !l {
                        1.0
                    } else if self.look.is_some() {
                        self.dir
                    } else if self.rng.f() < 0.5 {
                        1.0
                    } else {
                        -1.0
                    };
                    self.up = false;
                    self.start_uncoil(dir);
                }
            }
            Mode::CoilIn { at } => {
                self.head += SPEED * 2.0 * dt;
                if self.head - LEN > at {
                    self.mode = Mode::Coiled { left: self.rng.range(4.0, 10.0) };
                    self.breath_t = BREATH;
                }
            }
            Mode::Uncoil => {
                self.head += SPEED * dt;
                if self.head - LEN >= 0.0 {
                    self.mode = Mode::Glide;
                    self.wrap_t = self.wrap_t.max(3.0);
                }
            }
            Mode::Peek { to, hold } => {
                if self.head < to {
                    self.head = (self.head + SPEED * dt).min(to);
                } else if hold > dt {
                    self.mode = Mode::Peek { to, hold: hold - dt };
                } else {
                    self.mode = Mode::Retract;
                }
            }
            Mode::Retract => {
                self.head -= SPEED * dt;
                if self.head <= 0.0 {
                    self.head = 0.0;
                    self.mode = Mode::Coiled { left: self.rng.range(4.0, 10.0) };
                    self.breath_t = BREATH;
                }
            }
            Mode::Glide => self.glide(dt, lo, hi),
        }
        let mut pos = [&mut self.head];
        let tail = *pos[0] - LEN;
        let had = self.track.wrap;
        self.track.rebase(tail, &mut pos);
        if let (Some(w), None, Mode::CoilIn { at }) = (had, self.track.wrap, &mut self.mode) {
            *at += w.dx() - w.arc();
        }
        self.next_ms()
    }

    fn glide(&mut self, dt: f32, lo: f32, hi: f32) {
        // Bakış yalnızca baş helezondan çıkmışken (boyun düz izde kalkar).
        let free = self.track.wrap.map_or(true, |w| self.head > w.at + w.arc() + path::NECK as f32 * STEP);
        let looking = self.look.is_some() && free;
        let target = if looking { 1.0 } else { 0.0 };
        let rate = 3.0 * dt;
        self.raise = if self.raise < target { (self.raise + rate).min(target) } else { (self.raise - rate).max(target) };
        if looking || self.raise > 0.0 {
            return;
        }
        self.head += SPEED * dt;
        let hx = self.head_x();
        if self.room(hx, lo, hi) < END_PAD {
            self.start_coil();
            return;
        }
        if self.track.wrap.is_none() {
            self.wrap_t -= dt;
            self.coil_t -= dt;
            if self.coil_t <= 0.0 {
                self.coil_t = self.rng.range(25.0, 50.0);
                self.start_coil();
            } else if self.wrap_t <= 0.0 {
                self.wrap_t = self.rng.range(10.0, 25.0);
                let w = Wrap { at: self.head + 6.0, loops: if self.rng.f() < 0.5 { 2.0 } else { 3.0 }, rr: 11.0 };
                // Helezon ve ardından bir boy düz yol sığmalı.
                if self.room(hx, lo, hi) > w.dx() + END_PAD + 60.0 {
                    self.track.wrap = Some(w);
                }
            }
        }
    }

    fn next_ms(&self) -> f32 {
        let moving = match self.mode {
            Mode::Glide => self.look.is_none() || self.raise < 1.0,
            Mode::Uncoil | Mode::CoilIn { .. } | Mode::Retract => true,
            Mode::Peek { to, .. } => self.head < to,
            Mode::Coiled { .. } => false,
        };
        if moving {
            return 66.0;
        }
        // Durgun: bir sonraki olaya (nefes, kırpma, dil) kadar bekle.
        let mut t = if let Some((_, s)) = self.tongue { s } else { self.tongue_t };
        t = t.min(if self.blinking > 0.0 { self.blinking } else { self.blink_t });
        match self.mode {
            Mode::Coiled { left } => t = t.min(self.breath_t).min(left),
            Mode::Peek { hold, .. } => t = t.min(hold),
            _ => {}
        }
        if self.look.is_some() {
            t = t.min(0.1);
        }
        (t * 1000.0).clamp(33.0, 2000.0)
    }

    /// Şimdiki pozun çizim listeleri.
    pub fn frame(&self) -> Frame {
        let mut pts = path::body(&self.track, self.head);
        if self.raise > 0.0 {
            path::raise(&mut pts, self.raise);
        }
        let s_of = |i: usize| self.head - (BEADS - 1 - i) as f32 * STEP;
        let (a, e) = match self.mode {
            Mode::Uncoil | Mode::Peek { .. } | Mode::Retract => ((0..BEADS).find(|&i| s_of(i) >= 0.0).unwrap_or(BEADS), BEADS),
            Mode::CoilIn { at } => (0, (0..BEADS).find(|&i| s_of(i) > at).unwrap_or(BEADS)),
            Mode::Coiled { .. } => (0, 0),
            Mode::Glide => (0, BEADS),
        };
        let all = path::beads(&pts);
        let b = &all[a..e];
        let face = self.face();
        let head = (e == BEADS && a < e).then(|| {
            let pitch = match self.look {
                Some((dx, dy)) if self.raise > 0.5 => {
                    // Arkadaki imlece başı tam yukarı kaldırır (ayna baş yok).
                    if dx < 0.0 { 4 } else { path::pitch_of((dx, dy)) as u8 }
                }
                _ => path::pitch_of(path::head_tangent(&pts)) as u8,
            };
            (pitch, face)
        });
        let coil = !matches!(self.mode, Mode::Glide);
        let cx = ((self.coil_c - self.ox) * self.dir).round() as i32;
        // Tuvalin sağ kenarı içeriğin en sağına göre (gövde ≤ tuval).
        let mut right = i32::MIN;
        for bd in b {
            right = right.max(bd.x + CELL / 2 + 1);
        }
        if let Some(last) = b.last().filter(|_| head.is_some()) {
            right = right.max(last.x + HEAD_W - HEAD_PIVOT.0);
        }
        if coil {
            right = right.max(cx + COIL_W / 2 + 1);
        }
        let x0 = right + 2 - CW;
        let (mut front, mut back) = (Vec::new(), Vec::new());
        if a < e {
            body_ops(b, head, -x0, UP, &mut front, &mut back);
        }
        if coil {
            // Kıvrıkken baş kıvrımın kendisinde; geçişlerde gövdenin başı var.
            let (up, frame, face, bare) = match self.mode {
                Mode::Coiled { .. } => (self.up, self.breath, face, false),
                _ => (false, 0, Face::Open, true),
            };
            front.push(Op::Coil { up, frame, face, bare, x: cx - COIL_W / 2 - x0, y: -COIL_H + UP });
        }
        Frame { dir: self.dir, ox: self.ox, x0, front, back }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(b: &mut Brain, secs: f32, lo: f32, hi: f32, cursor: Option<(f32, f32)>, mut f: impl FnMut(&Brain)) {
        let mut t = 0.0;
        while t < secs {
            let ms = b.step(0.066, lo, hi, cursor);
            assert!((33.0..=2000.0).contains(&ms));
            f(b);
            t += 0.066;
        }
    }

    #[test]
    fn aralikta_kalir_ve_doner() {
        let (lo, hi) = (40.0, 600.0);
        let mut b = Brain::new(300.0, Rng::new(7));
        let (mut dirs, mut wraps, mut coils) = (Vec::new(), 0, 0);
        let mut was_coiled = true;
        run(&mut b, 600.0, lo, hi, None, |b| {
            // Sarılırken baş kıvrımın içinde (görünmez) ilerler.
            if matches!(b.mode, Mode::Glide | Mode::Uncoil) {
                let x = b.head_x();
                assert!(x > lo - 1.0 && x < hi + 1.0, "baş aralık dışında: {x}");
            }
            if b.track.wrap.is_some() && b.head > b.track.wrap.unwrap().at + 50.0 {
                wraps += 1;
            }
            let c = b.coiled();
            if c && !was_coiled {
                coils += 1;
            }
            was_coiled = c;
            if dirs.last() != Some(&b.dir) {
                dirs.push(b.dir);
            }
        });
        assert!(dirs.len() >= 3, "yön değişmedi: {dirs:?}");
        assert!(wraps > 0 && coils >= 2);
    }

    #[test]
    fn makara_gizler_ve_acar() {
        let mut b = Brain::new(300.0, Rng::new(3));
        // Kıvrıkken gövde ve baş yok, yalnızca kıvrım.
        let f = b.frame();
        assert!(f.back.is_empty() && f.front.len() == 1 && matches!(f.front[0], Op::Coil { .. }));
        // Açılırken baş görünür, kuyruk hâlâ kıvrımda; tam açılınca kıvrım gider.
        let mut seen_partial = false;
        run(&mut b, 20.0, 0.0, 2000.0, None, |b| {
            if b.mode == Mode::Uncoil && b.head > 20.0 {
                let f = b.frame();
                let lines = f.front.iter().filter(|o| matches!(o, Op::Line { .. })).count();
                assert!(lines < BEADS && f.front.iter().any(|o| matches!(o, Op::HeadFill { .. })));
                assert!(matches!(f.front.last(), Some(Op::Coil { .. })));
                seen_partial = true;
            }
        });
        assert!(seen_partial);
    }

    #[test]
    fn cizim_tuvale_sigar() {
        let mut b = Brain::new(400.0, Rng::new(11));
        run(&mut b, 300.0, 40.0, 800.0, None, |b| {
            let f = b.frame();
            for op in f.front.iter().chain(&f.back) {
                let (x, y, w, h) = match *op {
                    Op::Line { x, y, .. } | Op::Fill { x, y, .. } => (x, y, 9, 9),
                    Op::HeadLine { x, y, .. } | Op::HeadFill { x, y, .. } => (x, y, HEAD_W, super::super::art::HEAD_H),
                    Op::Coil { x, y, .. } => (x, y, COIL_W, COIL_H),
                };
                assert!(x >= -2 && x + w <= CW + 2, "{op:?} x0={}", f.x0);
                assert!(y + h > 0 && y < CH, "{op:?}");
            }
        });
    }

    #[test]
    fn imlece_bakar() {
        let mut b = Brain::new(300.0, Rng::new(5));
        // Açılıp kaymaya başlayana dek.
        run(&mut b, 15.0, 0.0, 3000.0, None, |_| {});
        assert_eq!(b.mode, Mode::Glide);
        let hx = b.head_x();
        let stop = b.head;
        // İmleç başın önünde ve yukarıda: durur, boyun kalkar, baş yukarı-ileri bakar.
        let cur = Some((hx + 30.0 * b.dir, -60.0));
        run(&mut b, 2.0, 0.0, 3000.0, cur, |_| {});
        assert!((b.head - stop).abs() < SPEED * 0.2 && b.raise >= 1.0);
        let f = b.frame();
        let pitch = f.front.iter().find_map(|o| if let Op::HeadFill { pitch, .. } = o { Some(*pitch) } else { None }).unwrap();
        assert!(path::PITCHES[pitch as usize] >= 30, "eğim {pitch}");
        // İmleç gidince yeniden kayar.
        run(&mut b, 2.0, 0.0, 3000.0, None, |_| {});
        assert!(b.head > stop + 5.0);
    }

    #[test]
    fn dar_dockta_bas_cikarip_geri_sarilir() {
        // Bir boy + pay hiçbir yana sığmaz: tam açılmaz ama kıvrık da kalmaz.
        let (lo, hi) = (200.0, 380.0);
        let mut b = Brain::new(290.0, Rng::new(4));
        let (mut peeks, mut back, mut max_out) = (0, 0, 0.0f32);
        let mut prev = b.mode;
        run(&mut b, 120.0, lo, hi, None, |b| {
            assert!(!matches!(b.mode, Mode::Glide | Mode::Uncoil), "dar dockta tam açıldı: {:?}", b.mode);
            if matches!(b.mode, Mode::Peek { .. } | Mode::Retract) {
                let x = b.head_x();
                assert!(x >= lo - 1.0 && x <= hi + 1.0, "baş aralık dışında: {x}");
                max_out = max_out.max(b.head);
                let f = b.frame();
                assert!(f.front.iter().any(|o| matches!(o, Op::HeadFill { .. })));
            }
            if matches!(b.mode, Mode::Peek { .. }) && !matches!(prev, Mode::Peek { .. }) {
                peeks += 1;
            }
            if b.coiled() && prev == Mode::Retract {
                back += 1;
            }
            prev = b.mode;
        });
        assert!(peeks >= 3 && back >= 3, "çıkış {peeks}, dönüş {back}");
        assert!(max_out >= PEEK_MIN, "baş yeterince çıkmadı: {max_out}");
    }

    #[test]
    fn kivrikken_imlece_doner_ve_kalkar() {
        let mut b = Brain::new(300.0, Rng::new(9));
        b.mode = Mode::Coiled { left: 100.0 };
        let d = b.dir;
        // İmleç arkada ve yüksekte.
        b.step(0.05, 0.0, 1000.0, Some((300.0 - 20.0 * d, -60.0)));
        assert_eq!(b.dir, -d);
        b.step(0.05, 0.0, 1000.0, Some((300.0 + 20.0 * b.dir, -60.0)));
        assert!(b.up);
        assert!(matches!(b.frame().front[0], Op::Coil { up: true, .. }));
    }
}
