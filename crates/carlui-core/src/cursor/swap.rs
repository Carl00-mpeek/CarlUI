//! Görünen sistem imlecini kısa süreliğine hazır bir görselle değiştirip geri
//! koymak için ortak parçalar (dock'taki buzlanma, sallayınca büyüme).
//!
//! `SetSystemCursor` rolün paylaşılan tutamağının içeriğini yerinde değiştirir:
//! imleç kıpırdamasa da yeni görsel hemen görünür, çizimi Windows yapar. Geri
//! koymak için değiştirmeden önce rolün bir kopyası alınır; `SPI_SETCURSORS`
//! kullanılmaz (CarlUI imleç setini baştan yükletirdi).

use windows::core::PCWSTR;
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Değiştirilebilen roller (OCR_*); animasyonlu meşgul imleçleri hariç.
pub const ROLES: [u32; 15] = [32512, 32513, 32515, 32516, 32631, 32642, 32643, 32644, 32645, 32646, 32648, 32649, 32651, 32671, 32672];

/// İmleç görseli: düz (ön çarpımsız) BGRA, satırlar yukarıdan aşağı.
#[derive(Clone)]
pub struct Pixels {
    pub w: usize,
    pub h: usize,
    pub hot: (u32, u32),
    pub px: Vec<u8>,
}

impl Pixels {
    /// Görünür sayılan pikseller.
    pub fn on(&self) -> Vec<bool> {
        self.px.chunks_exact(4).map(|p| p[3] > 24).collect()
    }
}

/// Görünen imleç: ekran konumu ve tutamağı. Gizliyse (yazarken, oyunda) `None`.
pub fn shown() -> Option<(POINT, HCURSOR)> {
    let mut ci = CURSORINFO { cbSize: std::mem::size_of::<CURSORINFO>() as u32, ..Default::default() };
    unsafe { GetCursorInfo(&mut ci).ok()? };
    (ci.flags == CURSOR_SHOWING && !ci.hCursor.is_invalid()).then_some((ci.ptScreenPos, ci.hCursor))
}

fn system(id: u32) -> Option<HCURSOR> {
    unsafe { LoadCursorW(None, PCWSTR(id as usize as *const u16)).ok() }
}

/// Bu tutamak hangi sistem rolü? Uygulamanın kendi imleciyse `None`.
pub fn role(c: HCURSOR) -> Option<u32> {
    ROLES.into_iter().find(|&id| system(id) == Some(c))
}

/// Bitmap'i 32 bit üstten alta piksellere çevirir.
unsafe fn dib(bmp: HBITMAP) -> Option<(usize, usize, Vec<u8>)> {
    if bmp.is_invalid() {
        return None;
    }
    let mut bm = BITMAP::default();
    if GetObjectW(bmp, std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _)) == 0 {
        return None;
    }
    let (w, h) = (bm.bmWidth, bm.bmHeight);
    if w <= 0 || h <= 0 || w > 512 || h > 1024 {
        return None;
    }
    let mut bi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() },
        ..Default::default()
    };
    let mut px = vec![0u8; (w * h * 4) as usize];
    let dc = GetDC(None);
    let n = GetDIBits(dc, bmp, 0, h as u32, Some(px.as_mut_ptr() as *mut _), &mut bi, DIB_RGB_COLORS);
    ReleaseDC(None, dc);
    (n > 0).then_some((w as usize, h as usize, px))
}

/// Renkli imleç: alfa kanalı varsa olduğu gibi. Alfa hiç yoksa (eski 24 bit
/// imleç) maskede siyah olan piksel opak sayılır.
fn from_color(mut px: Vec<u8>, mask: &[u8]) -> Vec<u8> {
    if !px.chunks_exact(4).any(|p| p[3] != 0) {
        for (i, p) in px.chunks_exact_mut(4).enumerate() {
            p[3] = if mask.get(i * 4).is_some_and(|m| *m < 128) { 255 } else { 0 };
        }
    }
    px
}

