//! Yılanın çizim parçaları ve CPU tuvali. Parçalar yüklemede bir kez
//! sayfalardan kesilir; her poz bir "çizim listesi"ne (`Op`) çevrilir ve
//! liste değişmedikçe tuval yeniden çizilmez.
//!
//! Sayfa düzeni (yılan aracının çıktısı):
//! - `bead_fill` / `bead_band`: 9 kare 9×9, karın yönü k·45° (8 = sırt).
//! - `bead_line`: 6 kare 9×9, boyut 0..5 (kontur).
//! - `tail_fill_N`: kuyruk boncuğu N, 9 karın yönü.
//! - `head_<eğim>_line/fill`: 5 kare 28×34 (açık, kırp, dil 0-2), pivot (8, 21).
//! - `coil`, `coil_up`: 4 nefes karesi 54×44; `coil_extra`: kırp, dil 0-2.

use super::path::{self, Bead};
use crate::live::load::{Sheet, Sheets};

pub const CELL: i32 = 9;
pub const HEAD_W: i32 = 28;
pub const HEAD_H: i32 = 34;
pub const HEAD_PIVOT: (i32, i32) = (8, 21);
pub const COIL_W: i32 = 54;
pub const COIL_H: i32 = 44;

/// Başın yüz ifadesi (sanat dosyasındaki kare sırası).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    Open,
    Blink,
    Tongue(u8),
}

impl Face {
    pub fn index(self) -> usize {
        match self {
            Face::Open => 0,
            Face::Blink => 1,
            Face::Tongue(i) => 2 + i.min(2) as usize,
        }
    }
}

/// Tek bir parça: PBGRA pikseller (`u32`, önceden çarpılmış).
#[derive(Default)]
pub struct Spr {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u32>,
}

fn cut(s: &Sheet, x: u32, w: u32) -> Spr {
    let mut px = Vec::with_capacity((w * s.h) as usize);
    for y in 0..s.h {
        for i in 0..w {
            let o = ((y * s.w + x + i) * 4) as usize;
            px.push(s.px.get(o..o + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])));
        }
    }
    Spr { w: w as i32, h: s.h as i32, px }
}

fn row(s: Option<&Sheet>, n: u32, name: &str) -> Result<Vec<Spr>, String> {
    let s = s.ok_or_else(|| format!("{name} yok"))?;
    if s.w % n != 0 {
        return Err(format!("{name}: genişlik {} / {n}", s.w));
    }
    Ok((0..n).map(|i| cut(s, i * (s.w / n), s.w / n)).collect())
}

/// Kıvrım çizimindeki boyun/baş bölgesi: bu satırın üstü tamamen, sağ yarıda
/// ise `NECK_Y` satırına kadar. Açılırken/sarılırken gövdenin kendi başı
/// çizildiği için kıvrımın başı gizlenir (yoksa iki baş görünür).
const NECK_TOP: i32 = 21;
const NECK_X: i32 = 24;
const NECK_Y: i32 = 28;

fn neck(x: i32, y: i32) -> bool {
    y < NECK_TOP || (x >= NECK_X && y < NECK_Y)
}

/// Başsız kıvrım: boyun bölgesini siler, kesik kenarı kontur rengiyle kapatır.
fn bare(s: &Spr, outline: u32) -> Spr {
    let opaque = |x: i32, y: i32| x >= 0 && y >= 0 && x < s.w && y < s.h && s.px[(y * s.w + x) as usize] >> 24 != 0;
    let mut px = s.px.clone();
    for y in 0..s.h {
        for x in 0..s.w {
            let i = (y * s.w + x) as usize;
            if neck(x, y) {
                px[i] = 0;
            } else if opaque(x, y) && [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|&(dx, dy)| opaque(x + dx, y + dy) && neck(x + dx, y + dy)) {
                px[i] = outline;
            }
        }
    }
    Spr { w: s.w, h: s.h, px }
}

