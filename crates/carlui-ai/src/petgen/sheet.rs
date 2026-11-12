//! Kareleri şeritlere dizer: her kareye kontur eklenir, tüm durumlar aynı
//! kare boyunu ve aynı yatay çapayı paylaşır (durum değişince hayvan
//! zıplamasın), ayaklar karenin en alt satırına basar (`baseline = fh`).

use super::anim::Placed;
use super::sprite::{parts, Sprite, CLEAR};
use crate::Image;

/// Dock'un kabul ettiği en küçük kare kenarı (bkz. dock `live/pack.rs`).
pub const FRAME_MIN: u32 = 8;
pub const FRAME_MAX: u32 = 128;

/// Bir durumun şeridi: `frames` kare yan yana, düz RGBA.
#[derive(Clone, Debug)]
pub struct StripOut {
    /// `pet.json`'daki durum anahtarı.
    pub key: &'static str,
    pub file: &'static str,
    pub frames: u32,
    /// Kare süresi (ms).
    pub ms: u32,
    /// `frames * fw` × `fh`, satırlar yukarıdan aşağı, düz RGBA.
    pub rgba: Vec<u8>,
}

/// Üretilmiş evcil hayvan (henüz diske yazılmamış).
#[derive(Clone, Debug)]
pub struct Pet {
    pub name: String,
    pub fw: u32,
    pub fh: u32,
    /// Sanat pikseli / saniye.
    pub walk_speed: f32,
    pub strips: Vec<StripOut>,
}

impl Pet {
    pub fn strip(&self, key: &str) -> Option<&StripOut> {
        self.strips.iter().find(|s| s.key == key)
    }

    /// Bir karenin opak piksel maskesi (sınama ve önizleme için).
    pub fn frame_alpha(&self, key: &str, i: u32) -> Option<Vec<bool>> {
        let s = self.strip(key)?;
        if i >= s.frames {
            return None;
        }
        let sw = (s.frames * self.fw) as usize;
        let mut v = Vec::with_capacity((self.fw * self.fh) as usize);
        for y in 0..self.fh as usize {
            for x in 0..self.fw as usize {
                v.push(s.rgba[(y * sw + i as usize * self.fw as usize + x) * 4 + 3] != 0);
            }
        }
        Some(v)
    }
}

/// Bir durum: anahtar, dosya, kare süresi, kareler (konturlanmamış).
pub struct State {
    pub key: &'static str,
    pub file: &'static str,
    pub ms: u32,
    pub frames: Vec<Placed>,
}

/// Konturlar, ortak kare boyunu bulur ve şeritleri çizer.
pub fn compose(name: &str, walk_speed: f32, states: Vec<State>, outline: u32) -> Result<Pet, String> {
    let states: Vec<(State, Vec<Placed>)> = states
        .into_iter()
        .map(|st| {
            let f = st.frames.iter().map(|p| Placed { s: p.s.outline(outline), x: p.x - 1, lift: p.lift }).collect();
            (st, f)
        })
        .collect();
    let all = || states.iter().flat_map(|(_, f)| f.iter());
    let min_x = all().map(|p| p.x).min().ok_or("kare yok")?;
    let max_x = all().map(|p| p.x + p.s.w as i32).max().unwrap_or(0);
    let top = all().map(|p| p.lift + p.s.h as i32).max().unwrap_or(0);
    let (mut fw, mut fh) = ((max_x - min_x) as u32, top.max(0) as u32);
    let pad_l = FRAME_MIN.saturating_sub(fw) / 2;
    fw = fw.max(FRAME_MIN);
    fh = fh.max(FRAME_MIN);
    if fw > FRAME_MAX || fh > FRAME_MAX {
        return Err(format!("kare {fw}x{fh} çok büyük"));
    }
    let mut strips = Vec::new();
    for (st, frames) in states {
        let n = frames.len() as u32;
        let sw = (n * fw) as usize;
        let mut rgba = vec![0u8; sw * fh as usize * 4];
        for (i, p) in frames.iter().enumerate() {
            let ox = i as i32 * fw as i32 + (p.x - min_x) + pad_l as i32;
            let oy = fh as i32 - p.lift - p.s.h as i32;
            blit(&mut rgba, sw, fh as usize, &p.s, ox, oy, i as i32 * fw as i32, fw as i32);
        }
        strips.push(StripOut { key: st.key, file: st.file, frames: n, ms: st.ms, rgba });
    }
    Ok(Pet { name: name.to_string(), fw, fh, walk_speed, strips })
}

/// Sprite'ı RGBA tuvale çizer; `cell..cell+cw` sütunlarının dışına taşmaz.
#[allow(clippy::too_many_arguments)]
fn blit(dst: &mut [u8], dw: usize, dh: usize, s: &Sprite, ox: i32, oy: i32, cell: i32, cw: i32) {
    for y in 0..s.h {
        for x in 0..s.w {
            let c = s.px[y * s.w + x];
            let (tx, ty) = (ox + x as i32, oy + y as i32);
            if c == CLEAR || tx < cell || tx >= cell + cw || ty < 0 || ty as usize >= dh {
                continue;
            }
            let (r, g, b) = parts(c);
            let i = (ty as usize * dw + tx as usize) * 4;
            dst[i..i + 4].copy_from_slice(&[r, g, b, 255]);
        }
    }
}

/// Göz kontrolü için önizleme: şeritler alt alta, her sanat pikseli
/// `scale`×`scale`, satranç tahtası zeminde (BGRA, düz).
pub fn preview(pet: &Pet, scale: u32) -> Image {
    let scale = scale.clamp(1, 16);
    let gap = 4 * scale;
    let maxw = pet.strips.iter().map(|s| s.frames * pet.fw).max().unwrap_or(1);
    let w = maxw * scale + 2 * gap;
    let h = pet.strips.len() as u32 * (pet.fh * scale + gap) + gap;
    let mut bgra = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let light = ((x / (4 * scale)) + (y / (4 * scale))) % 2 == 0;
            let v = if light { 0x3a } else { 0x30 };
            bgra[((y * w + x) * 4) as usize..][..4].copy_from_slice(&[v + 6, v, v, 255]);
        }
    }
    for (row, s) in pet.strips.iter().enumerate() {
        let sw = s.frames * pet.fw;
        let oy = gap + row as u32 * (pet.fh * scale + gap);
        for y in 0..pet.fh * scale {
            for x in 0..sw * scale {
                let si = (((y / scale) * sw + x / scale) * 4) as usize;
                if s.rgba[si + 3] == 0 {
                    // Kare sınırlarını belli et.
                    if x % (pet.fw * scale) == 0 {
                        let di = (((oy + y) * w + gap + x) * 4) as usize;
                        bgra[di..di + 4].copy_from_slice(&[0x60, 0x50, 0x50, 255]);
                    }
                    continue;
                }
                let di = (((oy + y) * w + gap + x) * 4) as usize;
                bgra[di..di + 4].copy_from_slice(&[s.rgba[si + 2], s.rgba[si + 1], s.rgba[si], 255]);
            }
        }
    }
    Image { w, h, bgra }
}
