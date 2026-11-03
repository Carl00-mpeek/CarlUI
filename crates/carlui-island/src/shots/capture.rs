//! Ekranın o anki görüntüsünü alır. Önce GDI ile o anki kare kopyalanır;
//! siyah dönen monitörde (tam ekran oyun) Desktop Duplication (DXGI) kullanılır:
//! oyunlar dahil DWM'nin ekrana bastığı her şeyi yakalar ve oyunu arka plana atmaz.

use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::{Direct3D::*, Direct3D11::*, Dxgi::{Common::*, *}, Gdi::*},
    },
};

/// BGRA (alfa 255) piksel dizisi.
pub struct Frame {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
    /// Ekranda kapladığı yer (sanal masaüstü koordinatı).
    pub rect: RECT,
    /// İçerdiği monitörler (sanal masaüstü koordinatı); düzenlenen resimde boş.
    pub monitors: Vec<RECT>,
}

fn monitors() -> Vec<(HMONITOR, RECT)> {
    unsafe extern "system" fn cb(m: HMONITOR, _: HDC, r: *mut RECT, data: LPARAM) -> BOOL {
        (*(data.0 as *mut Vec<(HMONITOR, RECT)>)).push((m, *r));
        TRUE
    }
    let mut out = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(cb), LPARAM(&mut out as *mut _ as isize));
    }
    out
}

/// Tüm monitörleri AYNI ANDA yakalar (tek GDI kopyası: hiçbir ekran basıştan
/// sonraya kaymaz). GDI'nin siyah döndüğü monitör (tam ekran oyun, korumalı
/// içerik) ayrıca Desktop Duplication ile alınıp yerine konur. Desktop
/// Duplication bir SONRAKİ kareyi beklediği için yalnızca gerekince kullanılır.
pub fn all() -> Option<Frame> {
    let mons = monitors();
    let virt = mons.iter().fold(None::<RECT>, |acc, (_, r)| {
        Some(match acc {
            None => *r,
            Some(a) => RECT { left: a.left.min(r.left), top: a.top.min(r.top), right: a.right.max(r.right), bottom: a.bottom.max(r.bottom) },
        })
    })?;
    let mut frame = gdi(virt)?;
    for &(mon, r) in &mons {
        if region_black(&frame, r) {
            if let Some(d) = duplicate(mon, r) {
                paste(&mut frame, &d);
            }
        }
    }
    frame.monitors = mons.into_iter().map(|(_, r)| r).collect();
    Some(frame)
}

/// Yalnızca verilen monitör (özel tam ekran oyun: oyunun ekranı).
pub fn monitor_at(pt: POINT) -> Option<Frame> {
    let mon = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe {
        let _ = GetMonitorInfoW(mon, &mut mi);
    }
    let mut f = gdi(mi.rcMonitor).filter(|f| !is_black(f)).or_else(|| duplicate(mon, mi.rcMonitor)).or_else(|| gdi(mi.rcMonitor))?;
    f.monitors = vec![mi.rcMonitor];
    Some(f)
}

/// Kare tamamen siyah mı? (seyrek örnekleme yeter)
fn is_black(f: &Frame) -> bool {
    f.px.chunks_exact(4).step_by(997).all(|p| p[0] < 6 && p[1] < 6 && p[2] < 6)
}

/// Karenin bir monitöre düşen kısmı tamamen siyah mı?
fn region_black(f: &Frame, r: RECT) -> bool {
    let (x0, y0) = ((r.left - f.rect.left) as usize, (r.top - f.rect.top) as usize);
    let (w, h) = ((r.right - r.left) as usize, (r.bottom - r.top) as usize);
    (0..h).step_by(7).all(|y| (0..w).step_by(13).all(|x| {
        let i = ((y0 + y) * f.w as usize + x0 + x) * 4;
        f.px[i] < 6 && f.px[i + 1] < 6 && f.px[i + 2] < 6
    }))
}