/// Tek renkli imleç: maske iki kat yüksektir; üst yarı AND, alt yarı XOR.
/// AND siyahsa piksel XOR rengiyle boyanır; AND ve XOR beyazsa ekran ters
/// çevrilir (metin imleci böyle çizilir), burada beyaz sayılır.
fn from_mono(mask: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut px = vec![0u8; w * h * 4];
    for i in 0..w * h {
        let and_black = mask.get(i * 4).is_some_and(|v| *v < 128);
        let xor_white = mask.get((w * h + i) * 4).is_some_and(|v| *v >= 128);
        let c = if and_black { if xor_white { 255 } else { 0 } } else { 255 };
        if and_black || xor_white {
            px[i * 4..i * 4 + 4].copy_from_slice(&[c, c, c, 255]);
        }
    }
    px
}

/// İmlecin görseli ve sıcak noktası.
pub fn pixels(c: HCURSOR) -> Option<Pixels> {
    unsafe {
        let mut ii = ICONINFO::default();
        GetIconInfo(HICON(c.0), &mut ii).ok()?;
        let mask = dib(ii.hbmMask);
        let color = dib(ii.hbmColor);
        let _ = DeleteObject(ii.hbmMask);
        if !ii.hbmColor.is_invalid() {
            let _ = DeleteObject(ii.hbmColor);
        }
        let (mw, mh, m) = mask?;
        let hot = (ii.xHotspot, ii.yHotspot);
        let p = match color {
            Some((w, h, px)) => Pixels { w, h, hot, px: from_color(px, if mh == h { &m } else { &[] }) },
            None => Pixels { w: mw, h: mh / 2, hot, px: from_mono(&m, mw, mh / 2) },
        };
        p.px.chunks_exact(4).any(|q| q[3] > 24).then_some(p)
    }
}

