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
#[derive(Clone, Debug, PartialEq)]
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
    /// Kalınlık çarpanı (dock'ta kar daha kalın).
    pub depth: f32,
    /// Tepedeki pürüzün genliği (0..=0.5; alt kenar birikintisinde dalgalı).
    pub rough: f32,
    /// Bir noktada yığılan tepeciğin genliği (0 = yok).
    pub mound: f32,
    /// Pencerenin ekrana kırpılmamış sol kenarı ve genişliği: ekranın dışına
    /// taşan pencere taşınırken kırpılmış boyu değişse de aynı yüzey sayılır.
    pub left: i32,
    pub full: i32,
}

/// Pencere taşınırken bu kadar piksellik yol karın tamamını dökerdi (ölçek 1);
/// [`KEEP_MIN`] yüzünden hiçbir zaman tamamı dökülmez.
const SHED_PX: f32 = 700.0;
/// Taşınan pencerede en az kalan kar payı.
const KEEP_MIN: f32 = 0.4;

/// Pencere üstleri için olağan pürüz.
pub const ROUGH: f32 = 0.16;

/// Pil çıkıntısının yüzeyi: boyu değişince karı sıfırlanmaz (esner), kaybolunca
/// karı saklanır ve geri gelince yerine konur.
pub const NUB_KEY: isize = -3;

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
    /// Taşınırken kalan pay (1 = hiç dökülmedi). Kar en çok [`KEEP_MIN`]'e
    /// dek dökülür; pencere durunca yavaşça 1'e döner.
    kept: f32,
}