pub struct Art {
    /// [boyut 0..5][karın 0..8]; boyut 0 için bantlı hali ayrı.
    fill: Vec<Vec<Spr>>,
    band: Vec<Spr>,
    line: Vec<Spr>,
    /// [eğim][ifade]
    head_line: Vec<Vec<Spr>>,
    head_fill: Vec<Vec<Spr>>,
    coil: Vec<Spr>,
    coil_up: Vec<Spr>,
    coil_extra: Vec<Spr>,
    /// Açılma/sarılma sırasında çizilen başsız kıvrım.
    coil_bare: Spr,
}

const PITCH_FILES: [&str; 5] = ["m30", "p00", "p30", "p60", "p90"];

impl Art {
    pub fn new(sh: &Sheets) -> Result<Self, String> {
        let get = |n: &str| sh.get(&format!("{n}.png"));
        let mut fill = vec![row(get("bead_fill"), 9, "bead_fill")?];
        for i in 1..=5 {
            let n = format!("tail_fill_{i}");
            fill.push(row(get(&n), 9, &n)?);
        }
        let mut head_line = Vec::new();
        let mut head_fill = Vec::new();
        for p in PITCH_FILES {
            let (l, f) = (format!("head_{p}_line"), format!("head_{p}_fill"));
            head_line.push(row(get(&l), 5, &l)?);
            head_fill.push(row(get(&f), 5, &f)?);
        }
        let line = row(get("bead_line"), 6, "bead_line")?;
        let coil = row(get("coil"), 4, "coil")?;
        // Kontur rengi: boncuk konturunun ilk saydam olmayan pikseli.
        let outline = line.iter().flat_map(|s| s.px.iter()).copied().find(|c| c >> 24 != 0).unwrap_or(0xFF24_1E2A);
        let coil_bare = coil.first().map(|s| bare(s, outline)).unwrap_or_default();
        let a = Self {
            fill,
            band: row(get("bead_band"), 9, "bead_band")?,
            line,
            head_line,
            head_fill,
            coil,
            coil_up: row(get("coil_up"), 4, "coil_up")?,
            coil_extra: row(get("coil_extra"), 4, "coil_extra")?,
            coil_bare,
        };
        let ok = a.line.iter().chain(&a.band).all(|s| s.w == CELL && s.h == CELL)
            && a.head_line.iter().flatten().all(|s| s.w == HEAD_W && s.h == HEAD_H)
            && a.coil.iter().chain(&a.coil_up).chain(&a.coil_extra).all(|s| s.w == COIL_W && s.h == COIL_H);
        if !ok {
            return Err("yılan parçalarının boyutu beklenenden farklı".into());
        }
        Ok(a)
    }
}

/// Tuvale çizilecek tek parça. Konumlar tuvalin yerel pikselinde (sol üst).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Op {
    Line { size: u8, x: i32, y: i32 },
    Fill { size: u8, belly: Option<u8>, band: bool, x: i32, y: i32 },
    HeadLine { pitch: u8, face: Face, x: i32, y: i32 },
    HeadFill { pitch: u8, face: Face, x: i32, y: i32 },
    /// `bare`: başsız kıvrım (gövde açılırken/sarılırken).
    Coil { up: bool, frame: u8, face: Face, bare: bool, x: i32, y: i32 },
}

/// Bir boncuk şeridinin çizim sırası: önce kendi boncuklarının konturu
/// (+ baş konturu), sonra iki yandan 2'şer komşuyla dolgu (dikiş örtüşmesi)
/// ve baş dolgusu. Konumlar `dx, dy` kadar kaydırılır (yerel → tuval).
pub fn strand(out: &mut Vec<Op>, b: &[Bead], a: usize, e: usize, head: Option<(u8, Face)>, dx: i32, dy: i32) {
    let c = CELL / 2;
    for bd in &b[a..e] {
        out.push(Op::Line { size: bd.size, x: bd.x - c + dx, y: bd.y - c + dy });
    }
    let last = b[b.len() - 1];
    let (hx, hy) = (last.x - HEAD_PIVOT.0 + dx, last.y - HEAD_PIVOT.1 + dy);
    if let Some((pitch, face)) = head {
        out.push(Op::HeadLine { pitch, face, x: hx, y: hy });
    }
    for bd in &b[a.saturating_sub(2)..(e + 2).min(b.len())] {
        out.push(Op::Fill { size: bd.size, belly: bd.belly, band: bd.band, x: bd.x - c + dx, y: bd.y - c + dy });
    }
    if let Some((pitch, face)) = head {
        out.push(Op::HeadFill { pitch, face, x: hx, y: hy });
    }
}