/// Görselden yeni bir imleç; çağıran `DestroyCursor` ile bırakır.
pub fn make(p: &Pixels) -> Option<HCURSOR> {
    unsafe {
        let bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: p.w as i32, biHeight: -(p.h as i32), biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let color = CreateDIBSection(None, &bi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        std::ptr::copy_nonoverlapping(p.px.as_ptr(), bits as *mut u8, p.w * p.h * 4);
        // Boş AND maskesi: görünürlüğü alfa belirler.
        let row = p.w.div_ceil(16) * 2;
        let zeros = vec![0u8; row * p.h];
        let mask = CreateBitmap(p.w as i32, p.h as i32, 1, 1, Some(zeros.as_ptr() as *const _));
        let ii = ICONINFO { fIcon: false.into(), xHotspot: p.hot.0, yHotspot: p.hot.1, hbmMask: mask, hbmColor: color };
        let h = CreateIconIndirect(&ii);
        let _ = DeleteObject(color);
        let _ = DeleteObject(mask);
        h.ok().map(|h| HCURSOR(h.0))
    }
}

/// Bir rolün geçici olarak değiştirilmesi. Değiştirmeden önce rolün kopyası
/// alınır; `end` (ya da düşürülmesi) onu geri koyar.
pub struct Swap {
    id: u32,
    orig: Option<HICON>,
}

impl Swap {
    pub fn begin(id: u32) -> Option<Swap> {
        let c = system(id)?;
        let orig = unsafe { CopyIcon(HICON(c.0)).ok()? };
        Some(Swap { id, orig: Some(orig) })
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    /// Rolü `c`'nin bir kopyasıyla değiştirir (`c` çağıranda kalır).
    pub fn show(&self, c: HCURSOR) -> bool {
        unsafe {
            let Ok(copy) = CopyIcon(HICON(c.0)) else { return false };
            let ok = SetSystemCursor(HCURSOR(copy.0), SYSTEM_CURSOR_ID(self.id)).is_ok();
            if !ok {
                let _ = DestroyIcon(copy);
            }
            ok
        }
    }

    pub fn end(mut self) {
        self.put_back();
    }

    fn put_back(&mut self) {
        if let Some(orig) = self.orig.take() {
            unsafe {
                if SetSystemCursor(HCURSOR(orig.0), SYSTEM_CURSOR_ID(self.id)).is_err() {
                    let _ = DestroyIcon(orig);
                }
            }
        }
    }
}

impl Drop for Swap {
    fn drop(&mut self) {
        self.put_back();
    }
}

/// Çift doğrusal büyütme/küçültme (düz alfayı ağırlık sayarak, kenarlarda
/// renk kirlenmesin). Sıcak nokta da ölçeklenir.
pub fn scale(p: &Pixels, w: usize, h: usize) -> Pixels {
    let (sx, sy) = (p.w as f32 / w as f32, p.h as f32 / h as f32);
    let get = |x: i32, y: i32| -> [f32; 4] {
        let x = x.clamp(0, p.w as i32 - 1) as usize;
        let y = y.clamp(0, p.h as i32 - 1) as usize;
        let q = &p.px[(y * p.w + x) * 4..][..4];
        let a = q[3] as f32 / 255.0;
        [q[0] as f32 * a, q[1] as f32 * a, q[2] as f32 * a, a]
    };
    let mut px = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let fx = (x as f32 + 0.5) * sx - 0.5;
            let fy = (y as f32 + 0.5) * sy - 0.5;
            let (x0, y0) = (fx.floor() as i32, fy.floor() as i32);
            let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
            let (a, b, c, d) = (get(x0, y0), get(x0 + 1, y0), get(x0, y0 + 1), get(x0 + 1, y0 + 1));
            let mut v = [0f32; 4];
            for k in 0..4 {
                let top = a[k] + (b[k] - a[k]) * tx;
                let bot = c[k] + (d[k] - c[k]) * tx;
                v[k] = top + (bot - top) * ty;
            }
            let o = &mut px[(y * w + x) * 4..][..4];
            if v[3] > 0.0 {
                for k in 0..3 {
                    o[k] = (v[k] / v[3]).round().clamp(0.0, 255.0) as u8;
                }
            }
            o[3] = (v[3] * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    let hot = ((p.hot.0 as f32 / sx).round() as u32, (p.hot.1 as f32 / sy).round() as u32);
    Pixels { w, h, hot, px }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tek_renkli_metin_imleci_gorunur_sayilir() {
        // 3×1: siyah boyanan, ters çevrilen, saydam.
        let px = |v: u8| [v, v, v, 0];
        let mask: Vec<u8> = [px(0), px(255), px(255), px(0), px(255), px(0)].concat();
        let out = from_mono(&mask, 3, 1);
        assert_eq!(&out[0..4], &[0, 0, 0, 255]);
        assert_eq!(&out[4..8], &[255, 255, 255, 255]);
        assert_eq!(out[11], 0);
    }

    #[test]
    fn alfasiz_renkli_imlec_maskeye_bakar() {
        let px = vec![10, 10, 10, 0, 20, 20, 20, 0];
        let mask = vec![0, 0, 0, 0, 255, 255, 255, 0];
        let out = from_color(px, &mask);
        assert_eq!((out[3], out[7]), (255, 0));
        let alpha = vec![0, 0, 0, 200, 0, 0, 0, 0];
        assert_eq!(from_color(alpha.clone(), &[]), alpha);
    }

    #[test]
    fn olcekleme_boyutu_ve_sicak_noktayi_tasir() {
        let mut px = vec![0u8; 4 * 4 * 4];
        for p in px.chunks_exact_mut(4).take(8) {
            p.copy_from_slice(&[40, 80, 120, 255]);
        }
        let p = Pixels { w: 4, h: 4, hot: (1, 2), px };
        let big = scale(&p, 12, 12);
        assert_eq!((big.w, big.h, big.hot), (12, 12, (3, 6)));
        // Üst yarı opak ve renk korunur, alt köşe saydam.
        assert_eq!(&big.px[0..4], &[40, 80, 120, 255]);
        assert_eq!(big.px[(11 * 12 + 11) * 4 + 3], 0);
    }
}
