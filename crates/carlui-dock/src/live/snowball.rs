//! Kar topları ve kardan adam: saf model (çizim yok, Windows yok; test edilir).
//!
//! Birikmiş kara basınca bir top avuçlanır (`scoop`), sürüklenir (`drag`),
//! bırakılınca (`release`) yerçekimiyle düşer, bir zemine (dock'un üstü ya
//! da ekranın alt birikintisi) iner, biraz yuvarlanıp durur. İmleç duran
//! topları iter (`push`); kenardan taşan top aşağı düşer. Duran bir topun
//! tam üstüne düşen top onun üstüne oturur; üç top üst üste gelince kardan
//! adam olur. Kardan adama tıklamak onu yok eder; küreme aracı alt
//! birikintideki topları/adamları temizler (`clear`).
//!
//! Koordinatlar ekran pikselidir (y aşağı doğru artar). Zeminler her karede
//! çağıran tarafından verilir; zemini kaybolan (ör. dock gizlendi) toplar
//! olduğu yerde donar.

/// Bir zemin: `x0..x1` aralığında, üst yüzeyi `y` (karın tabanı).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Floor {
    pub key: isize,
    pub x0: f32,
    pub x1: f32,
    pub y: f32,
    /// Kenarlar duvar mı (ekran kenarı: top seker, düşmez).
    pub wall: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    /// İmleçte.
    Held,
    /// Havada.
    Flying,
    /// Zeminde yuvarlanıyor.
    Rolling(isize),
    /// Zeminde duruyor.
    Resting(isize),
    /// Başka bir topun (id) üstünde.
    Stacked(u32),
}

#[derive(Clone, Copy, Debug)]
pub struct Ball {
    pub id: u32,
    /// Merkez.
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub r: f32,
    pub state: State,
}

impl Ball {
    pub fn floor(&self) -> Option<isize> {
        match self.state {
            State::Rolling(k) | State::Resting(k) => Some(k),
            _ => None,
        }
    }

    pub fn moving(&self) -> bool {
        matches!(self.state, State::Held | State::Flying | State::Rolling(_))
    }

    fn contains(&self, x: f32, y: f32, pad: f32) -> bool {
        let (dx, dy) = (x - self.x, y - self.y);
        dx * dx + dy * dy <= (self.r + pad) * (self.r + pad)
    }
}

/// Kardan adam: alttan üste üç topun yarıçapı; `y` zeminin yüzeyi.
#[derive(Clone, Copy, Debug)]
pub struct Man {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub key: isize,
    pub rs: [f32; 3],
}

impl Man {
    /// Topların merkezleri (alttan üste).
    pub fn centers(&self) -> [(f32, f32); 3] {
        let [a, b, c] = self.rs;
        let y0 = self.y - a;
        let y1 = y0 - (a + b) * OVERLAP;
        let y2 = y1 - (b + c) * OVERLAP;
        [(self.x, y0), (self.x, y1), (self.x, y2)]
    }

    pub fn height(&self) -> f32 {
        self.y - (self.centers()[2].1 - self.rs[2])
    }

    fn contains(&self, x: f32, y: f32) -> bool {
        self.centers().iter().zip(self.rs).any(|(&(cx, cy), r)| {
            let (dx, dy) = (x - cx, y - cy);
            dx * dx + dy * dy <= r * r * 1.1
        })
    }
}

/// Basışın sonucu.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Press {
    /// Duran bir top tutuldu.
    Ball(u32),
    /// Kardan adam yıkıldı.
    Man,
    /// Burada top/adam yok (çağıran kardan top avuçlayabilir).
    Nothing,
}

/// Üst üste duran topların iç içe geçme oranı.
const OVERLAP: f32 = 0.82;
/// En fazla top (fazlası gelince en eski duran top gider).
pub const MAX_BALLS: usize = 12;
/// En fazla kardan adam.
pub const MAX_MEN: usize = 4;
/// Kardan adam için üst üste top sayısı.
const STACK: usize = 3;
/// Yığına çekilme mesafesi (iki yarıçap toplamına oranla).
const MAGNET: f32 = 0.7;
/// Topun büyüyebileceği en büyük yarıçap (s ile çarpılır).
pub const GROW_MAX: f32 = 26.0;

pub struct Snow {
    pub balls: Vec<Ball>,
    pub men: Vec<Man>,
    s: f32,
    next_id: u32,
    /// Tutulan topun son konumu ve tahmini hızı (fırlatma için).
    held_from: Option<(f32, f32)>,
    held_v: (f32, f32),
    /// Bu y'nin altına düşen top yok olur.
    limit: f32,
}

impl Snow {
    pub fn new(s: f32, limit: f32) -> Self {
        Self { balls: Vec::new(), men: Vec::new(), s: s.max(0.5), next_id: 1, held_from: None, held_v: (0.0, 0.0), limit }
    }

    pub fn is_empty(&self) -> bool {
        self.balls.is_empty() && self.men.is_empty()
    }

    /// Kare gerekiyor mu (hareket eden top var mı)?
    pub fn busy(&self) -> bool {
        self.balls.iter().any(Ball::moving)
    }

    pub fn holding(&self) -> bool {
        self.balls.iter().any(|b| b.state == State::Held)
    }

