//! Bu modül "oyun ekranı başka bir çözünürlüğe aldığında kayıt nasıl sürer?"
//! sorununu çözüyor. Kodlayıcı ve segmentler hep masaüstü boyutunda kalır;
//! kaynak farklı boyuttaysa kare, ekran kartının video işlemcisiyle en-boy oranı
//! korunarak bu boyuta sığdırılır (kalan yerler siyah). Böylece oyuna girip
//! çıkmak kaydı kesmez ve eski segmentler yeni kayıtla birleştirilebilir.

use windows::{
    core::*,
    Win32::{
        Foundation::{BOOL, E_POINTER, RECT},
        Graphics::{Direct3D11::*, Dxgi::Common::DXGI_RATIONAL},
    },
};

/// `sw`×`sh` kaynağın `dw`×`dh` tuvalde en-boy oranı korunarak kapladığı alan.
pub fn rect(sw: u32, sh: u32, dw: u32, dh: u32) -> RECT {
    let (sw, sh, dw, dh) = (sw as u64, sh as u64, dw as u64, dh as u64);
    let (w, h) = if sw * dh <= dw * sh { (sw * dh / sh, dh) } else { (dw, sh * dw / sw) };
    let (x, y) = ((dw - w) / 2, (dh - h) / 2);
    RECT { left: x as i32, top: y as i32, right: (x + w) as i32, bottom: (y + h) as i32 }
}

pub struct Fit {
    vc: ID3D11VideoContext,
    vp: ID3D11VideoProcessor,
    input: ID3D11VideoProcessorInputView,
    outs: Vec<ID3D11VideoProcessorOutputView>,
}

impl Fit {
    /// `src` dokusundan (`sw`×`sh`) `outs` dokularına (`dw`×`dh`) sığdırma hazırlar.
    pub unsafe fn new(dev: &ID3D11Device, ctx: &ID3D11DeviceContext, src: &ID3D11Texture2D, (sw, sh): (u32, u32), outs: &[ID3D11Texture2D], (dw, dh): (u32, u32)) -> Result<Self> {
        let vd: ID3D11VideoDevice = dev.cast()?;
        let vc: ID3D11VideoContext = ctx.cast()?;
        let rate = DXGI_RATIONAL { Numerator: 60, Denominator: 1 };
        let desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: rate,
            InputWidth: sw,
            InputHeight: sh,
            OutputFrameRate: rate,
            OutputWidth: dw,
            OutputHeight: dh,
            Usage: D3D11_VIDEO_USAGE_OPTIMAL_SPEED,
        };
        let en = vd.CreateVideoProcessorEnumerator(&desc)?;
        let vp = vd.CreateVideoProcessor(&en, 0)?;

        let id = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC { ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D, ..Default::default() };
        let mut input = None;
        vd.CreateVideoProcessorInputView(src, &en, &id, Some(&mut input))?;
        let input = input.ok_or(Error::from(E_POINTER))?;
        let od = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC { ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D, ..Default::default() };
        let mut views = Vec::with_capacity(outs.len());
        for t in outs {
            let mut v = None;
            vd.CreateVideoProcessorOutputView(t, &en, &od, Some(&mut v))?;
            views.push(v.ok_or(Error::from(E_POINTER))?);
        }

