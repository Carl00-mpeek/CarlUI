//! Piksel sanatı tuvali: her piksel ya tamamen saydam (`CLEAR`) ya da tam opak
//! bir renktir (yumuşatma yok). Fotoğraftan kesilmiş nesne burada alan
//! ortalamasıyla küçük bir ızgaraya indirilir, kırıntılar atılır, delikler
//! kapatılır ve 1 piksellik koyu kontur eklenir.

use crate::Image;

/// Saydam piksel.
pub const CLEAR: u32 = 0;

/// Opak renk: `0xFF_RR_GG_BB`.
pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    0xFF00_0000 | (r as u32) << 16 | (g as u32) << 8 | b as u32
}

/// Rengin (r, g, b) bileşenleri.
pub const fn parts(c: u32) -> (u8, u8, u8) {
    ((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sprite {
    pub w: usize,
    pub h: usize,
    /// Satırlar yukarıdan aşağı.
    pub px: Vec<u32>,
}

impl Sprite {
    pub fn new(w: usize, h: usize) -> Self {
        Self { w, h, px: vec![CLEAR; w * h] }
    }

    /// Dışarısı saydam sayılır.
    pub fn get(&self, x: i32, y: i32) -> u32 {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
            CLEAR
        } else {
            self.px[y as usize * self.w + x as usize]
        }
    }

    /// Dışarıya yazılanlar yok sayılır.
    pub fn set(&mut self, x: i32, y: i32, c: u32) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            self.px[y as usize * self.w + x as usize] = c;
        }
    }

    pub fn opaque_count(&self) -> usize {
        self.px.iter().filter(|&&c| c != CLEAR).count()
    }

    /// Opak piksellerin kutusu `(x0, y0, x1, y1)` (sonlar hariç).
    pub fn bbox(&self) -> Option<(usize, usize, usize, usize)> {
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        for y in 0..self.h {
            for x in 0..self.w {
                if self.px[y * self.w + x] != CLEAR {
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
                }
            }
        }
        (x0 != usize::MAX).then_some((x0, y0, x1, y1))
    }

    pub fn crop(&self, x0: usize, y0: usize, x1: usize, y1: usize) -> Sprite {
        let mut s = Sprite::new(x1 - x0, y1 - y0);
        for y in y0..y1 {
            s.px[(y - y0) * s.w..(y - y0 + 1) * s.w].copy_from_slice(&self.px[y * self.w + x0..y * self.w + x1]);
        }
        s
    }

    /// Opak kutuya kırpar; boşsa `None`.
    pub fn trimmed(&self) -> Option<Sprite> {
        self.bbox().map(|(x0, y0, x1, y1)| self.crop(x0, y0, x1, y1))
    }

    /// Yatay ayna.
    pub fn mirror(&self) -> Sprite {
        let mut s = self.clone();
        for row in s.px.chunks_exact_mut(self.w) {
            row.reverse();
        }
        s
    }

    /// Her yöne 1 piksel büyür; opak bir komşusu (4 yön) olan saydam
    /// pikseller `c` olur. Köşeler boyanmaz: piksel sanatındaki ince kontur.
    pub fn outline(&self, c: u32) -> Sprite {
        let mut s = Sprite::new(self.w + 2, self.h + 2);
        for y in 0..self.h {
            for x in 0..self.w {
                s.px[(y + 1) * s.w + x + 1] = self.px[y * self.w + x];
            }
        }
        let src = s.clone();
        for y in 0..s.h as i32 {
            for x in 0..s.w as i32 {
                if src.get(x, y) == CLEAR && [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| src.get(x + dx, y + dy) != CLEAR) {
                    s.set(x, y, c);
                }
            }
        }
        s
    }

    /// 4 yönde bağlı parçalar: her pikselin parça numarası (saydamlar `u32::MAX`)
    /// ve parça boyları.
    fn components(&self) -> (Vec<u32>, Vec<usize>) {
        let mut id = vec![u32::MAX; self.px.len()];
        let mut sizes = Vec::new();
        let mut stack = Vec::new();
        for start in 0..self.px.len() {
            if self.px[start] == CLEAR || id[start] != u32::MAX {
                continue;
            }
            let n = sizes.len() as u32;
            let mut size = 0;
            id[start] = n;
            stack.push(start);
            while let Some(i) = stack.pop() {
                size += 1;
                let (x, y) = ((i % self.w) as i32, (i / self.w) as i32);
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if self.get(nx, ny) != CLEAR {
                        let j = ny as usize * self.w + nx as usize;
                        if id[j] == u32::MAX {
                            id[j] = n;
                            stack.push(j);
                        }
                    }
                }
            }
            sizes.push(size);
        }
        (id, sizes)
    }

    /// Ana gövdeden kopuk küçük kırıntıları siler (en büyük parçanın
    /// %15'inden küçük olanlar); kulak/kuyruk gibi belirgin parçalar kalır.
    pub fn drop_specks(&mut self) {
        let (id, sizes) = self.components();
        let Some(&big) = sizes.iter().max() else { return };
        let keep = (big * 15 / 100).max(3);
        for (p, &n) in self.px.iter_mut().zip(&id) {
            if n != u32::MAX && sizes[n as usize] < keep {
                *p = CLEAR;
            }
        }
    }

    /// Önündeki dal/tel gibi bir engel kesilince gövde parçalara bölünür ve her
    /// parça ayrı konturlanır. İki *farklı* parçayı ayıran en çok 2 piksellik
    /// boşluklar (yatay, dikey, çapraz) iki yanın ortalama rengiyle doldurulur.
    /// Aynı parçanın iki kolu (bacak arası gibi) birleştirilmez; tek piksellik
    /// kırıntılar köprülenmez (sonra `drop_specks` siler).
    pub fn bridge_gaps(&mut self) {
        let (id, sizes) = self.components();
        let src = self.clone();
        let at = |x: i32, y: i32| -> Option<(u32, u32)> {
            let c = src.get(x, y);
            (c != CLEAR).then(|| (id[y as usize * src.w + x as usize], c))
        };
        // Bir yönde en çok 2 adım içindeki ilk opak piksel.
        let near = |x: i32, y: i32, dx: i32, dy: i32| (1..=2).find_map(|k| at(x + dx * k, y + dy * k));
        for y in 0..self.h as i32 {
            for x in 0..self.w as i32 {
                if src.get(x, y) != CLEAR {
                    continue;
                }
                for (dx, dy) in [(1, 0), (0, 1), (1, 1), (1, -1)] {
                    let (Some((a, ca)), Some((b, cb))) = (near(x, y, dx, dy), near(x, y, -dx, -dy)) else { continue };
                    if a != b && sizes[a as usize] >= 3 && sizes[b as usize] >= 3 {
                        let (p, q) = (parts(ca), parts(cb));
                        let m = |u: u8, v: u8| ((u as u16 + v as u16) / 2) as u8;
                        self.set(x, y, rgb(m(p.0, q.0), m(p.1, q.1), m(p.2, q.2)));
                        break;
                    }
                }
            }
        }
    }

    /// Dört yanı da opak olan tek piksellik delikleri komşuların ortalamasıyla kapatır.
    pub fn fill_pinholes(&mut self) {
        let src = self.clone();
        for y in 1..self.h as i32 - 1 {
            for x in 1..self.w as i32 - 1 {
                if src.get(x, y) != CLEAR {
                    continue;
                }
                let n = [(1, 0), (-1, 0), (0, 1), (0, -1)].map(|(dx, dy)| src.get(x + dx, y + dy));
                if n.iter().all(|&c| c != CLEAR) {
                    let avg = |f: fn((u8, u8, u8)) -> u8| (n.iter().map(|&c| f(parts(c)) as u32).sum::<u32>() / 4) as u8;
                    self.set(x, y, rgb(avg(|p| p.0), avg(|p| p.1), avg(|p| p.2)));
                }
            }
        }
    }
}

