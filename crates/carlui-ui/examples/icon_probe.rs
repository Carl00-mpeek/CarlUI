//! Tanı aracı: shell'in döndürdüğü HBITMAP'in boyutunu ve alpha dağılımını yazdırır.
use windows::{core::*, Win32::{Foundation::*, Graphics::Gdi::*, System::WinRT::*, UI::Shell::*}};

fn main() -> Result<()> {
    unsafe {
        RoInitialize(RO_INIT_SINGLETHREADED)?;
        for path in std::env::args().skip(1) {
            for req in [30, 48, 64] {
                let f: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(path.as_str()), None)?;
                let hbmp = f.GetImage(SIZE { cx: req, cy: req }, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK)?;
                let mut bm = BITMAP::default();
                GetObjectW(hbmp, std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _));
                let n = (bm.bmWidth * bm.bmHeight) as usize;
                let px = std::slice::from_raw_parts(bm.bmBits as *const [u8; 4], if bm.bmBits.is_null() { 0 } else { n });
                let mut hist = [0usize; 3]; // 0, ara, 255
                let mut premul_violations = 0;
                for p in px {
                    let a = p[3];
                    hist[if a == 0 { 0 } else if a == 255 { 2 } else { 1 }] += 1;
                    if p[0] > a || p[1] > a || p[2] > a { premul_violations += 1; }
                }
                // ilk ve son sütunda opak piksel var mı? (kesik ikon belirtisi)
                let w = bm.bmWidth as usize;
                let edge = px.chunks(w.max(1)).filter(|r| r.first().map_or(false, |p| p[3] > 0) || r.last().map_or(false, |p| p[3] > 0)).count();
                println!("{path} req={req} -> {}x{} bpp={} height_sign={} alpha0/mid/255={:?} premul_ihlal={} kenar_dolu_satir={}",
                    bm.bmWidth, bm.bmHeight, bm.bmBitsPixel, bm.bmHeight.signum(), hist, premul_violations, edge);
                let _ = DeleteObject(hbmp);
            }
        }
    }
    Ok(())
}