/// Bir monitörün karesini sanal masaüstü karesindeki yerine kopyalar.
fn paste(dst: &mut Frame, src: &Frame) {
    let (x0, y0) = ((src.rect.left - dst.rect.left) as usize, (src.rect.top - dst.rect.top) as usize);
    let row = (src.w as usize).min(dst.w as usize - x0) * 4;
    for y in 0..(src.h as usize).min(dst.h as usize - y0) {
        let d = ((y0 + y) * dst.w as usize + x0) * 4;
        let s = y * src.w as usize * 4;
        dst.px[d..d + row].copy_from_slice(&src.px[s..s + row]);
    }
}

fn duplicate(mon: HMONITOR, rect: RECT) -> Option<Frame> {
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        let mut ai = 0;
        while let Ok(adapter) = factory.EnumAdapters1(ai) {
            ai += 1;
            let mut oi = 0;
            while let Ok(output) = adapter.EnumOutputs(oi) {
                oi += 1;
                let Ok(desc) = output.GetDesc() else { continue };
                if desc.Monitor != mon {
                    continue;
                }
                let mut dev = None;
                let mut ctx = None;
                D3D11CreateDevice(&adapter, D3D_DRIVER_TYPE_UNKNOWN, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut dev), None, Some(&mut ctx)).ok()?;
                let (dev, ctx) = (dev?, ctx?);
                let out1: IDXGIOutput1 = output.cast().ok()?;
                let dup = out1.DuplicateOutput(&dev).ok()?;
                let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
                let mut res = None;
                // İlk kare çoğu zaman boştur (görüntü henüz kopyalanmamış); gerçekten
                // ekrana basılmış bir kare gelene kadar kısa süre bekle. Ekran hiç
                // değişmiyorsa (masaüstü) GDI kopyası zaten doğru sonuç verir.
                let mut got = false;
                for _ in 0..6 {
                    res = None;
                    if dup.AcquireNextFrame(60, &mut info, &mut res).is_ok() {
                        if info.LastPresentTime != 0 && res.is_some() {
                            got = true;
                            break;
                        }
                        let _ = dup.ReleaseFrame();
                    }
                }
                if !got {
                    return None;
                }
                let tex: ID3D11Texture2D = res?.cast().ok()?;
                let mut td = D3D11_TEXTURE2D_DESC::default();
                tex.GetDesc(&mut td);
                td.Usage = D3D11_USAGE_STAGING;
                td.BindFlags = 0;
                td.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
                td.MiscFlags = 0;
                let mut staging = None;
                dev.CreateTexture2D(&td, None, Some(&mut staging)).ok()?;
                let staging = staging?;
                ctx.CopyResource(&staging, &tex);
                let _ = dup.ReleaseFrame();
                let mut map = D3D11_MAPPED_SUBRESOURCE::default();
                ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut map)).ok()?;
                let (w, h) = (td.Width, td.Height);
                let mut px = vec![0u8; (w * h * 4) as usize];
                for y in 0..h as usize {
                    let src = std::slice::from_raw_parts((map.pData as *const u8).add(y * map.RowPitch as usize), w as usize * 4);
                    px[y * w as usize * 4..(y + 1) * w as usize * 4].copy_from_slice(src);
                }
                ctx.Unmap(&staging, 0);
                for p in px.chunks_exact_mut(4) {
                    p[3] = 255;
                }
                // Döndürülmüş ekranlarda GDI'ye bırak (piksel düzeni farklı).
                if desc.Rotation != DXGI_MODE_ROTATION_IDENTITY && desc.Rotation != DXGI_MODE_ROTATION_UNSPECIFIED {
                    return None;
                }
                return Some(Frame { w, h, px, rect, monitors: Vec::new() });
            }
        }
        None
    }
}

fn gdi(r: RECT) -> Option<Frame> {
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp);
        let _ = BitBlt(mem, 0, 0, w, h, screen, r.left, r.top, SRCCOPY | CAPTUREBLT);
        let mut bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, ..Default::default() },
            ..Default::default()
        };
        let mut px = vec![0u8; (w * h * 4) as usize];
        let n = GetDIBits(mem, bmp, 0, h as u32, Some(px.as_mut_ptr() as *mut _), &mut bi, DIB_RGB_COLORS);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp);
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        if n == 0 {
            return None;
        }
        for p in px.chunks_exact_mut(4) {
            p[3] = 255;
        }
        Some(Frame { w: w as u32, h: h as u32, px, rect: r, monitors: Vec::new() })
    }
}