/// Saydamlığın bu kadarı (0..255) ve üstü nesneye aittir.
const SOLID: u8 = 128;
/// Küçültülmüş hücrenin bu oranı kaplanmışsa piksel opak olur. Yarıdan biraz
/// azı ince bacak/kulak/kuyruğun kaybolmasını önler.
const COVER: f32 = 0.42;

/// Kesilmiş resmi (saydam arka plan) en uzun kenarı `size` olacak şekilde
/// piksel ızgarasına indirir. Renkler saydamlıkla ağırlıklı ortalanır (arka
/// planın rengi kenara sızmaz); kaplama eşiği altındaki hücreler saydam olur.
pub fn from_cutout(img: &Image, size: usize) -> Result<Sprite, String> {
    let (w, h) = (img.w as usize, img.h as usize);
    if w == 0 || h == 0 || img.bgra.len() < w * h * 4 {
        return Err(t!("Resim boş", "The image is empty").into());
    }
    let alpha = |x: usize, y: usize| img.bgra[(y * w + x) * 4 + 3];
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if alpha(x, y) >= SOLID {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
            }
        }
    }
    let none = || -> String { t!("Fotoğrafta belirgin bir nesne bulunamadı", "No clear subject was found in the photo").into() };
    if x0 == usize::MAX {
        return Err(none());
    }
    let (bw, bh) = (x1 - x0, y1 - y0);
    // Küçük resimler büyütülmez (bulanık iri pikseller olmasın).
    let k = (bw.max(bh) as f64 / size.max(1) as f64).max(1.0);
    let (dw, dh) = (((bw as f64 / k).round() as usize).clamp(1, size), ((bh as f64 / k).round() as usize).clamp(1, size));
    let mut s = Sprite::new(dw, dh);
    let span = |d: usize, n: usize, len: usize| {
        let a = ((d as f64 * len as f64 / n as f64) as usize).min(len - 1);
        let b = (((d + 1) as f64 * len as f64 / n as f64) as usize).clamp(a + 1, len);
        (a, b)
    };
    for dy in 0..dh {
        let (sy0, sy1) = span(dy, dh, bh);
        for dx in 0..dw {
            let (sx0, sx1) = span(dx, dw, bw);
            let (mut sa, mut sr, mut sg, mut sb) = (0u64, 0u64, 0u64, 0u64);
            for y in y0 + sy0..y0 + sy1 {
                for p in img.bgra[(y * w + x0 + sx0) * 4..(y * w + x0 + sx1) * 4].chunks_exact(4) {
                    let a = p[3] as u64;
                    sa += a;
                    sb += p[0] as u64 * a;
                    sg += p[1] as u64 * a;
                    sr += p[2] as u64 * a;
                }
            }
            let n = ((sy1 - sy0) * (sx1 - sx0)) as f32 * 255.0;
            if sa > 0 && sa as f32 / n >= COVER {
                s.px[dy * dw + dx] = rgb((sr / sa) as u8, (sg / sa) as u8, (sb / sa) as u8);
            }
        }
    }
    s.bridge_gaps();
    s.drop_specks();
    s.fill_pinholes();
    s.trimmed().ok_or_else(none)
}
