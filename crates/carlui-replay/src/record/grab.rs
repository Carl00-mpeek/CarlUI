//! Alanı okuyup MP4'e yazar. Masaüstü çoğaltma yalnızca bir ekranı verdiği ve
//! imleci içermediği için burada GDI kullanılır: alan birkaç ekrana yayılabilir,
//! imleç kareye çizilir. Kare ekran kartına yüklenir, kodlamayı `writer.rs`teki
//! `Writer`'ı yapar. Ses ayrı iş parçacığında gelir; bilgisayar sessizken Windows
//! paket göndermediği için boşluk sessizlikle doldurulur.

use super::{Area, Phase, Shared};
use crate::{
    audio::Loopback,
    clock,
    writer::{frames_to_100ns, Format, Writer, AUDIO_CH, AUDIO_RATE},
};
use std::{
    sync::{atomic::Ordering, Arc, Mutex},
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::{
            Direct3D::*,
            Direct3D11::*,
            Dxgi::Common::*,
            Gdi::*,
        },
        Media::MediaFoundation::*,
        System::{Com::*, Threading::*},
        UI::WindowsAndMessaging::*,
    },
};

const FPS: u32 = 30;
/// Kodlayıcı kareyi eşzamansız okur: dokular sırayla yeniden kullanılır.
const POOL: usize = 6;

type Out = Arc<Mutex<Option<Writer>>>;

/// Ekrandan okunan kare: imleçli, üstten alta BGRA.
struct Screen {
    area: Area,
    screen: HDC,
    mem: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    bits: *mut core::ffi::c_void,
    /// Son imleç ve sıcak noktası.
    cursor: (isize, i32, i32),
}

impl Screen {
    unsafe fn open(area: Area) -> Result<Self> {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(screen);
        let bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: area.w as i32,
                biHeight: -(area.h as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bmp = CreateDIBSection(mem, &bi, DIB_RGB_COLORS, &mut bits, None, 0)?;
        let old = SelectObject(mem, bmp);
        Ok(Self { area, screen, mem, bmp, old, bits, cursor: (0, 0, 0) })
    }

    unsafe fn hotspot(&mut self, c: HCURSOR) -> (i32, i32) {
        if self.cursor.0 != c.0 as isize {
            let mut ii = ICONINFO::default();
            if GetIconInfo(HICON(c.0), &mut ii).is_ok() {
                self.cursor = (c.0 as isize, ii.xHotspot as i32, ii.yHotspot as i32);
                let _ = DeleteObject(ii.hbmMask);
                let _ = DeleteObject(ii.hbmColor);
            }
        }
        (self.cursor.1, self.cursor.2)
    }

    unsafe fn grab(&mut self) -> Result<()> {
        let a = self.area;
        BitBlt(self.mem, 0, 0, a.w as i32, a.h as i32, self.screen, a.x, a.y, SRCCOPY | CAPTUREBLT)?;
        let mut ci = CURSORINFO { cbSize: std::mem::size_of::<CURSORINFO>() as u32, ..Default::default() };
        if GetCursorInfo(&mut ci).is_ok() && ci.flags == CURSOR_SHOWING {
            let (hx, hy) = self.hotspot(ci.hCursor);
            let _ = DrawIconEx(self.mem, ci.ptScreenPos.x - hx - a.x, ci.ptScreenPos.y - hy - a.y, HICON(ci.hCursor.0), 0, 0, 0, None, DI_NORMAL);
        }
        Ok(())
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.mem, self.old);
            let _ = DeleteObject(self.bmp);
            let _ = DeleteDC(self.mem);
            ReleaseDC(None, self.screen);
        }
    }
}

unsafe fn device() -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    let (mut dev, mut ctx) = (None, None);
    D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut dev), None, Some(&mut ctx))?;
    let (Some(dev), Some(ctx)) = (dev, ctx) else { return Err(Error::from(E_POINTER)) };
    // Kodlayıcı aynı aygıtı başka iş parçacığından kullanır.
    let _ = dev.cast::<ID3D11Multithread>()?.SetMultithreadProtected(true);
    Ok((dev, ctx))
}