    fn id(&mut self) -> u32 {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.next_id
    }

    fn get(&self, id: u32) -> Option<&Ball> {
        self.balls.iter().find(|b| b.id == id)
    }

    /// Üstünde başka top olan top.
    fn carries(&self, id: u32) -> bool {
        self.balls.iter().any(|b| b.state == State::Stacked(id))
    }

    /// `(x, y)`'de bir basış: önce kardan adam, sonra duran/yuvarlanan top.
    pub fn press(&mut self, x: f32, y: f32) -> Press {
        if let Some(i) = self.men.iter().rposition(|m| m.contains(x, y)) {
            self.men.remove(i);
            return Press::Man;
        }
        let pad = 2.0 * self.s;
        // En üstteki (en son eklenen) top önce; altında top olan tutulmaz.
        let pick = self.balls.iter().rev().find(|b| b.state != State::Held && b.contains(x, y, pad) && !self.carries(b.id)).map(|b| b.id);
        match pick {
            Some(id) => {
                if let Some(b) = self.balls.iter_mut().find(|b| b.id == id) {
                    b.state = State::Held;
                    b.vx = 0.0;
                    b.vy = 0.0;
                    b.x = x;
                    b.y = y;
                }
                self.held_from = Some((x, y));
                self.held_v = (0.0, 0.0);
                Press::Ball(id)
            }
            None => Press::Nothing,
        }
    }

    /// Kardan yeni top avuçlar ve tutar.
    pub fn scoop(&mut self, x: f32, y: f32, r: f32) -> u32 {
        self.balls.retain(|b| b.state != State::Held);
        if self.balls.len() >= MAX_BALLS {
            // En eski serbest top gider; yoksa en eskisi (üstündekiler düşer).
            let free = self.balls.iter().position(|b| !self.carries(b.id) && !matches!(b.state, State::Stacked(_)));
            let i = free.unwrap_or(0);
            self.balls.remove(i);
        }
        let id = self.id();
        self.balls.push(Ball { id, x, y, vx: 0.0, vy: 0.0, r, state: State::Held });
        self.held_from = Some((x, y));
        self.held_v = (0.0, 0.0);
        id
    }

    /// Tutulan top imleçle gelir.
    pub fn drag(&mut self, x: f32, y: f32) {
        if let Some(b) = self.balls.iter_mut().find(|b| b.state == State::Held) {
            b.x = x;
            b.y = y;
        }
    }

    /// Bırakış: tutulan top son sürükleme hızıyla fırlar.
    pub fn release(&mut self) {
        let cap = 1600.0 * self.s;
        let (vx, vy) = (self.held_v.0.clamp(-cap, cap), self.held_v.1.clamp(-cap, cap));
        for b in self.balls.iter_mut().filter(|b| b.state == State::Held) {
            b.state = State::Flying;
            b.vx = vx;
            b.vy = vy;
        }
        self.held_from = None;
        self.held_v = (0.0, 0.0);
    }

    /// İmleç `(x, y)`'de, son hareketten beri `dx` kadar yatay gitti: değdiği
    /// serbest topları iter. Üstünde top taşıyan top yerinden oynamaz;
    /// üstteki top itilirse düşer.
    pub fn push(&mut self, x: f32, y: f32, dx: f32) {
        if self.holding() {
            return;
        }
        let s = self.s;
        let anchors: Vec<u32> = self.balls.iter().filter(|b| self.carries(b.id)).map(|b| b.id).collect();
        for b in self.balls.iter_mut() {
            let free = matches!(b.state, State::Resting(_) | State::Rolling(_) | State::Stacked(_));
            if !free || anchors.contains(&b.id) || !b.contains(x, y, 1.0 * s) {
                continue;
            }
            let dir = if dx.abs() > 0.5 { dx.signum() } else if b.x >= x { 1.0 } else { -1.0 };
            let speed = (dx.abs() * 14.0).clamp(90.0 * s, 900.0 * s);
            b.x = x + dir * (b.r + 1.5 * s);
            b.vx = dir * speed;
            match b.state {
                State::Stacked(_) => {
                    b.state = State::Flying;
                    b.vy = -60.0 * s;
                }
                State::Resting(k) => b.state = State::Rolling(k),
                _ => {}
            }
        }
    }

    /// `key` zemininde `a..b` aralığındaki topları ve adamları siler (küreme).
    pub fn clear(&mut self, key: isize, a: f32, b: f32) {
        self.men.retain(|m| !(m.key == key && m.x >= a && m.x <= b));
        let gone: Vec<u32> = self.balls.iter().filter(|o| o.floor() == Some(key) && o.x >= a && o.x <= b).map(|o| o.id).collect();
        if !gone.is_empty() {
            self.balls.retain(|o| !gone.contains(&o.id));
        }
    }

    /// Hepsini siler.
    pub fn reset(&mut self) {
        self.balls.clear();
        self.men.clear();
        self.held_from = None;
    }

    /// Kar yağmıyorken toplar ve adamlar yavaşça erir.
    pub fn melt(&mut self, dt: f32) {
        let (k, min) = (0.35 * self.s * dt, 3.0 * self.s);
        for b in self.balls.iter_mut().filter(|b| b.state != State::Held) {
            b.r -= k;
        }
        self.balls.retain(|b| b.r >= min);
        for m in self.men.iter_mut() {
            for r in m.rs.iter_mut() {
                *r -= k;
            }
        }
        self.men.retain(|m| m.rs.iter().all(|&r| r >= min));
    }

