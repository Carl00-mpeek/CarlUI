//! Okşama için tıklama alanı.
//!
//! Tünek penceresi (`window.rs`) tamamen tıklama geçirir ve öyle kalır. Onun
//! yerine hayvanın yalnızca opak piksellerinin sınır kutusu kadar, içeriği
//! olmayan (WS_EX_NOREDIRECTIONBITMAP: hiçbir şey çizmez) küçük bir pencere
//! hayvanın üstünde tutulur. Kutunun dışındaki her tıklama bugünkü gibi
//! alttaki pencereye gider. Pencere odak çalmaz (WS_EX_NOACTIVATE +
//! MA_NOACTIVATE); sol tıkta dock penceresine `WM_APP_PET_CLICK` gönderir.
//!
//! Maliyet: zamanlayıcı ya da yoklama yok; pencere yalnızca hayvanın konumu ya
//! da durumu değişince (zaten çizilen karelerde) taşınır.
//!
//! Piksel başına isabet (alfa) yerine kutu kullanılır: içeriksiz pencerede
//! Windows alfa bilmez; HTTRANSPARENT ise yalnızca aynı iş parçacığının
//! pencerelerine geçirir, alttaki başka uygulamaya geçirmez.

use std::sync::Once;
use windows::{
    core::*,
    Win32::{Foundation::*, System::LibraryLoader::GetModuleHandleW, UI::WindowsAndMessaging::*},
};

/// Hayvana tıklandı (dock penceresine gönderilir).
pub const WM_APP_PET_CLICK: u32 = WM_APP + 32;

/// Opak piksellerin kutusu (kare içi sanat pikseli; sağ/alt hariç).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoxPx {
    pub l: u32,
    pub t: u32,
    pub r: u32,
    pub b: u32,
}

/// Yatay şeridin `frames` karesinin opak piksellerini kapsayan ortak kutu
/// (`px` PBGRA, `sw`×`sh`; kare `fw`×`fh`). Hiç opak piksel yoksa `None`.
pub fn opaque_box(px: &[u8], sw: u32, sh: u32, frames: u32, fw: u32, fh: u32) -> Option<BoxPx> {
    let mut b: Option<BoxPx> = None;
    for f in 0..frames {
        for y in 0..fh.min(sh) {
            for x in 0..fw {
                let sx = f * fw + x;
                if sx >= sw {
                    break;
                }
                let i = ((y * sw + sx) * 4 + 3) as usize;
                if px.get(i).copied().unwrap_or(0) < 32 {
                    continue;
                }
                b = Some(match b {
                    None => BoxPx { l: x, t: y, r: x + 1, b: y + 1 },
                    Some(o) => BoxPx { l: o.l.min(x), t: o.t.min(y), r: o.r.max(x + 1), b: o.b.max(y + 1) },
                });
            }
        }
    }
    b
}

/// Kutunun ekrandaki dikdörtgeni. `sprite_x`/`sprite_y` görselin pencere içi
/// sol üst köşesi, `k` büyütme, `fw` kare genişliği (sanat pikseli);
/// aynalıyken kutu yatayda çevrilir. Dock'un üstüne taşmasın diye alt kenar
/// `pill_top`'ta kesilir; boş kalırsa `None`.
#[allow(clippy::too_many_arguments)]
pub fn screen_rect(win: (i32, i32), sprite_x: i32, sprite_y: i32, k: f32, fw: u32, b: BoxPx, mirror: bool, pill_top: i32) -> Option<RECT> {
    let (l, r) = if mirror { (fw - b.r, fw - b.l) } else { (b.l, b.r) };
    let left = win.0 + sprite_x + (l as f32 * k).floor() as i32;
    let right = win.0 + sprite_x + (r as f32 * k).ceil() as i32;
    let top = win.1 + sprite_y + (b.t as f32 * k).floor() as i32;
    let bottom = (win.1 + sprite_y + (b.b as f32 * k).ceil() as i32).min(pill_top);
    (right > left && bottom > top).then_some(RECT { left, top, right, bottom })
}

