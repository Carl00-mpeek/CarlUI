//! Kenar birikintisi: kar yağarken açık pencerelerin, dock'un ve bildirim
//! adasının üst kenarında biriken kar; yağmurda aynı kenarlarda ıslak parıltı.
//!
//! Bu dosya saf modeldir (Win32 yok, testli): kenarın önünde başka pencere
//! olmayan aralıkları bulma, sütun sütun kar yüksekliği, büyüme ve erime.
//! Pencereleri toplama ve çizim `ledge_view.rs`'de.
//!
//! Bir pencere taşınır ya da boyutu değişirse üstündeki kar düşer (sıfırdan
//! birikir); kar durunca birikinti yavaşça erir, yağmur onu daha hızlı eritir.

use super::rng::Rng;

/// Kenar başına en çok sütun: çok geniş pencerede bile bellek sabit.
pub const MAX_COLS: usize = 192;
/// En çok bu kadar yüzey izlenir (z sırasında öndekiler önce gelir).
pub const MAX_LEDGES: usize = 24;
/// Bundan kısa görünür aralıklar çizilmez (pencere arasında kalan kıymık).
pub const MIN_SPAN: i32 = 10;

/// Ekran dikdörtgeni: (sol, üst, sağ, alt).
pub type R4 = (i32, i32, i32, i32);

/// Ekrandaki bir üst kenar (toplayıcının çıktısı).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edge {
    /// Pencere tanıtıcısı ya da özel yüzeyler için sabit anahtar.
    pub key: isize,
    pub x0: i32,
    pub x1: i32,
    /// Üst kenarın ekran y'si.
    pub y: i32,
    /// Kenarın üstünde kar için boş alan (ekranın üst kenarına dayanmasın).
    pub room: i32,
    /// Uçlarda kar bu kadar pikselde incelir (yuvarlak köşeler).
    pub taper: i32,
    /// Kenarın önünde başka pencere olmayan aralıklar (ekran x'i, artan).
    pub spans: Vec<(i32, i32)>,
}

/// `spans` aralıklarından `cut` aralığını çıkarır.
pub fn subtract(spans: &mut Vec<(i32, i32)>, cut: (i32, i32)) {
    let (c0, c1) = cut;
    if c1 <= c0 {
        return;
    }
    let mut i = 0;
    while i < spans.len() {
        let (a, b) = spans[i];
        if c1 <= a || c0 >= b {
            i += 1;
            continue;
        }
        match (a < c0, c1 < b) {
            (true, true) => {
                spans[i] = (a, c0);
                spans.insert(i + 1, (c1, b));
                i += 2;
            }
            (true, false) => {
                spans[i] = (a, c0);
                i += 1;
            }
            (false, true) => {
                spans[i] = (c1, b);
                i += 1;
            }
            (false, false) => {
                spans.remove(i);
            }
        }
    }
}

/// `[x0, x1)` kenarının `top..bottom` şeridinde, öndeki dikdörtgenlerin
/// örtmediği aralıklar. Çok kısa parçalar atılır.
pub fn visible(x0: i32, x1: i32, top: i32, bottom: i32, above: &[R4]) -> Vec<(i32, i32)> {
    let mut spans = if x1 > x0 { vec![(x0, x1)] } else { Vec::new() };
    for &(l, t, r, b) in above {
        if spans.is_empty() {
            break;
        }
        // Şeritle dikeyde kesişmeyen pencere kenarı örtmez.
        if b <= top || t >= bottom {
            continue;
        }
        subtract(&mut spans, (l, r));
    }
    spans.retain(|&(a, b)| b - a >= MIN_SPAN);
    spans
}

/// Bir yüzeyin birikintisi.
pub struct Ledge {
    pub edge: Edge,
    /// Sütun yükseklikleri (piksel); sütun genişliği `col_w`.
    pub h: Vec<f32>,
    pub col_w: f32,
    /// Son çizimden beri biriken en büyük yükseklik değişimi.
    pub pending: f32,
    /// Yer, boy ya da görünür aralık değişti: yeniden çizilmeli.
    pub dirty: bool,
    /// Sütunlara serpiştirilen sabit pürüz (karın tepesi dümdüz olmasın).
    seed: f32,
}