    /// Fiziği `dt` saniye ilerletir. `floors`: şu an var olan zeminler;
    /// `surf(zemin, x)`: o x'te karın yüzeyi (y).
    /// Kartopu büyütme: kar üstünde tutulan ya da karda yuvarlanan top kar
    /// toplayıp büyür (tutulan top durunca da yavaşça, sürüklenince hızla).
    /// Dönen: yenen kar `(zemin, x0, x1, kalınlık)`; çağıran o karı azaltır.
    pub fn roll(&mut self, dt: f32, floors: &[Floor], surf: impl Fn(&Floor, f32) -> f32) -> Vec<(isize, f32, f32, f32)> {
        let dt = if dt.is_finite() { dt.clamp(0.0, 0.05) } else { 0.0 };
        let s = self.s;
        let max_r = GROW_MAX * s;
        let speed = (self.held_v.0 * self.held_v.0 + self.held_v.1 * self.held_v.1).sqrt();
        let mut eaten = Vec::new();
        for b in self.balls.iter_mut() {
            if b.r >= max_r || dt == 0.0 {
                continue;
            }
            let rate = match b.state {
                State::Held => 3.0 * s + speed.min(1500.0 * s) * 0.012,
                State::Rolling(_) => b.vx.abs() * 0.03,
                _ => continue,
            };
            let on = floors.iter().find(|f| {
                if b.x < f.x0 || b.x > f.x1 {
                    return false;
                }
                let top = surf(f, b.x);
                let depth = f.y - top;
                match b.state {
                    State::Rolling(k) => k == f.key && depth > 1.0 * s,
                    // Tutulan top karın içinde ya da hemen üstünde olmalı.
                    _ => depth > 1.0 * s && b.y + b.r >= top - 4.0 * s && b.y <= f.y + b.r,
                }
            });
            let Some(f) = on else { continue };
            let dr = (rate * dt).min(max_r - b.r);
            if dr <= 0.0 {
                continue;
            }
            if let State::Rolling(_) = b.state {
                // Büyüyen top zeminden kalkmaz: merkez yukarı kayar.
                b.y -= dr;
            }
            b.r += dr;
            let w = b.r * 0.7;
            eaten.push((f.key, b.x - w, b.x + w, dr * 0.9));
        }
        eaten
    }

