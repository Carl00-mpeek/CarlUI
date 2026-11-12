//! Tek bir duruş resminden kural tabanlı animasyon. Her hareket piksel
//! ızgarasında yapılır: satır silme/çoğaltma (nefes, çökme), satır bandını
//! yatay kaydırma (adım, öne eğilme) ve yerden yükselme (zıplama). Yeniden
//! örnekleme yok, bu yüzden pikseller hep keskin kalır.
//!
//! Sprite sağa bakar (gerekirse çağıran önceden aynalar); "ön" = büyük x.

use super::sprite::{Sprite, CLEAR};

/// Konumlanmış kare: `x` gövdenin özgün sol kenarına göre yatay kayma,
/// `lift` en alt satırın yerden yüksekliği (sanat pikseli).
#[derive(Clone, Debug)]
pub struct Placed {
    pub s: Sprite,
    pub x: i32,
    pub lift: i32,
}

impl Placed {
    fn at(s: Sprite, x: i32, lift: i32) -> Self {
        Self { s, x, lift }
    }
}

/// Verilen satırları siler (sıra ve tekrar önemsiz).
pub fn remove_rows(s: &Sprite, rows: &[usize]) -> Sprite {
    let keep: Vec<usize> = (0..s.h).filter(|y| !rows.contains(y)).collect();
    let mut o = Sprite::new(s.w, keep.len());
    for (i, &y) in keep.iter().enumerate() {
        o.px[i * s.w..(i + 1) * s.w].copy_from_slice(&s.px[y * s.w..(y + 1) * s.w]);
    }
    o
}

/// `y` satırını iki kez yazar (o noktadan yukarısı 1 piksel yükselir).
pub fn dup_row(s: &Sprite, y: usize) -> Sprite {
    let y = y.min(s.h.saturating_sub(1));
    let mut rows: Vec<usize> = (0..s.h).collect();
    rows.insert(y, y);
    let mut o = Sprite::new(s.w, rows.len());
    for (i, &r) in rows.iter().enumerate() {
        o.px[i * s.w..(i + 1) * s.w].copy_from_slice(&s.px[r * s.w..(r + 1) * s.w]);
    }
    o
}

/// `y0..y1` satırlarındaki her pikseli `dx(x)` kadar kaydırır; tuval gerektiği
/// kadar genişler, sonra boş sütunlar kırpılır. Dönen sayı yeni sol kenarın
/// eskisine göre kayması. Üst üste binerse öndeki (büyük x) kazanır.
pub fn shift_band(s: &Sprite, y0: usize, y1: usize, dx: impl Fn(usize) -> i32) -> (Sprite, i32) {
    let m = (0..s.w).map(|x| dx(x).unsigned_abs() as usize).max().unwrap_or(0);
    let mut o = Sprite::new(s.w + 2 * m, s.h);
    for y in 0..s.h {
        for x in 0..s.w {
            let c = s.px[y * s.w + x];
            if c == CLEAR {
                continue;
            }
            let nx = (x + m) as i32 + if (y0..y1).contains(&y) { dx(x) } else { 0 };
            o.set(nx, y as i32, c);
        }
    }
    let left = (0..o.w).find(|&x| (0..o.h).any(|y| o.px[y * o.w + x] != CLEAR)).unwrap_or(0);
    let right = (0..o.w).rev().find(|&x| (0..o.h).any(|y| o.px[y * o.w + x] != CLEAR)).map_or(o.w, |x| x + 1);
    (o.crop(left, 0, right.max(left + 1), o.h), left as i32 - m as i32)
}

/// Bacak bandının yüksekliği: alt dörtte bir, en az 2 satır.
fn legs(s: &Sprite) -> usize {
    (s.h / 4).max(2).min(s.h.saturating_sub(1)).max(1)
}

/// Göğüs satırı: nefeste bu satır silinir (üst gövde 1 piksel iner).
fn chest(s: &Sprite) -> usize {
    (s.h * 2 / 5).min(s.h.saturating_sub(1))
}

/// Oturma: 8 kare, hafif nefes (göğüs 1 piksel iner, sonra kalkar).
pub fn idle(s: &Sprite) -> Vec<Placed> {
    const BREATH: [bool; 8] = [false, false, false, true, true, true, false, false];
    BREATH.iter().map(|&b| Placed::at(if b && s.h > 4 { remove_rows(s, &[chest(s)]) } else { s.clone() }, 0, 0)).collect()
}

/// Yürüme: 8 kare, silüet hiç kesilmez (yalnızca bütün satırlar kayar ya da
/// çoğalır). İki adımlık ritim: adımda gövde 1 piksel yükselir ve üst gövde
/// 1 piksel öne eğilir (paytak yürüyüş); en alt satırlar sırayla öne/arkaya
/// 1 piksel kayar (ayak sürme).
pub fn walk(s: &Sprite) -> Vec<Placed> {
    // (gövde yukarıda mı, ayak kayması)
    const STEPS: [(bool, i32); 8] = [(false, 0), (true, 1), (true, 1), (false, 0), (false, 0), (true, -1), (true, -1), (false, 0)];
    let lh = legs(s);
    let feet = lh.clamp(1, 2);
    let head = (s.h * 2 / 5).max(1);
    STEPS
        .iter()
        .map(|&(up, foot)| {
            let (mut f, mut x) = (s.clone(), 0);
            if foot != 0 && s.h > 6 {
                let (g, dx) = shift_band(&f, f.h - feet, f.h, |_| foot);
                (f, x) = (g, x + dx);
            }
            if up && s.h > 6 {
                let (g, dx) = shift_band(&f, 0, head, |_| 1);
                (f, x) = (g, x + dx);
            }
            if up && s.h > 4 {
                f = dup_row(&f, f.h - lh);
            }
            Placed::at(f, x, 0)
        })
        .collect()
}