impl Ledge {
    fn new(edge: Edge, s: f32, seed: f32) -> Self {
        let w = (edge.x1 - edge.x0).max(1) as f32;
        let col_w = (w / MAX_COLS as f32).max(3.0 * s);
        let n = ((w / col_w).ceil() as usize + 1).min(MAX_COLS + 1);
        Self { edge, h: vec![0.0; n], col_w, pending: 0.0, dirty: true, seed }
    }

    /// `i`. sütunun ekran x'i (sütun merkezi).
    pub fn col_x(&self, i: usize) -> f32 {
        self.edge.x0 as f32 + i as f32 * self.col_w
    }

    /// Ekran x'indeki kar yüksekliği (sütunlar arası doğrusal).
    pub fn height_at(&self, x: f32) -> f32 {
        let u = ((x - self.edge.x0 as f32) / self.col_w).max(0.0);
        let i = (u.floor() as usize).min(self.h.len() - 1);
        let j = (i + 1).min(self.h.len() - 1);
        let f = (u - i as f32).clamp(0.0, 1.0);
        self.h[i] * (1.0 - f) + self.h[j] * f
    }

    /// Sütunun taşıyabileceği en çok kar: uçlarda incelir, pürüzlü, boş alanla sınırlı.
    fn limit(&self, i: usize, max_h: f32) -> f32 {
        let x = self.col_x(i);
        let d = (x - self.edge.x0 as f32).min(self.edge.x1 as f32 - x).max(0.0);
        let taper = self.edge.taper.max(1) as f32;
        let u = (d / taper).clamp(0.0, 1.0);
        let ends = u * u * (3.0 - 2.0 * u);
        let k = i as f32;
        let bumps = 0.84 + 0.16 * (0.6 * (k * 0.9 + self.seed).sin() + 0.4 * (k * 0.31 + self.seed * 2.3).sin());
        (max_h * ends * bumps).min(self.edge.room.max(0) as f32)
    }

    /// Hiç kar yok.
    pub fn bare(&self) -> bool {
        self.h.iter().all(|&h| h < 0.05)
    }

    /// Sütunlar arası yumuşatma: tek sütun sivrilmesin.
    fn smooth(&mut self, k: f32) {
        let n = self.h.len();
        if n < 3 {
            return;
        }
        let mut prev = self.h[0];
        for i in 1..n - 1 {
            let cur = self.h[i];
            let avg = 0.25 * prev + 0.5 * cur + 0.25 * self.h[i + 1];
            self.h[i] = cur + (avg - cur) * k;
            prev = cur;
        }
    }
}

/// Tüm yüzeylerin birikintileri.
pub struct Ledges {
    pub list: Vec<Ledge>,
    s: f32,
    rng: Rng,
}

/// Şiddet (0..=1) için en yüksek kar kalınlığı (ölçek 1'de piksel).
pub fn max_height(level: f32, s: f32) -> f32 {
    (4.0 + 7.0 * level.clamp(0.0, 1.0)) * s
}

impl Ledges {
    pub fn new(s: f32, rng: Rng) -> Self {
        Self { list: Vec::new(), s: s.max(0.5), rng }
    }

    /// Toplayıcıdan gelen kenarlarla eşler. Yeri ya da boyu değişen yüzeyin
    /// karı düşer; artık görünmeyen yüzey bırakılır.
    pub fn sync(&mut self, mut edges: Vec<Edge>) {
        edges.truncate(MAX_LEDGES);
        let mut old = std::mem::take(&mut self.list);
        for e in edges {
            let found = old.iter().position(|l| l.edge.key == e.key);
            let ledge = match found.map(|i| old.swap_remove(i)) {
                Some(mut l) if same_place(&l.edge, &e) => {
                    if l.edge.spans != e.spans || l.edge.room != e.room {
                        l.dirty = true;
                    }
                    l.edge = e;
                    l
                }
                // Taşındı ya da yeni: kar sıfırdan birikir.
                _ => {
                    let seed = self.rng.range(0.0, 6.28);
                    Ledge::new(e, self.s, seed)
                }
            };
            self.list.push(ledge);
        }
    }

