//! Bu modül "oyunun görüntüsünü, odağını almadan ve tam ekranı bozmadan nasıl
//! alırız?" sorununu çözüyor: DXGI masaüstü çoğaltma (Desktop Duplication).
//! Pencere açmaz, kanca kurmaz, oyuna DLL sokmaz; Windows'un birleştiricisinin
//! ya da (özel tam ekranda) oyunun doğrudan ekrana bastığı görüntüyü GPU'da
//! kopyalar. Kare CPU'ya hiç inmez. Ekran kipi değişince (oyun özel tam ekrana
//! geçince, çözünürlük değişince, UAC ekranında) çoğaltma kaybolur; yeniden
//! kurulur.

use windows::{
    core::*,
    Win32::{
        Foundation::HMODULE,
        Graphics::{Direct3D::*, Direct3D11::*, Dxgi::Common::*, Dxgi::*},
    },
};

pub struct Screen {
    pub dev: ID3D11Device,
    pub ctx: ID3D11DeviceContext,
    output: IDXGIOutput1,
    dup: Option<IDXGIOutputDuplication>,
    pub w: u32,
    pub h: u32,
}

pub enum Got {
    /// Yeni bir kare `dst`'ye kopyalandı.
    Frame,
    /// Süre doldu, ekranda değişiklik yok.
    Idle,
    /// Çoğaltma şu an yok (güvenli masaüstü, kip değişimi); yeniden denenecek.
    Lost,
}

/// Birincil monitörü süren ekran kartında bir D3D11 aygıtı açar.
pub unsafe fn open() -> Result<Screen> {
    let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
    let mut ai = 0;
    while let Ok(adapter) = factory.EnumAdapters1(ai) {
        ai += 1;
        let mut oi = 0;
        while let Ok(output) = adapter.EnumOutputs(oi) {
            oi += 1;
            let Ok(desc) = output.GetDesc() else { continue };
            let r = desc.DesktopCoordinates;
            // Birincil monitör sanal masaüstünün (0,0) noktasındadır.
            if !desc.AttachedToDesktop.as_bool() || r.left != 0 || r.top != 0 {
                continue;
            }
            let mut dev = None;
            let mut ctx = None;
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut dev),
                None,
                Some(&mut ctx),
            )?;
            let (dev, ctx) = (dev.ok_or(Error::from(windows::Win32::Foundation::E_POINTER))?, ctx.ok_or(Error::from(windows::Win32::Foundation::E_POINTER))?);
            // Kodlayıcı aynı aygıtı başka iş parçacıklarından kullanır.
            if let Ok(mt) = dev.cast::<ID3D11Multithread>() {
                let _ = mt.SetMultithreadProtected(true);
            }
            let output: IDXGIOutput1 = output.cast()?;
            let mut s = Screen { dev, ctx, output, dup: None, w: (r.right - r.left) as u32, h: (r.bottom - r.top) as u32 };
            s.reopen();
            return Ok(s);
        }
    }
    Err(Error::new(windows::Win32::Foundation::E_FAIL, "birincil monitör bulunamadı"))
}

impl Screen {
    unsafe fn reopen(&mut self) {
        self.dup = None;
        match self.output.DuplicateOutput(&self.dev) {
            Ok(d) => {
                let desc = d.GetDesc();
                self.w = desc.ModeDesc.Width;
                self.h = desc.ModeDesc.Height;
                // Döndürülmüş ekranlarda genişlik/yükseklik yer değiştirir.
                if matches!(desc.Rotation, DXGI_MODE_ROTATION_ROTATE90 | DXGI_MODE_ROTATION_ROTATE270) {
                    std::mem::swap(&mut self.w, &mut self.h);
                }
                self.dup = Some(d);
            }
            Err(_) => {}
        }
    }

    /// Kodlayıcıya verilecek BGRA doku (çift boyutlu: H.264 tek sayı sevmez).
    pub unsafe fn texture(&self) -> Result<ID3D11Texture2D> {
        let td = D3D11_TEXTURE2D_DESC {
            Width: self.w & !1,
            Height: self.h & !1,
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
        self.dev.CreateTexture2D(&td, None, Some(&mut t))?;
        t.ok_or(Error::from(windows::Win32::Foundation::E_POINTER))
    }

    /// En çok `wait_ms` bekleyip yeni kare varsa `dst`'ye kopyalar.
    pub unsafe fn next(&mut self, wait_ms: u32, dst: &ID3D11Texture2D) -> Got {
        if self.dup.is_none() {
            self.reopen();
            if self.dup.is_none() {
                std::thread::sleep(std::time::Duration::from_millis(wait_ms.max(1) as u64));
                return Got::Lost;
            }
        }
        let dup = self.dup.as_ref().unwrap();
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut res = None;
        match dup.AcquireNextFrame(wait_ms, &mut info, &mut res) {
            Ok(()) => {
                let mut got = Got::Idle;
                // Yalnızca fare hareketi olan güncellemelerde görüntü değişmez.
                if info.LastPresentTime != 0 {
                    if let Some(tex) = res.and_then(|r| r.cast::<ID3D11Texture2D>().ok()) {
                        let mut sd = D3D11_TEXTURE2D_DESC::default();
                        tex.GetDesc(&mut sd);
                        let mut dd = D3D11_TEXTURE2D_DESC::default();
                        dst.GetDesc(&mut dd);
                        if sd.Width >= dd.Width && sd.Height >= dd.Height {
                            let b = D3D11_BOX { left: 0, top: 0, front: 0, right: dd.Width, bottom: dd.Height, back: 1 };
                            self.ctx.CopySubresourceRegion(dst, 0, 0, 0, 0, &tex, 0, Some(&b));
                            got = Got::Frame;
                        } else {
                            // Boyut değişti: çağıran yeni boyutla baştan kurar.
                            self.w = sd.Width;
                            self.h = sd.Height;
                            got = Got::Lost;
                        }
                    }
                }
                let _ = dup.ReleaseFrame();
                got
            }
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => Got::Idle,
            Err(_) => {
                // DXGI_ERROR_ACCESS_LOST vb.: kip değişti, yeniden kur.
                self.dup = None;
                Got::Lost
            }
        }
    }
}