/// Uyku "z"leri: küçük (4×4) ve büyük (5×5); karelerde sırayla değişir.
const ZED_SMALL: &[&str] = &["####", "..#.", ".#..", "####"];
const ZED_BIG: &[&str] = &["#####", "...#.", "..#..", ".#...", "#####"];
/// Uykudaki "z"nin rengi: açık, konturla her zeminde okunur.
pub const ZED_COLOUR: u32 = super::sprite::rgb(0xec, 0xe8, 0xf4);

/// Gövdenin üstüne, öne yakın bir "z" ekler; `up` kadar daha yukarıda.
fn with_zed(s: &Sprite, glyph: &[&str], up: usize) -> (Sprite, i32) {
    let gw = glyph[0].len();
    let top = glyph.len() + 1 + up;
    let zx = s.w.saturating_sub(3);
    let w = s.w.max(zx + gw);
    let mut o = Sprite::new(w, s.h + top);
    for y in 0..s.h {
        o.px[(y + top) * w..(y + top) * w + s.w].copy_from_slice(&s.px[y * s.w..(y + 1) * s.w]);
    }
    for (dy, row) in glyph.iter().enumerate() {
        for (dx, ch) in row.bytes().enumerate() {
            if ch == b'#' {
                o.set((zx + dx) as i32, dy as i32, ZED_COLOUR);
            }
        }
    }
    (o, 0)
}

/// Uyku: 4 yavaş kare. Bacaklar gövdenin altına toplanır, gövde basıklaşır
/// (yaklaşık üçte bir alçalır); nefeste 1 piksel kabarır, son iki karede "z".
pub fn sleep(s: &Sprite) -> Vec<Placed> {
    let lh = legs(s);
    let target = ((s.h as f32 * 0.62).round() as usize).max(3).min(s.h);
    let mut cut: Vec<usize> = Vec::new();
    // Önce bacaklar (en alt satır kalır: yere basan çizgi).
    for y in (s.h - lh..s.h - 1).rev() {
        if s.h - cut.len() <= target {
            break;
        }
        cut.push(y);
    }
    // Sonra gövde bandından eşit aralıklı satırlar.
    let (b0, b1) = (s.h / 6, s.h - lh);
    let need = (s.h - cut.len()).saturating_sub(target);
    if need > 0 && b1 > b0 {
        let step = (b1 - b0) as f32 / need as f32;
        for i in 0..need {
            cut.push(b0 + ((i as f32 + 0.5) * step) as usize);
        }
    }
    let mut flat = remove_rows(s, &cut);
    // Satır silmek çapraz bağlı ince bir kuyruk ucunu gövdeden koparabilir:
    // kopan kırıntı atılır, boşalan sol sütunlar kırpılıp kayma olarak verilir.
    flat.drop_specks();
    let left = (0..flat.w).find(|&x| (0..flat.h).any(|y| flat.px[y * flat.w + x] != CLEAR)).unwrap_or(0);
    let right = (0..flat.w).rev().find(|&x| (0..flat.h).any(|y| flat.px[y * flat.w + x] != CLEAR)).map_or(flat.w, |x| x + 1);
    let flat = flat.crop(left, 0, right.max(left + 1), flat.h);
    let breath = dup_row(&flat, flat.h / 2);
    // Küçük "z" alçakta, büyük "z" daha yukarıda: sırayla.
    [(false, ZED_SMALL, 0), (true, ZED_BIG, 1), (true, ZED_SMALL, 0), (false, ZED_BIG, 1)]
        .into_iter()
        .map(|(b, z, up)| {
            let f = if b { breath.clone() } else { flat.clone() };
            let (g, x) = with_zed(&f, z, up);
            Placed::at(g, x + left as i32, 0)
        })
        .collect()
}

/// Zıplama: 6 kare (dock 1-3. karelerde ileri taşır). Çömel, uzayarak
/// yüksel, in, çömel, otur.
pub fn hop(s: &Sprite) -> Vec<Placed> {
    let squash = |s: &Sprite| if s.h > 4 { remove_rows(s, &[chest(s)]) } else { s.clone() };
    let stretch = |s: &Sprite| dup_row(s, chest(s));
    vec![
        Placed::at(squash(s), 0, 0),
        Placed::at(stretch(s), 0, 2),
        Placed::at(s.clone(), 0, 3),
        Placed::at(s.clone(), 0, 2),
        Placed::at(squash(s), 0, 0),
        Placed::at(s.clone(), 0, 0),
    ]
}