impl Ledge {
    fn new(edge: Edge, s: f32, seed: f32) -> Self {
        let w = (edge.x1 - edge.x0).max(1) as f32;
        let col_w = (w / MAX_COLS as f32).max(3.0 * s);
        let n = ((w / col_w).ceil() as usize + 1).min(MAX_COLS + 1);
        Self { edge, h: vec![0.0; n], col_w, pending: 0.0, dirty: true, seed, kept: 1.0 }
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
        let r = self.edge.rough.clamp(0.0, 0.5);
        let bumps = 1.0 - r + r * (0.6 * (k * 0.9 + self.seed).sin() + 0.4 * (k * 0.31 + self.seed * 2.3).sin());
        // Tepecik: kenarın tohuma bağlı bir yerinde yumuşak yığın.
        let w = (self.edge.x1 - self.edge.x0).max(1) as f32;
        let c = self.edge.x0 as f32 + w * (0.2 + 0.6 * (self.seed / std::f32::consts::TAU).clamp(0.0, 1.0));
        let sigma = (0.1 * w).max(20.0);
        let mound = 1.0 + self.edge.mound.max(0.0) * (-((x - c) / sigma).powi(2)).exp();
        (max_h * self.edge.depth.max(0.0) * ends * bumps * mound).min(self.edge.room.max(0) as f32)
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

/// Taşınan yüzeyden dökülen kar (ekran koordinatları, dökülmeden önceki yer).
#[derive(Clone, Copy, Debug)]
pub struct Shed {
    pub x0: f32,
    pub x1: f32,
    pub y: f32,
    /// Dökülmeden önceki en kalın yer.
    pub h: f32,
    /// Dökülen pay (0..1).
    pub frac: f32,
    /// Pencerenin yatay yer değişimi.
    pub dx: f32,
}

/// Tüm yüzeylerin birikintileri.
pub struct Ledges {
    pub list: Vec<Ledge>,
    s: f32,
    rng: Rng,
    lost: Vec<Lost>,
    /// Kaybolan çıkıntının saklanan karı.
    parked: Vec<Ledge>,
}

/// Görünmez olan (küçültülen, kapanan) bir yüzeyin karı: nereden ve ne kadar (px²).
#[derive(Clone, Copy, Debug)]
pub struct Lost {
    pub x0: f32,
    pub x1: f32,
    pub y: f32,
    pub vol: f32,
}

/// Şiddet (0..=1) için en yüksek kar kalınlığı (ölçek 1'de piksel).
pub fn max_height(level: f32, s: f32) -> f32 {
    (4.0 + 7.0 * level.clamp(0.0, 1.0)) * s
}

impl Ledges {
    pub fn new(s: f32, rng: Rng) -> Self {
        Self { list: Vec::new(), s: s.max(0.5), rng, lost: Vec::new(), parked: Vec::new() }
    }

    /// Toplayıcıdan gelen kenarlarla eşler. Taşınan yüzeyin karı onunla gider,
    /// yalnızca bir kısmı (yol ve hızla orantılı) dökülür; boyu değişenin karı
    /// düşer; artık görünmeyen yüzey bırakılır. Taşınan yüzeylerin döktüğü karı döndürür
    /// (dökülmeyen taşıma `frac == 0` kaydıyla bildirilir).
    pub fn sync(&mut self, mut edges: Vec<Edge>) -> Vec<Shed> {
        edges.truncate(MAX_LEDGES);
        let mut old = std::mem::take(&mut self.list);
        // Saklanan kar, yüzeyi geri gelince yerine konur.
        for p in std::mem::take(&mut self.parked) {
            if old.iter().any(|l| l.edge.key == p.edge.key) {
                continue;
            }
            if edges.iter().any(|e| e.key == p.edge.key) {
                old.push(p);
            } else {
                self.parked.push(p);
            }
        }
        let mut shed = Vec::new();
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
                // Yalnızca taşındı (boyu aynı): kar pencereyle gider, bir kısmı dökülür.
                Some(mut l) if l.edge.full == e.full => {
                    let d = (((e.left - l.edge.left).pow(2) + (e.y - l.edge.y).pow(2)) as f32).sqrt();
                    if (l.edge.x1 - l.edge.x0) != (e.x1 - e.x0) {
                        // Ekran kenarında kırpılan kısım değişti: kar pencereye göre yeniden dağılır.
                        let mut n = Ledge::new(e.clone(), self.s, l.seed);
                        let shift = (e.left - l.edge.left) as f32;
                        let (a, b) = (l.edge.x0 as f32, l.edge.x1 as f32);
                        for i in 0..n.h.len() {
                            let x = n.col_x(i) - shift;
                            n.h[i] = if x >= a && x <= b { l.height_at(x) } else { 0.0 };
                        }
                        n.kept = l.kept;
                        // Dökülen kar eski yerinden düşer (aşağıda `e` ile değişir).
                        n.edge = l.edge.clone();
                        l = n;
                    }
                    let target = (l.kept * (1.0 - d / (SHED_PX * self.s))).max(KEEP_MIN);
                    let top = l.h.iter().copied().fold(0.0f32, f32::max);
                    let mut frac = 0.0;
                    if target < l.kept {
                        let f = target / l.kept;
                        frac = 1.0 - f;
                        l.h.iter_mut().for_each(|h| *h *= f);
                        l.kept = target;
                    }
                    shed.push(Shed { x0: l.edge.x0 as f32, x1: l.edge.x1 as f32, y: l.edge.y as f32, h: top, frac, dx: (e.left - l.edge.left) as f32 });
                    l.edge = e;
                    l.dirty = true;
                    l
                }
                // Çıkıntı büyüdü/küçüldü: kar sıfırlanmaz, yeni boya yayılır.
                Some(l) if l.edge.key == NUB_KEY => {
                    let mut n = Ledge::new(e, self.s, l.seed);
                    let ow = (l.edge.x1 - l.edge.x0).max(1) as f32;
                    let nw = (n.edge.x1 - n.edge.x0).max(1) as f32;
                    for i in 0..n.h.len() {
                        let u = ((n.col_x(i) - n.edge.x0 as f32) / nw).clamp(0.0, 1.0);
                        n.h[i] = l.height_at(l.edge.x0 as f32 + u * ow);
                    }
                    n.kept = l.kept;
                    n
                }
                // Boyu değişti ya da yeni: kar sıfırdan birikir.
                _ => {
                    let seed = self.rng.range(0.0, 6.28);
                    Ledge::new(e, self.s, seed)
                }
            };
            self.list.push(ledge);
        }
        // Artık görünmeyen pencere yüzeylerinin karı (dock ve alt birikinti hariç).
        for l in old.iter().filter(|l| l.edge.key >= 0) {
            let vol: f32 = l.h.iter().sum::<f32>() * l.col_w;
            if vol > 20.0 * self.s * self.s {
                self.lost.push(Lost { x0: l.edge.x0 as f32, x1: l.edge.x1 as f32, y: l.edge.y as f32, vol });
            }
        }
        self.lost.truncate(8);
        // Kaybolan çıkıntının karı saklanır (panel kapanınca geri gelir).
        self.parked.extend(old.into_iter().filter(|l| l.edge.key == NUB_KEY && !l.bare()));
        shed
    }