    /// `snow`: kar yağıyorsa şiddet (0..=1); `None` ise erir. `melt_fast`: yağmur.
    pub fn grow(&mut self, dt: f32, snow: Option<f32>, melt_fast: bool) {
        let dt = if dt.is_finite() { dt.clamp(0.0, 0.5) } else { 0.0 };
        if dt == 0.0 {
            return;
        }
        let s = self.s;
        for l in &mut self.list {
            let mut moved = 0.0f32;
            match snow {
                Some(level) => {
                    let max_h = max_height(level, s);
                    // Yoğun karda ~1 dk, hafifte ~3 dk'da dolar.
                    let rate = max_h / (50.0 + 130.0 * (1.0 - level.clamp(0.0, 1.0)));
                    for i in 0..l.h.len() {
                        let lim = l.limit(i, max_h);
                        let old = l.h[i];
                        let inc = rate * dt * self.rng.range(0.3, 1.7);
                        let new = if old > lim { (old - 0.2 * s * dt).max(lim) } else { (old + inc).min(lim) };
                        l.h[i] = new;
                        moved = moved.max((new - old).abs());
                    }
                }
                None => {
                    let rate = if melt_fast { 0.9 } else { 0.22 } * s;
                    for h in l.h.iter_mut() {
                        let old = *h;
                        *h = (old - rate * dt * self.rng.range(0.5, 1.5)).max(0.0);
                        moved = moved.max(old - *h);
                    }
                }
            }
            l.smooth((dt * 1.5).min(1.0));
            l.pending += moved;
        }
    }

    /// Azaltılmış hareket: kar beklemeden son kalınlığına oturur.
    pub fn settle(&mut self, level: f32) {
        let max_h = max_height(level, self.s);
        for l in &mut self.list {
            for i in 0..l.h.len() {
                l.h[i] = l.limit(i, max_h);
            }
            l.smooth(1.0);
            l.dirty = true;
        }
    }

    /// Hiçbir yüzeyde kar kalmadı.
    pub fn bare(&self) -> bool {
        self.list.iter().all(Ledge::bare)
    }
}