unsafe fn texture(dev: &ID3D11Device, a: Area) -> Result<ID3D11Texture2D> {
    let td = D3D11_TEXTURE2D_DESC {
        Width: a.w,
        Height: a.h,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut t = None;
    dev.CreateTexture2D(&td, None, Some(&mut t))?;
    t.ok_or(Error::from(E_POINTER))
}

/// Piksel başına ~0,14 bit: yazı ve arayüz net kalır, dosya şişmez.
fn bitrate(w: u32, h: u32) -> u32 {
    (w as u64 * h as u64 * FPS as u64 * 14 / 100).clamp(2_000_000, 40_000_000) as u32
}

fn audio_loop(out: Out, shared: Arc<Shared>, start: i64) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let rate = AUDIO_RATE as i64;
        let mut src = Loopback::open(false).ok();
        let mut retry = Instant::now();
        // Yazılmış ses, kare (örnek çifti) olarak.
        let mut pos = 0i64;
        let mut pcm: Vec<i16> = Vec::new();
        while !shared.stop.load(Ordering::SeqCst) {
            pcm.clear();
            match src.as_mut() {
                Some(s) => {
                    if s.read(|p, _| pcm.extend_from_slice(p)).is_err() {
                        src = None;
                        retry = Instant::now();
                    }
                }
                // Aygıt gitti (kulaklık çıkarıldı): yenisi gelene dek sessizlik.
                None if retry.elapsed() > Duration::from_secs(2) => {
                    src = Loopback::open(false).ok();
                    retry = Instant::now();
                }
                None => {}
            }
            let behind = (clock::now() - start) * rate / 10_000_000 - pos - pcm.len() as i64 / AUDIO_CH as i64;
            if behind > rate / 10 {
                pcm.resize(pcm.len() + (behind - rate / 20) as usize * AUDIO_CH as usize, 0);
            }
            if !pcm.is_empty() {
                let mut g = out.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(w) = g.as_mut() {
                    let _ = w.audio(&pcm, frames_to_100ns(pos as u64));
                }
                pos += pcm.len() as i64 / AUDIO_CH as i64;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

unsafe fn video_loop(area: Area, ctx: &ID3D11DeviceContext, pool: &[ID3D11Texture2D], out: &Out, shared: &Shared, start: i64) -> Result<u32> {
    let mut screen = Screen::open(area)?;
    let period = 10_000_000 / FPS as i64;
    let mut next = start;
    let mut frames = 0u32;
    while !shared.stop.load(Ordering::SeqCst) {
        let now = clock::now();
        if now < next {
            std::thread::sleep(Duration::from_millis(((next - now) / 10_000).clamp(1, 33) as u64));
            continue;
        }
        screen.grab()?;
        let tex = &pool[frames as usize % pool.len()];
        ctx.UpdateSubresource(tex, 0, None, screen.bits, area.w * 4, 0);
        ctx.Flush();
        if let Some(w) = out.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            w.video(tex, now - start, period)?;
        }
        frames += 1;
        next = (next + period).max(now);
    }
    Ok(frames)
}

unsafe fn record(area: Area, shared: &Arc<Shared>) -> Result<()> {
    let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL);
    let (dev, ctx) = device()?;
    let mut token = 0u32;
    let mut dm = None;
    MFCreateDXGIDeviceManager(&mut token, &mut dm)?;
    let dm = dm.ok_or(Error::from(E_POINTER))?;
    dm.ResetDevice(&dev, token)?;

    let audio = Loopback::open(false).is_ok();
    if !audio {
        warn!("alan kaydı: sistem sesi açılamadı, sessiz kaydediliyor");
        shared.silent.store(true, Ordering::SeqCst);
    }
    let (ow, oh) = area.out_size();
    let fmt = Format { w: area.w, h: area.h, ow, oh, hevc: false, fps: FPS, bitrate: bitrate(ow, oh), audio };
    if let Some(d) = shared.path.parent() {
        std::fs::create_dir_all(d).map_err(|_| Error::from(E_ACCESSDENIED))?;
    }
    let pool = (0..POOL).map(|_| texture(&dev, area)).collect::<Result<Vec<_>>>()?;
    let out: Out = Arc::new(Mutex::new(Some(Writer::open(&shared.path, &fmt, &dm)?)));

    let start = clock::now();
    let sound = audio.then(|| {
        let (o, s) = (out.clone(), shared.clone());
        std::thread::spawn(move || audio_loop(o, s, start))
    });
    let r = video_loop(area, &ctx, &pool, &out, shared, start);
    shared.stop.store(true, Ordering::SeqCst);
    if let Some(t) = sound {
        let _ = t.join();
    }
    shared.set(Phase::Saving);
    let w = out.lock().unwrap_or_else(|e| e.into_inner()).take();
    // Yazma yarıda kesildiyse (disk doldu) o ana kadarki kısım yine kurtarılır.
    let closed = w.map_or(Ok(()), |w| w.finish());
    let frames = r?;
    closed?;
    info!("alan kaydı bitti: {frames} kare");
    Ok(())
}

pub fn run(area: Area, shared: Arc<Shared>) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        match record(area, &shared) {
            Ok(()) => shared.set(Phase::Done),
            Err(e) => {
                warn!("alan kaydı başarısız: {e}");
                shared.stop.store(true, Ordering::SeqCst);
                let kept = std::fs::metadata(&shared.path).is_ok_and(|m| m.len() > 0);
                if !kept {
                    let _ = std::fs::remove_file(&shared.path);
                }
                shared.set(Phase::Failed(e.message()));
            }
        }
    }
}