        let full = RECT { left: 0, top: 0, right: sw as i32, bottom: sh as i32 };
        let dest = rect(sw, sh, dw, dh);
        let canvas = RECT { left: 0, top: 0, right: dw as i32, bottom: dh as i32 };
        let black = D3D11_VIDEO_COLOR { Anonymous: D3D11_VIDEO_COLOR_0 { RGBA: D3D11_VIDEO_COLOR_RGBA { R: 0.0, G: 0.0, B: 0.0, A: 1.0 } } };
        let (on, off) = (BOOL::from(true), BOOL::from(false));
        vc.VideoProcessorSetStreamFrameFormat(&vp, 0, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE);
        vc.VideoProcessorSetStreamAutoProcessingMode(&vp, 0, off);
        vc.VideoProcessorSetStreamSourceRect(&vp, 0, on, Some(&full));
        vc.VideoProcessorSetStreamDestRect(&vp, 0, on, Some(&dest));
        vc.VideoProcessorSetOutputTargetRect(&vp, on, Some(&canvas));
        vc.VideoProcessorSetOutputBackgroundColor(&vp, off, &black);
        Ok(Self { vc, vp, input, outs: views })
    }

    /// Kaynağı `slot` numaralı çıkış dokusuna sığdırır.
    pub unsafe fn blit(&self, slot: usize) -> Result<()> {
        let stream = D3D11_VIDEO_PROCESSOR_STREAM { Enable: true.into(), pInputSurface: std::mem::ManuallyDrop::new(Some(self.input.clone())), ..Default::default() };
        let mut streams = [stream];
        let r = self.vc.VideoProcessorBlt(&self.vp, &self.outs[slot], 0, &streams);
        std::mem::ManuallyDrop::drop(&mut streams[0].pInputSurface);
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::{
        Foundation::HMODULE,
        Graphics::{Direct3D::*, Dxgi::Common::*},
    };

    unsafe fn device() -> (ID3D11Device, ID3D11DeviceContext) {
        let (mut dev, mut ctx) = (None, None);
        D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut dev), None, Some(&mut ctx)).unwrap();
        (dev.unwrap(), ctx.unwrap())
    }

    unsafe fn texture(dev: &ID3D11Device, w: u32, h: u32, staging: bool) -> ID3D11Texture2D {
        let td = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: if staging { D3D11_USAGE_STAGING } else { D3D11_USAGE_DEFAULT },
            BindFlags: if staging { 0 } else { (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32 },
            CPUAccessFlags: if staging { D3D11_CPU_ACCESS_READ.0 as u32 } else { 0 },
            MiscFlags: 0,
        };
        let mut t = None;
        dev.CreateTexture2D(&td, None, Some(&mut t)).unwrap();
        t.unwrap()
    }

    unsafe fn fill(dev: &ID3D11Device, ctx: &ID3D11DeviceContext, t: &ID3D11Texture2D, rgba: [f32; 4]) {
        let mut v = None;
        dev.CreateRenderTargetView(t, None, Some(&mut v)).unwrap();
        ctx.ClearRenderTargetView(v.as_ref().unwrap(), &rgba);
    }

    /// (mavi, yeşil, kırmızı) piksel okur.
    unsafe fn pixels(dev: &ID3D11Device, ctx: &ID3D11DeviceContext, t: &ID3D11Texture2D, (w, h): (u32, u32), at: &[(u32, u32)]) -> Vec<(u8, u8, u8)> {
        let s = texture(dev, w, h, true);
        ctx.CopyResource(&s, t);
        let mut m = D3D11_MAPPED_SUBRESOURCE::default();
        ctx.Map(&s, 0, D3D11_MAP_READ, 0, Some(&mut m)).unwrap();
        let out = at
            .iter()
            .map(|&(x, y)| {
                let p = (m.pData as *const u8).add((y * m.RowPitch + x * 4) as usize);
                (*p, *p.add(1), *p.add(2))
            })
            .collect();
        ctx.Unmap(&s, 0);
        out
    }

    #[test]
    fn rect_keeps_aspect() {
        let r = rect(1568, 1080, 1920, 1080);
        assert_eq!((r.left, r.top, r.right, r.bottom), (176, 0, 1744, 1080));
        let r = rect(2560, 1080, 1920, 1080);
        assert_eq!((r.left, r.right), (0, 1920));
        assert_eq!(r.bottom - r.top, 810);
        let r = rect(2560, 1440, 1920, 1080);
        assert_eq!((r.left, r.top, r.right, r.bottom), (0, 0, 1920, 1080));
    }

    fn red(p: (u8, u8, u8)) -> bool {
        p.2 > 230 && p.0 < 25 && p.1 < 25
    }
    fn black(p: (u8, u8, u8)) -> bool {
        p.0 < 25 && p.1 < 25 && p.2 < 25
    }

    #[test]
    fn narrow_source_gets_black_bars() {
        unsafe {
            let (dev, ctx) = device();
            let src = texture(&dev, 1568, 1080, false);
            let out = texture(&dev, 1920, 1080, false);
            fill(&dev, &ctx, &src, [1.0, 0.0, 0.0, 1.0]);
            fill(&dev, &ctx, &out, [1.0, 1.0, 1.0, 1.0]);
            let f = Fit::new(&dev, &ctx, &src, (1568, 1080), std::slice::from_ref(&out), (1920, 1080)).unwrap();
            f.blit(0).unwrap();
            let p = pixels(&dev, &ctx, &out, (1920, 1080), &[(20, 540), (960, 540), (190, 20), (1730, 1060), (1900, 540)]);
            assert!(black(p[0]) && black(p[4]), "{p:?}");
            assert!(red(p[1]) && red(p[2]) && red(p[3]), "{p:?}");
        }
    }

    #[test]
    fn large_source_is_scaled_down() {
        unsafe {
            let (dev, ctx) = device();
            let src = texture(&dev, 2560, 1440, false);
            let out = texture(&dev, 1920, 1080, false);
            fill(&dev, &ctx, &src, [1.0, 0.0, 0.0, 1.0]);
            let f = Fit::new(&dev, &ctx, &src, (2560, 1440), std::slice::from_ref(&out), (1920, 1080)).unwrap();
            f.blit(0).unwrap();
            let p = pixels(&dev, &ctx, &out, (1920, 1080), &[(4, 4), (960, 540), (1915, 1075)]);
            assert!(p.iter().all(|x| red(*x)), "{p:?}");
        }
    }
}