/// Aynı yerde mi (bir piksellik oynama sayılmaz)?
fn same_place(a: &Edge, b: &Edge) -> bool {
    (a.x0 - b.x0).abs() <= 1 && (a.x1 - b.x1).abs() <= 1 && (a.y - b.y).abs() <= 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(key: isize, x0: i32, x1: i32, y: i32) -> Edge {
        Edge { key, x0, x1, y, room: 40, taper: 8, spans: vec![(x0, x1)] }
    }

    #[test]
    fn aralik_cikarma() {
        let mut s = vec![(0, 100)];
        subtract(&mut s, (20, 30));
        assert_eq!(s, vec![(0, 20), (30, 100)]);
        subtract(&mut s, (-5, 5));
        assert_eq!(s, vec![(5, 20), (30, 100)]);
        subtract(&mut s, (15, 40));
        assert_eq!(s, vec![(5, 15), (40, 100)]);
        subtract(&mut s, (0, 200));
        assert!(s.is_empty());
        // Boş ya da ters kesim hiçbir şeyi değiştirmez.
        let mut s = vec![(0, 10)];
        subtract(&mut s, (5, 5));
        subtract(&mut s, (8, 2));
        assert_eq!(s, vec![(0, 10)]);
    }

    #[test]
    fn onde_pencere_karini_gizler() {
        // Kenar y=100, şerit 90..105. Öndeki pencere ortayı örter.
        let above = [(300, 50, 500, 400), (0, 200, 1000, 300)];
        let v = visible(0, 1000, 90, 105, &above);
        // İkinci pencere şeridin altında kalır: örtmez.
        assert_eq!(v, vec![(0, 300), (500, 1000)]);
        // Kıymık aralık atılır.
        let v = visible(0, 100, 90, 105, &[(5, 0, 100, 200)]);
        assert!(v.is_empty());
    }

    #[test]
    fn kar_birikir_sinirda_kalir_ve_erir() {
        let mut m = Ledges::new(1.0, Rng::new(7));
        m.sync(vec![edge(1, 0, 800, 300), Edge { room: 3, ..edge(2, 100, 400, 5) }]);
        for _ in 0..2000 {
            m.grow(0.1, Some(1.0), false);
        }
        let max_h = max_height(1.0, 1.0);
        for l in &m.list {
            assert!(l.h.iter().all(|h| h.is_finite() && *h >= 0.0 && *h <= max_h + 0.01));
            assert!(l.h.len() <= MAX_COLS + 1);
            // Uçlarda incelir.
            assert!(l.h[0] < 0.5 && *l.h.last().unwrap() < 0.5);
            // Boş alan sınırı aşılmaz.
            assert!(l.h.iter().all(|h| *h <= l.edge.room as f32 + 0.01));
        }
        // Ortada gerçekten birikti.
        let mid = m.list[0].height_at(400.0);
        assert!(mid > max_h * 0.6, "{mid}");
        assert!(m.list[0].pending > 0.0);
        // Kar durunca tamamen erir.
        for _ in 0..1000 {
            m.grow(0.1, None, false);
        }
        assert!(m.bare());
    }

    #[test]
    fn tasinan_pencerenin_kari_duser() {
        let mut m = Ledges::new(1.0, Rng::new(3));
        m.sync(vec![edge(1, 0, 400, 300), edge(2, 500, 900, 200)]);
        m.settle(1.0);
        assert!(!m.list[0].bare() && !m.list[1].bare());
        // 1 numara yerinde (bir piksel oynama), 2 numara taşındı, 3 yeni.
        m.sync(vec![edge(1, 1, 400, 300), edge(2, 520, 920, 200), edge(3, 0, 50, 50)]);
        assert_eq!(m.list.len(), 3);
        assert!(!m.list[0].bare());
        assert!(m.list[1].bare());
        assert!(m.list[2].bare());
        // Kapanan pencere bırakılır; sınırın üstü kesilir.
        m.sync(vec![edge(3, 0, 50, 50)]);
        assert_eq!(m.list.len(), 1);
        m.sync((0..40).map(|k| edge(k, 0, 100, 100)).collect());
        assert_eq!(m.list.len(), MAX_LEDGES);
    }

    #[test]
    fn gorunur_aralik_degisince_yeniden_cizilir() {
        let mut m = Ledges::new(1.0, Rng::new(5));
        m.sync(vec![edge(1, 0, 400, 300)]);
        m.list[0].dirty = false;
        m.sync(vec![edge(1, 0, 400, 300)]);
        assert!(!m.list[0].dirty);
        m.sync(vec![Edge { spans: vec![(0, 200)], ..edge(1, 0, 400, 300) }]);
        assert!(m.list[0].dirty);
    }

    #[test]
    fn bozuk_zaman_adimi_zararsiz() {
        let mut m = Ledges::new(1.5, Rng::new(9));
        m.sync(vec![edge(1, 0, 10, 300), edge(2, 0, 1, 300)]);
        m.grow(f32::NAN, Some(0.5), false);
        m.grow(-1.0, Some(0.5), false);
        m.grow(100.0, Some(0.5), true);
        m.grow(0.1, None, true);
        assert!(m.list.iter().all(|l| l.h.iter().all(|h| h.is_finite() && *h >= 0.0)));
    }
}