    /// Yüzey silkelendi: karın `1 - keep` payı düşer, kalanı yerinde kalır.
    /// Düşen kar (düşen tanelerle gösterilmek üzere) döner.
    pub fn shake(&mut self, key: isize, keep: f32) -> Option<Shed> {
        let keep = keep.clamp(0.0, 1.0);
        let l = self.list.iter_mut().find(|l| l.edge.key == key)?;
        let top = l.h.iter().copied().fold(0.0f32, f32::max);
        if top < 0.5 {
            return None;
        }
        l.h.iter_mut().for_each(|h| *h *= keep);
        l.dirty = true;
        Some(Shed { x0: l.edge.x0 as f32, x1: l.edge.x1 as f32, y: l.edge.y as f32, h: top, frac: 1.0 - keep, dx: 0.0 })
    }

    /// `snow`: kar yağıyorsa şiddet (0..=1); `None` ise erir. `melt_fast`: yağmur.
    pub fn grow(&mut self, dt: f32, snow: Option<f32>, melt_fast: bool) {
        let dt = if dt.is_finite() { dt.clamp(0.0, 0.5) } else { 0.0 };
        if dt == 0.0 {
            return;
        }
        let s = self.s;
        for l in &mut self.list {
            // Durunca karı yeniden "yapışır": sonraki taşımada yine bir kısmı dökülür.
            l.kept = (l.kept + 0.6 * dt).min(1.0);
            let mut moved = 0.0f32;
            match snow {
                Some(level) => {
                    let max_h = max_height(level, s);
                    // Yoğun karda ~1 dk, hafifte ~3 dk'da dolar (kalın yüzeyler de aynı sürede).
                    let peak = l.edge.depth.max(0.0) * (1.0 + l.edge.mound.max(0.0));
                    let rate = max_h * peak / (50.0 + 130.0 * (1.0 - level.clamp(0.0, 1.0)));
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

    /// Kürek geçti: `key` yüzeyinde `[a, b]` ekran aralığındaki kar temizlenir
    /// (sonra yeniden birikir).
    pub fn clear(&mut self, key: isize, a: f32, b: f32) {
        let Some(l) = self.list.iter_mut().find(|l| l.edge.key == key) else { return };
        let mut any = false;
        for i in 0..l.h.len() {
            let x = l.col_x(i);
            if x >= a && x <= b && l.h[i] > 0.0 {
                l.h[i] = 0.0;
                any = true;
            }
        }
        if any {
            l.dirty = true;
        }
    }

    /// Kartopu karı yedi: `[a, b]` aralığındaki sütunlar en çok `dh` kadar incelir.
    pub fn take(&mut self, key: isize, a: f32, b: f32, dh: f32) {
        let Some(l) = self.list.iter_mut().find(|l| l.edge.key == key) else { return };
        let mut any = false;
        for i in 0..l.h.len() {
            let x = l.col_x(i);
            if x >= a && x <= b && l.h[i] > 0.0 {
                l.h[i] = (l.h[i] - dh).max(0.0);
                any = true;
            }
        }
        if any {
            l.dirty = true;
        }
    }

    /// Kaybolan yüzeylerin (küçültülen/kapanan pencere) karı; bir kez okunur.
    pub fn take_lost(&mut self) -> Vec<Lost> {
        std::mem::take(&mut self.lost)
    }

    /// Dökülen kar `key` yüzeyine, `cx` çevresine yayılarak eklenir; sınırı
    /// aşabilir (fazlası yavaşça çöker), ama kule olmasın diye tavanı vardır.
    /// Eklenen hacmi döndürür.
    pub fn deposit(&mut self, key: isize, cx: f32, spread: f32, vol: f32) -> f32 {
        let s = self.s;
        let Some(l) = self.list.iter_mut().find(|l| l.edge.key == key) else { return 0.0 };
        let (x0, x1) = (l.edge.x0 as f32, l.edge.x1 as f32);
        if x1 - x0 < 4.0 || !(vol > 0.0) {
            return 0.0;
        }
        let cx = cx.clamp(x0, x1);
        let sig = spread.clamp(24.0 * s, (x1 - x0) * 0.5);
        let w: Vec<f32> = (0..l.h.len()).map(|i| (-((l.col_x(i) - cx) / sig).powi(2)).exp()).collect();
        let total: f32 = w.iter().sum::<f32>() * l.col_w;
        if total <= 0.0 {
            return 0.0;
        }
        let cap = 3.0 * max_height(1.0, s);
        let mut added = 0.0;
        for i in 0..l.h.len() {
            let old = l.h[i];
            l.h[i] = (old + vol * w[i] / total).min(cap.max(old));
            added += (l.h[i] - old) * l.col_w;
        }
        l.smooth(0.5);
        l.dirty = true;
        added
    }

    /// `key` yüzeyinin dolu oranı (ortalama yükseklik / sınır), yoksa 0.
    pub fn fullness(&self, key: isize, level: f32) -> f32 {
        let Some(l) = self.list.iter().find(|l| l.edge.key == key) else { return 0.0 };
        let max_h = max_height(level, self.s);
        let (mut h, mut lim) = (0.0, 0.0);
        for i in 0..l.h.len() {
            h += l.h[i];
            lim += l.limit(i, max_h);
        }
        if lim > 0.0 { h / lim } else { 0.0 }
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
        Edge { key, x0, x1, y, room: 40, taper: 8, spans: vec![(x0, x1)], depth: 1.0, rough: ROUGH, mound: 0.0, left: x0, full: x1 - x0 }
    }

    #[test]
    fn cikinti_silkelenir_saklanir_ve_esner() {
        let mut m = Ledges::new(1.0, Rng::new(3));
        m.sync(vec![edge(NUB_KEY, 100, 146, 50)]);
        m.settle(1.0);
        let vol = |m: &Ledges| m.list.iter().find(|l| l.edge.key == NUB_KEY).map_or(0.0, |l| l.h.iter().sum::<f32>());
        let full = vol(&m);
        assert!(full > 1.0);
        // Silkelenince dörtte biri kalır, düşen pay bildirilir.
        let d = m.shake(NUB_KEY, 0.25).unwrap();
        assert!((d.frac - 0.75).abs() < 1e-4 && d.h > 0.5);
        let kept = vol(&m);
        assert!((kept - 0.25 * full).abs() < 0.01 * full);
        // Panel açıkken yüzey yok: kar saklanır, geri gelince (büyümüş olsa da) yerinde.
        m.sync(vec![]);
        assert!(m.list.is_empty());
        m.sync(vec![edge(NUB_KEY, 93, 153, 44)]);
        let back = vol(&m);
        assert!(back > 0.5 * kept, "{back} {kept}");
        // Boyu değişse de kar sıfırlanmaz.
        m.sync(vec![edge(NUB_KEY, 100, 146, 50)]);
        assert!(vol(&m) > 0.5 * kept);
        // Karsız yüzey bir şey dökmez.
        let mut e = Ledges::new(1.0, Rng::new(4));
        e.sync(vec![edge(NUB_KEY, 0, 40, 10)]);
        assert!(e.shake(NUB_KEY, 0.25).is_none());
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
        let before = m.list[1].h.iter().sum::<f32>();
        let shed = m.sync(vec![edge(1, 1, 400, 300), edge(2, 520, 920, 200), edge(3, 0, 50, 50)]);
        assert_eq!(m.list.len(), 3);
        assert!(!m.list[0].bare());
        // Taşınan pencerenin karı onunla gider, yalnızca bir kısmı dökülür.
        assert!(!m.list[1].bare());
        let after = m.list[1].h.iter().sum::<f32>();
        assert!(after < before && after > before * 0.9, "{before} {after}");
        assert_eq!(shed.len(), 1);
        assert!(shed[0].frac > 0.0 && shed[0].frac < 0.1 && shed[0].dx == 20.0);
        // Uzun sürükleme bile karın hepsini döktürmez.
        for k in 1..60 {
            m.sync(vec![edge(1, 1, 400, 300), edge(2, 520 + k * 30, 920 + k * 30, 200), edge(3, 0, 50, 50)]);
        }
        let rest = m.list[1].h.iter().sum::<f32>();
        assert!(rest > before * (KEEP_MIN - 0.01) && rest < before * 0.5, "{before} {rest}");
        assert!(m.list[2].bare());
        // Kapanan pencere bırakılır; sınırın üstü kesilir.
        m.sync(vec![edge(3, 0, 50, 50)]);
        assert_eq!(m.list.len(), 1);
        m.sync((0..40).map(|k| edge(k, 0, 100, 100)).collect());
        assert_eq!(m.list.len(), MAX_LEDGES);
    }

    #[test]
    fn ekran_disina_tasan_pencere_karini_korur() {
        let mut m = Ledges::new(1.0, Rng::new(9));
        // 1700..2100 penceresi ekranda 1920'de kırpılmış.
        let clipped = Edge { left: 1700, full: 400, ..edge(1, 1700, 1920, 300) };
        m.sync(vec![clipped]);
        m.settle(1.0);
        assert!(!m.list[0].bare());
        // Sola çekildi, artık tamamı ekranda: kar gider, yeni görünen kısım boş.
        let shed = m.sync(vec![Edge { left: 1600, full: 400, ..edge(1, 1600, 2000, 300) }]);
        assert_eq!(shed.len(), 1);
        let l = &m.list[0];
        assert!(l.height_at(1700.0) > 0.5 && l.height_at(1990.0) < 0.01, "{} {}", l.height_at(1700.0), l.height_at(1990.0));
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
    fn kaybolan_pencerenin_kari_doker() {
        let mut m = Ledges::new(1.0, Rng::new(5));
        let win = edge(7, 100, 600, 300);
        let drift = edge(-2, 0, 1920, 1080);
        m.sync(vec![win.clone(), drift.clone()]);
        m.settle(1.0);
        assert!(m.take_lost().is_empty());
        // Pencere küçültüldü: kar kayıp listesine düşer, alt birikintiye eklenir.
        m.sync(vec![drift]);
        let lost = m.take_lost();
        assert_eq!(lost.len(), 1);
        assert!(lost[0].vol > 100.0 && lost[0].x0 == 100.0);
        let before = m.list[0].height_at(350.0);
        let added = m.deposit(-2, 350.0, 150.0, lost[0].vol);
        assert!(added > 0.0 && m.list[0].height_at(350.0) > before);
        assert!(m.list[0].h.iter().all(|h| *h <= 3.0 * max_height(1.0, 1.0) + 0.01));
        // Kartopu karı yer.
        let h0 = m.list[0].height_at(350.0);
        m.take(-2, 340.0, 360.0, 2.0);
        assert!(m.list[0].height_at(350.0) < h0);
        assert_eq!(m.deposit(99, 0.0, 10.0, 100.0), 0.0);
    }

    #[test]
    fn tepecik_ve_kurek() {
        let mut m = Ledges::new(1.0, Rng::new(11));
        let e = edge(1, 0, 1000, 500);
        m.sync(vec![e.clone(), Edge { key: 2, depth: 1.5, mound: 0.9, ..e }]);
        m.settle(1.0);
        let top = |l: &Ledge| l.h.iter().cloned().fold(0.0f32, f32::max);
        let (flat, peak) = (top(&m.list[0]), top(&m.list[1]));
        assert!(peak > flat * 2.0, "{flat} {peak}");
        assert!(peak <= 40.0 + 0.01);
        assert!(m.fullness(2, 1.0) > 0.9);
        m.clear(2, 300.0, 1000.0);
        assert!(m.list[1].height_at(600.0) == 0.0 && m.list[1].height_at(100.0) > 0.0);
        assert!(m.list[1].dirty);
        assert!(m.fullness(2, 1.0) < 0.5);
        assert_eq!(m.fullness(9, 1.0), 0.0);
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