/// Boncukları ön ve arka listelere böler; baş son şeritle birlikte çizilir.
pub fn body_ops(b: &[Bead], head: Option<(u8, Face)>, dx: i32, dy: i32, front: &mut Vec<Op>, back: &mut Vec<Op>) {
    for (a, e, is_back) in path::split(b.len(), |i| b[i].back) {
        let h = if e == b.len() { head } else { None };
        strand(if is_back { &mut *back } else { &mut *front }, b, a, e, h, dx, dy);
    }
}

/// Tuval: `w`×`h` PBGRA.
pub fn paint(art: &Art, ops: &[Op], px: &mut [u32], w: i32, h: i32) {
    for op in ops {
        let (s, x, y) = match *op {
            Op::Line { size, x, y } => (art.line.get(size as usize), x, y),
            Op::Fill { size, belly, band, x, y } => {
                let k = belly.unwrap_or(8) as usize;
                let set = if size == 0 && band { &art.band } else { &art.fill[(size as usize).min(5)] };
                (set.get(k), x, y)
            }
            Op::HeadLine { pitch, face, x, y } => (art.head_line.get(pitch as usize).and_then(|v| v.get(face.index())), x, y),
            Op::HeadFill { pitch, face, x, y } => (art.head_fill.get(pitch as usize).and_then(|v| v.get(face.index())), x, y),
            Op::Coil { bare: true, x, y, .. } => (Some(&art.coil_bare), x, y),
            Op::Coil { up, frame, face, x, y, .. } => {
                let s = match (up, face) {
                    // Kırpma ve dil yalnızca alçak kıvrımda çizildi.
                    (false, Face::Blink | Face::Tongue(_)) => art.coil_extra.get(face.index() - 1),
                    (false, Face::Open) => art.coil.get(frame as usize),
                    (true, _) => art.coil_up.get(frame as usize),
                };
                (s, x, y)
            }
        };
        if let Some(s) = s {
            put(px, w, h, s, x, y);
        }
    }
}

/// Parçanın saydam olmayan piksellerini üzerine yazar (piksel sanatı: tam ya da hiç).
fn put(px: &mut [u32], w: i32, h: i32, s: &Spr, x: i32, y: i32) {
    for sy in 0..s.h {
        let ty = y + sy;
        if ty < 0 || ty >= h {
            continue;
        }
        for sx in 0..s.w {
            let tx = x + sx;
            if tx < 0 || tx >= w {
                continue;
            }
            let c = s.px[(sy * s.w + sx) as usize];
            if c >> 24 != 0 {
                px[(ty * w + tx) as usize] = c;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serit_cizim_sirasi() {
        let b = path::beads(&path::body(&path::Track::default(), 200.0));
        let (mut f, mut k) = (Vec::new(), Vec::new());
        body_ops(&b, Some((1, Face::Open)), 0, 0, &mut f, &mut k);
        assert!(k.is_empty());
        // Önce tüm konturlar, sonra baş konturu, sonra dolgular, en son baş dolgusu.
        let first_fill = f.iter().position(|o| matches!(o, Op::Fill { .. })).unwrap();
        assert!(f[..first_fill].iter().all(|o| matches!(o, Op::Line { .. } | Op::HeadLine { .. })));
        assert!(matches!(f[first_fill - 1], Op::HeadLine { .. }));
        assert!(matches!(f.last(), Some(Op::HeadFill { .. })));
    }

    #[test]
    fn put_kirpar_ve_saydami_atlar() {
        let s = Spr { w: 2, h: 2, px: vec![0xff00_00ff, 0, 0, 0xff00_ff00] };
        let mut px = vec![7u32; 4];
        put(&mut px, 2, 2, &s, 1, 1);
        assert_eq!(px, vec![7, 7, 7, 0xff00_00ff]);
        put(&mut px, 2, 2, &s, -1, -1);
        assert_eq!(px[0], 0xff00_ff00);
    }
}