extern "system" fn hit_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_LBUTTONDOWN => {
                let dock = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
                if dock != 0 {
                    let _ = PostMessageW(HWND(dock as _), WM_APP_PET_CLICK, WPARAM(0), LPARAM(0));
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

pub struct HitWindow {
    hwnd: HWND,
    dock: HWND,
    at: Option<RECT>,
}

impl HitWindow {
    pub fn new(dock: HWND) -> Result<Self> {
        static CLASS: Once = Once::new();
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let class = if crate::sandbox::on() { w!("CarlUI.DockSandboxPetHit") } else { w!("CarlUI.DockPetHit") };
            CLASS.call_once(|| {
                let cursor = LoadCursorW(None, IDC_HAND).unwrap_or_default();
                RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(hit_proc), hInstance: instance.into(), lpszClassName: class, hCursor: cursor, ..Default::default() });
            });
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP,
                class,
                w!(""),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                instance,
                None,
            )?;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, dock.0 as isize);
            Ok(Self { hwnd, dock, at: None })
        }
    }

    /// Kutuyu taşır (`None`: gizler). Değişmediyse dokunmaz.
    pub fn place(&mut self, r: Option<RECT>) {
        if r == self.at {
            return;
        }
        unsafe {
            match r {
                Some(r) => {
                    // Gösterirken dock'un hemen altına: büyüyen ikonlar üstte kalır.
                    let flags = if self.at.is_none() { SWP_NOACTIVATE | SWP_SHOWWINDOW } else { SWP_NOACTIVATE | SWP_NOZORDER };
                    let _ = SetWindowPos(self.hwnd, self.dock, r.left, r.top, r.right - r.left, r.bottom - r.top, flags);
                }
                None => {
                    let _ = ShowWindow(self.hwnd, SW_HIDE);
                }
            }
        }
        self.at = r;
    }
}

impl Drop for HitWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(fw: u32, fh: u32, frames: u32, opaque: &[(u32, u32, u32)]) -> Vec<u8> {
        let sw = fw * frames;
        let mut px = vec![0u8; (sw * fh * 4) as usize];
        for &(f, x, y) in opaque {
            px[((y * sw + f * fw + x) * 4 + 3) as usize] = 255;
        }
        px
    }

    #[test]
    fn opak_kutu_karelerin_birlesimi() {
        let px = strip(8, 6, 2, &[(0, 2, 3), (1, 5, 1), (1, 4, 5)]);
        assert_eq!(opaque_box(&px, 16, 6, 2, 8, 6), Some(BoxPx { l: 2, t: 1, r: 6, b: 6 }));
        assert_eq!(opaque_box(&strip(8, 6, 1, &[]), 8, 6, 1, 8, 6), None);
        // Kısa tampon taşmaz.
        assert_eq!(opaque_box(&[0; 8], 16, 6, 2, 8, 6), None);
    }

    #[test]
    fn ekran_kutusu_aynalama_ve_dock_siniri() {
        let b = BoxPx { l: 2, t: 4, r: 10, b: 16 };
        // Pencere (100, 500), görsel x=40 y=10, 2 kat büyütme.
        let r = screen_rect((100, 500), 40, 10, 2.0, 16, b, false, 10_000).unwrap();
        assert_eq!((r.left, r.top, r.right, r.bottom), (144, 518, 160, 542));
        // Aynalıyken kutu kare içinde çevrilir.
        let m = screen_rect((100, 500), 40, 10, 2.0, 16, b, true, 10_000).unwrap();
        assert_eq!((m.left, m.right), (140 + 12, 140 + 28));
        // Alt kenar dock'un üstünde kesilir: dock ikonlarının tıklaması çalınmaz.
        let c = screen_rect((100, 500), 40, 10, 2.0, 16, b, false, 530).unwrap();
        assert_eq!(c.bottom, 530);
        assert!(screen_rect((100, 500), 40, 10, 2.0, 16, b, false, 510).is_none());
    }
}