    pub fn step(&mut self, dt: f32, floors: &[Floor], surf: impl Fn(&Floor, f32) -> f32) {
        let dt = dt.clamp(0.0, 0.05);
        let s = self.s;
        // Tutulan topun hızı (yumuşatılmış): fırlatma için.
        if let Some(i) = self.balls.iter().position(|b| b.state == State::Held) {
            let (x, y) = (self.balls[i].x, self.balls[i].y);
            if let (Some((px, py)), true) = (self.held_from, dt > 0.0) {
                let (vx, vy) = ((x - px) / dt, (y - py) / dt);
                // Kare hızından bağımsız yumuşatma (~25 ms): yüksek yenilemeli
                // ekranda fare olayı gelmeyen karelerde hız birden sıfırlanmasın.
                let a = 1.0 - (-dt / 0.025).exp();
                self.held_v = (self.held_v.0 + (vx - self.held_v.0) * a, self.held_v.1 + (vy - self.held_v.1) * a);
            }
            self.held_from = Some((x, y));
        }
        let find = |k: isize| floors.iter().find(|f| f.key == k);
        let g = 2400.0 * s;
        let n = self.balls.len();
        for i in 0..n {
            let b = self.balls[i];
            match b.state {
                State::Held => {}
                State::Flying => {
                    let (px, py) = (b.x, b.y);
                    let vy = b.vy + g * dt;
                    let (mut x, y) = (b.x + b.vx * dt, b.y + vy * dt);
                    let mut vx = b.vx;
                    // Ekran kenarları (duvarlı zemin) havadaki topu da sektirir.
                    for f in floors.iter().filter(|f| f.wall) {
                        if x - b.r < f.x0 {
                            x = f.x0 + b.r;
                            vx = vx.abs() * 0.5;
                        } else if x + b.r > f.x1 {
                            x = f.x1 - b.r;
                            vx = -vx.abs() * 0.5;
                        }
                    }
                    let mut nb = Ball { x, y, vx, vy, ..b };
                    // Bir yığının üstüne mi düştü?
                    if vy > 0.0 {
                        let top = self.balls.iter().filter(|t| t.id != b.id && matches!(t.state, State::Resting(_) | State::Rolling(_) | State::Stacked(_)) && !self.carries(t.id)).find(|t| {
                            // Üstüne oturunca alt kenarının olacağı çizgi.
                            let line = t.y - (t.r + b.r) * OVERLAP + b.r;
                            // Mıknatıs: merkezi hedefin üst yarısındaysa ve yatayda
                            // yeterince yakınsa tam üstüne çekilir (bırakış tam isabet istemez).
                            // Yandan hızla gelen (itilen, fırlatılan) top çekilmez.
                            let reach = if vx.abs() < 120.0 * s { (t.r + b.r) * MAGNET } else { t.r * 0.5 };
                            (x - t.x).abs() <= reach && y < t.y - t.r * 0.3 && y + b.r >= line
                        });
                        if let Some(t) = top {
                            let tid = t.id;
                            if let Some(t) = self.balls.iter_mut().find(|o| o.id == tid) {
                                // Yuvarlanan taban durur: üstüne top oturdu.
                                if let State::Rolling(k) = t.state {
                                    t.state = State::Resting(k);
                                    t.vx = 0.0;
                                }
                            }
                            let t = *self.balls.iter().find(|o| o.id == tid).unwrap();
                            nb.state = State::Stacked(t.id);
                            nb.x = t.x;
                            nb.y = t.y - (t.r + b.r) * OVERLAP;
                            nb.vx = 0.0;
                            nb.vy = 0.0;
                            self.balls[i] = nb;
                            continue;
                        }
                    }
                    // Zemine iniş.
                    let mut landed = false;
                    for f in floors {
                        if x < f.x0 || x > f.x1 {
                            continue;
                        }
                        let top = surf(f, x);
                        // Dock'un karından yapılıp hemen bırakılan top yüzeyin biraz
                        // altında doğar: merkezi yüzeye yakınsa yine üstüne oturur
                        // (yoksa dock'un içinden düşüp gider).
                        let above = py + b.r <= top + 0.5 || (!f.wall && py <= top + 12.0 * s);
                        if vy > 0.0 && above && y + b.r >= top {
                            nb.y = top - b.r;
                            if vy > 700.0 * s {
                                // Sert iniş: hafif sekme.
                                nb.vy = -vy * 0.22;
                                nb.vx *= 0.8;
                            } else {
                                nb.vy = 0.0;
                                nb.vx *= 0.75;
                                nb.state = if nb.vx.abs() < 4.0 * s { State::Resting(f.key) } else { State::Rolling(f.key) };
                            }
                            landed = true;
                            break;
                        }
                    }
                    let _ = (landed, px);
                    if nb.y - b.r > self.limit {
                        nb.r = 0.0; // aşağıda silinir
                    }
                    self.balls[i] = nb;
                }
                State::Rolling(k) | State::Resting(k) => {
                    let Some(f) = find(k) else { continue }; // zemin yok: donar
                    let mut nb = b;
                    if let State::Rolling(_) = b.state {
                        nb.x += b.vx * dt;
                        // Yuvarlanma sürtünmesi.
                        let v = b.vx.abs() * (-3.0 * dt).exp() - 140.0 * s * dt;
                        nb.vx = b.vx.signum() * v.max(0.0);
                        if nb.vx.abs() < 2.0 * s {
                            nb.vx = 0.0;
                            nb.state = State::Resting(k);
                        }
                    }
                    if f.wall && (nb.x - nb.r < f.x0 || nb.x + nb.r > f.x1) {
                        nb.x = nb.x.clamp(f.x0 + nb.r, f.x1 - nb.r);
                        nb.vx = -nb.vx * 0.5;
                        nb.y = surf(f, nb.x) - nb.r;
                    } else if nb.x < f.x0 || nb.x > f.x1 {
                        // Kenardan taştı: düşer.
                        nb.state = State::Flying;
                        nb.vy = 0.0;
                    } else {
                        nb.y = surf(f, nb.x) - nb.r;
                    }
                    self.balls[i] = nb;
                }
                State::Stacked(_) => {}
            }
        }
        self.balls.retain(|b| b.r > 0.0);
        self.collide();
        self.follow_stacks();
        self.build_men(floors, &surf);
    }

    /// Aynı zemindeki serbest toplar birbirinin içine girmez.
    fn collide(&mut self) {
        let n = self.balls.len();
        for i in 0..n {
            for j in i + 1..n {
                let (a, b) = (self.balls[i], self.balls[j]);
                let (Some(ka), Some(kb)) = (a.floor(), b.floor()) else { continue };
                if ka != kb {
                    continue;
                }
                let gap = a.r + b.r - (a.x - b.x).abs();
                if gap <= 0.0 {
                    continue;
                }
                let fa = self.carries(a.id);
                let fb = self.carries(b.id);
                if fa && fb {
                    continue;
                }
                let dir = if a.x <= b.x { -1.0 } else { 1.0 }; // a'nın kaçacağı yön
                let (mut na, mut nb) = (a, b);
                if fa {
                    nb.x -= dir * gap;
                    nb.vx = -nb.vx * 0.4;
                } else if fb {
                    na.x += dir * gap;
                    na.vx = -na.vx * 0.4;
                } else {
                    na.x += dir * gap / 2.0;
                    nb.x -= dir * gap / 2.0;
                    // Eşit kütle: hızlar değişir (biraz enerji kaybıyla).
                    let (va, vb) = (a.vx, b.vx);
                    na.vx = vb * 0.8;
                    nb.vx = va * 0.8;
                }
                for (o, k) in [(&mut na, ka), (&mut nb, kb)] {
                    if o.vx.abs() > 0.0 {
                        o.state = State::Rolling(k);
                    }
                }
                self.balls[i] = na;
                self.balls[j] = nb;
            }
        }
    }

    /// Üstteki toplar alttakini izler; alttaki gittiyse düşer.
    fn follow_stacks(&mut self) {
        // Yığın en fazla birkaç kat: birkaç geçiş yeter.
        for _ in 0..STACK + 1 {
            let mut changed = false;
            for i in 0..self.balls.len() {
                let State::Stacked(on) = self.balls[i].state else { continue };
                let below = self.get(on).copied().filter(|t| matches!(t.state, State::Resting(_) | State::Stacked(_)));
                let b = &mut self.balls[i];
                match below {
                    Some(t) => {
                        let (x, y) = (t.x, t.y - (t.r + b.r) * OVERLAP);
                        if (b.x, b.y) != (x, y) {
                            b.x = x;
                            b.y = y;
                            changed = true;
                        }
                    }
                    None => {
                        b.state = State::Flying;
                        b.vy = 0.0;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// Yığının tabanı (Resting) ve yüksekliği.
    fn stack_of(&self, top: u32) -> Option<(u32, Vec<u32>)> {
        let mut chain = vec![top];
        let mut cur = self.get(top)?;
        while let State::Stacked(on) = cur.state {
            cur = self.get(on)?;
            chain.push(cur.id);
            if chain.len() > STACK + 2 {
                return None;
            }
        }
        match cur.state {
            State::Resting(_) => Some((cur.id, chain)),
            _ => None,
        }
    }

    /// Üç top üst üste geldiyse kardan adam olur.
    fn build_men(&mut self, floors: &[Floor], surf: &impl Fn(&Floor, f32) -> f32) {
        let tops: Vec<u32> = self.balls.iter().filter(|b| matches!(b.state, State::Stacked(_)) && !self.carries(b.id)).map(|b| b.id).collect();
        for top in tops {
            let Some((base, chain)) = self.stack_of(top) else { continue };
            if chain.len() < STACK {
                continue;
            }
            let Some(bb) = self.get(base).copied() else { continue };
            let key = bb.floor().unwrap_or(0);
            let y = floors.iter().find(|f| f.key == key).map(|f| surf(f, bb.x)).unwrap_or(bb.y + bb.r);
            // Zincir üstten alta; adam alttan üste.
            let rs: Vec<f32> = chain.iter().rev().take(STACK).filter_map(|id| self.get(*id).map(|b| b.r)).collect();
            if rs.len() < STACK {
                continue;
            }
            self.balls.retain(|b| !chain.contains(&b.id));
            if self.men.len() >= MAX_MEN {
                self.men.remove(0);
            }
            let id = self.id();
            self.men.push(Man { id, x: bb.x, y, key, rs: [rs[0], rs[1], rs[2]] });
        }
    }

    /// Adamlar zeminleriyle birlikte kayar (dock yükseldi/indi); zemini yoksa donar.
    pub fn follow_floors(&mut self, floors: &[Floor], surf: impl Fn(&Floor, f32) -> f32) {
        for m in self.men.iter_mut() {
            if let Some(f) = floors.iter().find(|f| f.key == m.key) {
                if m.x >= f.x0 && m.x <= f.x1 {
                    m.y = surf(f, m.x);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOCK: isize = -1;
    const DRIFT: isize = -2;

    fn floors() -> Vec<Floor> {
        vec![Floor { key: DOCK, x0: 700.0, x1: 1200.0, y: 1000.0, wall: false }, Floor { key: DRIFT, x0: 0.0, x1: 1920.0, y: 1080.0, wall: true }]
    }

    fn flat(f: &Floor, _x: f32) -> f32 {
        f.y
    }

    fn run(snow: &mut Snow, secs: f32, fl: &[Floor]) {
        let mut t = 0.0;
        while t < secs {
            snow.step(1.0 / 60.0, fl, flat);
            t += 1.0 / 60.0;
        }
    }

    fn drop_at(snow: &mut Snow, x: f32, y: f32, r: f32) -> u32 {
        let id = snow.scoop(x, y, r);
        snow.step(1.0 / 60.0, &floors(), flat);
        snow.release();
        id
    }

    #[test]
    fn birakilan_top_dock_ustune_iner_ve_durur() {
        let mut snow = Snow::new(1.0, 1200.0);
        let id = drop_at(&mut snow, 900.0, 700.0, 10.0);
        run(&mut snow, 3.0, &floors());
        let b = snow.get(id).copied().unwrap();
        assert_eq!(b.state, State::Resting(DOCK));
        assert!((b.y - (1000.0 - 10.0)).abs() < 0.01, "{b:?}");
        assert!(!snow.busy());
    }

    #[test]
    fn dock_karindan_yapilip_birakilan_top_dockta_kalir() {
        let mut snow = Snow::new(1.0, 1200.0);
        // Dock karından (yüzeyin hemen altında) yapılıp fırlatmadan bırakıldı.
        let id = drop_at(&mut snow, 900.0, 1001.0, 10.0);
        run(&mut snow, 3.0, &floors());
        let b = snow.get(id).copied().unwrap();
        assert_eq!(b.state, State::Resting(DOCK), "{b:?}");
        assert!((b.y - 990.0).abs() < 0.01, "{b:?}");
    }

    #[test]
    fn dock_disina_birakilan_top_birikintiye_iner() {
        let mut snow = Snow::new(1.0, 1200.0);
        let id = drop_at(&mut snow, 300.0, 500.0, 10.0);
        run(&mut snow, 3.0, &floors());
        let b = snow.get(id).copied().unwrap();
        assert_eq!(b.state, State::Resting(DRIFT));
        assert!((b.y - 1070.0).abs() < 0.01);
    }

    #[test]
    fn firlatilan_top_yatay_hizla_gider_yuvarlanip_durur() {
        let mut snow = Snow::new(1.0, 1200.0);
        let id = snow.scoop(200.0, 900.0, 10.0);
        // Sağa doğru hızlı sürükleme.
        for k in 1..=5 {
            snow.drag(200.0 + 12.0 * k as f32, 900.0);
            snow.step(1.0 / 60.0, &floors(), flat);
        }
        snow.release();
        let x_rel = snow.get(id).unwrap().x;
        assert!(snow.get(id).unwrap().vx > 300.0);
        run(&mut snow, 6.0, &floors());
        let b = snow.get(id).copied().unwrap();
        assert!(b.x > x_rel + 50.0, "top ilerlemeli: {b:?}");
        assert!(matches!(b.state, State::Resting(_)), "sonunda durmalı: {b:?}");
    }

    #[test]
    fn sert_iniste_seker_ama_sonunda_oturur() {
        let mut snow = Snow::new(1.0, 1200.0);
        let id = drop_at(&mut snow, 300.0, 0.0, 10.0);
        let mut bounced = false;
        for _ in 0..240 {
            snow.step(1.0 / 60.0, &floors(), flat);
            if snow.get(id).unwrap().vy < 0.0 {
                bounced = true;
            }
        }
        assert!(bounced, "yüksekten düşen top sekmeli");
        assert_eq!(snow.get(id).unwrap().state, State::Resting(DRIFT));
    }

    #[test]
    fn imlec_topu_iter_ve_dock_kenarindan_dusurur() {
        let mut snow = Snow::new(1.0, 1200.0);
        let id = drop_at(&mut snow, 1180.0, 900.0, 10.0);
        run(&mut snow, 2.0, &floors());
        assert_eq!(snow.get(id).unwrap().state, State::Resting(DOCK));
        // Soldan sağa iterek.
        snow.push(1175.0, 990.0, 15.0);
        assert!(matches!(snow.get(id).unwrap().state, State::Rolling(DOCK)));
        run(&mut snow, 4.0, &floors());
        let b = snow.get(id).copied().unwrap();
        assert_eq!(b.state, State::Resting(DRIFT), "kenardan taşan top birikintiye düşmeli: {b:?}");
        assert!(b.x > 1200.0);
    }

    #[test]
    fn uzaktaki_imlec_topa_dokunmaz() {
        let mut snow = Snow::new(1.0, 1200.0);
        let id = drop_at(&mut snow, 900.0, 900.0, 10.0);
        run(&mut snow, 2.0, &floors());
        let before = snow.get(id).copied().unwrap();
        snow.push(950.0, 990.0, 20.0);
        snow.push(900.0, 900.0, 20.0);
        assert_eq!(snow.get(id).unwrap().state, before.state);
        assert_eq!(snow.get(id).unwrap().x, before.x);
    }

    #[test]
    fn toplar_ic_ice_girmez() {
        let mut snow = Snow::new(1.0, 1200.0);
        let a = drop_at(&mut snow, 400.0, 1000.0, 10.0);
        run(&mut snow, 2.0, &floors());
        // İkincisini yanına at ve üstüne doğru yuvarla.
        let b = drop_at(&mut snow, 360.0, 1000.0, 10.0);
        run(&mut snow, 1.0, &floors());
        snow.push(355.0, 1070.0, 20.0);
        run(&mut snow, 4.0, &floors());
        let (ba, bb) = (snow.get(a).copied().unwrap(), snow.get(b).copied().unwrap());
        assert!((ba.x - bb.x).abs() >= ba.r + bb.r - 0.5, "{ba:?} {bb:?}");
        assert!(ba.x > 400.0, "çarpılan top itilmeli: {ba:?}");
    }

    #[test]
    fn uc_top_ust_uste_kardan_adam_olur() {
        let mut snow = Snow::new(1.0, 1200.0);
        drop_at(&mut snow, 500.0, 900.0, 14.0);
        run(&mut snow, 2.0, &floors());
        drop_at(&mut snow, 503.0, 900.0, 12.0);
        run(&mut snow, 2.0, &floors());
        assert_eq!(snow.balls.len(), 2);
        assert!(matches!(snow.balls[1].state, State::Stacked(_)), "{:?}", snow.balls);
        assert!(snow.men.is_empty());
        drop_at(&mut snow, 498.0, 850.0, 10.0);
        run(&mut snow, 2.0, &floors());
        assert!(snow.balls.is_empty(), "toplar adama dönüşmeli: {:?}", snow.balls);
        assert_eq!(snow.men.len(), 1);
        let m = snow.men[0];
        assert_eq!(m.key, DRIFT);
        assert_eq!(m.rs, [14.0, 12.0, 10.0]);
        assert_eq!(m.x, 500.0);
        assert!((m.y - 1080.0).abs() < 0.01);
        // Gövde zeminden yukarı doğru dizilir.
        let c = m.centers();
        assert!(c[0].1 > c[1].1 && c[1].1 > c[2].1);
        assert!(!snow.busy());
    }

    #[test]
    fn dockta_da_kardan_adam_yapilir() {
        let mut snow = Snow::new(1.0, 1200.0);
        for _ in 0..3 {
            drop_at(&mut snow, 950.0, 800.0, 12.0);
            run(&mut snow, 2.0, &floors());
        }
        assert_eq!(snow.men.len(), 1);
        assert_eq!(snow.men[0].key, DOCK);
    }

    #[test]
    fn yandan_dusen_top_yigina_binmez() {
        let mut snow = Snow::new(1.0, 1200.0);
        drop_at(&mut snow, 500.0, 900.0, 12.0);
        run(&mut snow, 2.0, &floors());
        drop_at(&mut snow, 520.0, 900.0, 12.0);
        run(&mut snow, 3.0, &floors());
        assert!(snow.balls.iter().all(|b| matches!(b.state, State::Resting(DRIFT))), "{:?}", snow.balls);
    }

    #[test]
    fn biraz_kenara_birakilan_top_da_yigina_cekilir() {
        let mut snow = Snow::new(1.0, 1200.0);
        drop_at(&mut snow, 500.0, 900.0, 14.0);
        run(&mut snow, 2.0, &floors());
        // 12 px yanda, tabana değecek kadar alçaktan bırakılır.
        drop_at(&mut snow, 512.0, 1040.0, 12.0);
        run(&mut snow, 2.0, &floors());
        assert!(snow.balls.iter().any(|b| matches!(b.state, State::Stacked(_))), "{:?}", snow.balls);
        drop_at(&mut snow, 490.0, 1000.0, 10.0);
        run(&mut snow, 2.0, &floors());
        assert_eq!(snow.men.len(), 1, "{:?}", snow.balls);
    }

    #[test]
    fn yuvarlanan_topun_ustune_de_oturulur() {
        let mut snow = Snow::new(1.0, 1200.0);
        snow.balls.push(Ball { id: 50, x: 500.0, y: 1066.0, vx: 30.0, vy: 0.0, r: 14.0, state: State::Rolling(DRIFT) });
        drop_at(&mut snow, 502.0, 1010.0, 12.0);
        run(&mut snow, 1.0, &floors());
        assert!(snow.balls.iter().any(|b| matches!(b.state, State::Stacked(50))), "{:?}", snow.balls);
    }

    fn deep(f: &Floor, _x: f32) -> f32 {
        f.y - 10.0
    }

    #[test]
    fn karda_surulen_top_buyur_ve_kar_yer() {
        let mut snow = Snow::new(1.0, 1200.0);
        snow.scoop(300.0, 1068.0, 8.0);
        let fl = floors();
        let mut eaten = 0.0;
        for i in 0..120 {
            snow.drag(300.0 + i as f32 * 5.0, 1068.0);
            snow.step(1.0 / 60.0, &fl, deep);
            eaten += snow.roll(1.0 / 60.0, &fl, deep).iter().map(|e| e.3).sum::<f32>();
        }
        let r = snow.balls[0].r;
        assert!(r > 14.0 && r <= GROW_MAX, "{r}");
        assert!(eaten > 0.0);
        // Karın uzağında (havada) büyümez.
        snow.drag(300.0, 600.0);
        snow.step(1.0 / 60.0, &fl, deep);
        assert!(snow.roll(1.0, &fl, deep).is_empty());
        // Kar yoksa büyümez.
        snow.drag(300.0, 1068.0);
        assert!(snow.roll(1.0 / 60.0, &fl, flat).is_empty());
    }

    #[test]
    fn kardan_adama_tiklamak_onu_yok_eder() {
        let mut snow = Snow::new(1.0, 1200.0);
        snow.men.push(Man { id: 7, x: 500.0, y: 1080.0, key: DRIFT, rs: [14.0, 12.0, 10.0] });
        assert_eq!(snow.press(300.0, 1070.0), Press::Nothing);
        let head = snow.men[0].centers()[2];
        assert_eq!(snow.press(head.0, head.1), Press::Man);
        assert!(snow.men.is_empty());
    }

    #[test]
    fn duran_top_yeniden_tutulup_atilabilir() {
        let mut snow = Snow::new(1.0, 1200.0);
        let id = drop_at(&mut snow, 900.0, 900.0, 10.0);
        run(&mut snow, 2.0, &floors());
        assert_eq!(snow.press(901.0, 989.0), Press::Ball(id));
        assert!(snow.holding());
        snow.drag(300.0, 600.0);
        snow.step(1.0 / 60.0, &floors(), flat);
        snow.release();
        run(&mut snow, 3.0, &floors());
        assert_eq!(snow.get(id).unwrap().state, State::Resting(DRIFT));
    }

    #[test]
    fn yigin_tabani_tutulamaz_ve_itilemez() {
        let mut snow = Snow::new(1.0, 1200.0);
        let a = drop_at(&mut snow, 500.0, 900.0, 14.0);
        run(&mut snow, 2.0, &floors());
        drop_at(&mut snow, 500.0, 900.0, 12.0);
        run(&mut snow, 2.0, &floors());
        let base = snow.get(a).copied().unwrap();
        // Tabanın alt yarısına basmak üstteki topu değil hiçbir şeyi tutmaz.
        assert_eq!(snow.press(500.0, base.y + 8.0), Press::Nothing);
        snow.push(500.0, base.y + 8.0, 20.0);
        assert_eq!(snow.get(a).unwrap().x, 500.0);
    }

    #[test]
    fn ustteki_top_itilince_duser() {
        let mut snow = Snow::new(1.0, 1200.0);
        drop_at(&mut snow, 500.0, 900.0, 14.0);
        run(&mut snow, 2.0, &floors());
        let top = drop_at(&mut snow, 500.0, 900.0, 12.0);
        run(&mut snow, 2.0, &floors());
        let t = snow.get(top).copied().unwrap();
        snow.push(t.x - 5.0, t.y, 20.0);
        run(&mut snow, 3.0, &floors());
        assert!(matches!(snow.get(top).unwrap().state, State::Resting(DRIFT)));
    }

    #[test]
    fn kureme_birikintideki_toplari_ve_adamlari_siler() {
        let mut snow = Snow::new(1.0, 1200.0);
        drop_at(&mut snow, 300.0, 900.0, 10.0);
        drop_at(&mut snow, 950.0, 900.0, 10.0);
        run(&mut snow, 2.0, &floors());
        snow.men.push(Man { id: 9, x: 320.0, y: 1080.0, key: DRIFT, rs: [14.0, 12.0, 10.0] });
        snow.clear(DRIFT, 250.0, 400.0);
        assert!(snow.men.is_empty());
        assert_eq!(snow.balls.len(), 1, "dock'taki top kalmalı");
        assert_eq!(snow.balls[0].floor(), Some(DOCK));
    }

    #[test]
    fn zemin_kaybolunca_toplar_donar() {
        let mut snow = Snow::new(1.0, 1200.0);
        let id = drop_at(&mut snow, 900.0, 900.0, 10.0);
        run(&mut snow, 2.0, &floors());
        let before = snow.get(id).copied().unwrap();
        let only_drift = [floors()[1]];
        run(&mut snow, 2.0, &only_drift);
        let b = snow.get(id).copied().unwrap();
        assert_eq!(b.state, before.state);
        assert_eq!((b.x, b.y), (before.x, before.y));
    }

    #[test]
    fn zemin_kayinca_top_onunla_gelir() {
        let mut snow = Snow::new(1.0, 1200.0);
        let id = drop_at(&mut snow, 900.0, 900.0, 10.0);
        run(&mut snow, 2.0, &floors());
        let mut fl = floors();
        fl[0].y = 980.0;
        run(&mut snow, 0.1, &fl);
        assert!((snow.get(id).unwrap().y - 970.0).abs() < 0.01);
    }

    #[test]
    fn top_sayisi_sinirli() {
        let mut snow = Snow::new(1.0, 1200.0);
        for k in 0..(MAX_BALLS + 5) {
            drop_at(&mut snow, 50.0 + 60.0 * k as f32, 900.0, 10.0);
        }
        assert!(snow.balls.len() <= MAX_BALLS);
    }

    #[test]
    fn erime_hepsini_sonunda_yok_eder() {
        let mut snow = Snow::new(1.0, 1200.0);
        drop_at(&mut snow, 300.0, 900.0, 10.0);
        run(&mut snow, 2.0, &floors());
        snow.men.push(Man { id: 9, x: 600.0, y: 1080.0, key: DRIFT, rs: [14.0, 12.0, 10.0] });
        for _ in 0..200 {
            snow.melt(0.5);
        }
        assert!(snow.is_empty());
    }

    #[test]
    fn ekrandan_cikan_top_silinir() {
        let mut snow = Snow::new(1.0, 1200.0);
        drop_at(&mut snow, 300.0, 900.0, 10.0);
        run(&mut snow, 3.0, &[]);
        assert!(snow.balls.is_empty());
    }

    #[test]
    fn ekran_kenari_topu_sektirir_kaybetmez() {
        let mut snow = Snow::new(1.0, 1200.0);
        let id = snow.scoop(60.0, 900.0, 10.0);
        for k in 1..=4 {
            snow.drag(60.0 - 15.0 * k as f32, 900.0);
            snow.step(1.0 / 60.0, &floors(), flat);
        }
        snow.release();
        run(&mut snow, 6.0, &floors());
        let b = snow.get(id).copied().expect("top ekranda kalmalı");
        assert_eq!(b.state, State::Resting(DRIFT));
        assert!(b.x >= 10.0 && b.x <= 1910.0, "{b:?}");
    }

    #[test]
    fn tuhaf_dt_degerleri_bozmaz() {
        let mut snow = Snow::new(1.0, 1200.0);
        let id = drop_at(&mut snow, 900.0, 900.0, 10.0);
        snow.step(f32::NAN.max(0.0), &floors(), flat);
        snow.step(-1.0, &floors(), flat);
        snow.step(10.0, &floors(), flat);
        let b = snow.get(id).copied().unwrap();
        assert!(b.x.is_finite() && b.y.is_finite());
        run(&mut snow, 3.0, &floors());
        assert_eq!(snow.get(id).unwrap().state, State::Resting(DOCK));
    }
}
