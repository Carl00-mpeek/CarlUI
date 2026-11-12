//! Canlı dock'un ortak GPU yardımcıları: çözülmüş sayfadan kare kesme (bir
//! kez, yükleme anında) ve CPU'da çizilip yüzeye yüklenen tuval (yılan).
//! Büyütme her zaman ekranda, fırçanın en yakın komşu süzgeciyle yapılır;
//! yüzeyler sanat pikseli boyutundadır (küçük bellek).

use super::load::Sheet;
use carlui_ui::gfx::Gfx;
use windows::{
    core::*,
    Foundation::{Numerics::Matrix3x2, Size},
    Graphics::DirectX::{DirectXAlphaMode, DirectXPixelFormat},
    Win32::{
        Foundation::POINT,
        Graphics::{
            Direct2D::{Common::*, *},
            Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
        },
        System::WinRT::Composition::ICompositionDrawingSurfaceInterop,
    },
    UI::Composition::*,
};

/// PBGRA piksellerini (`stride` bayt satır) bağlamın sol üst köşesine çizer.
unsafe fn blit(dc: &ID2D1DeviceContext, px: &[u8], stride: u32, w: u32, h: u32) -> Result<()> {
    let props = D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
        dpiX: 96.0,
        dpiY: 96.0,
        ..Default::default()
    };
    let bmp = dc.CreateBitmap(D2D_SIZE_U { width: w, height: h }, Some(px.as_ptr() as _), stride, &props)?;
    let dest = D2D_RECT_F { left: 0.0, top: 0.0, right: w as f32, bottom: h as f32 };
    dc.DrawBitmap(&bmp, Some(&dest), 1.0, D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, None, None);
    Ok(())
}

/// En yakın komşu, gerilen fırça.
pub fn brush(gfx: &Gfx, surf: &CompositionDrawingSurface, nearest: bool) -> Result<CompositionSurfaceBrush> {
    let b = gfx.comp.CreateSurfaceBrushWithSurface(surf)?;
    b.SetBitmapInterpolationMode(if nearest { CompositionBitmapInterpolationMode::NearestNeighbor } else { CompositionBitmapInterpolationMode::Linear })?;
    b.SetStretch(CompositionStretch::Fill)?;
    Ok(b)
}

/// Sayfanın `(x, y, w, h)` bölgesini kendi yüzeyine kopyalar (taşan kısım kırpılır).
pub fn crop(gfx: &Gfx, s: &Sheet, x: u32, y: u32, w: u32, h: u32) -> Result<CompositionDrawingSurface> {
    crop_px(gfx, &s.px, s.w, s.h, x, y, w, h)
}

/// `crop`'un ham piksel hali (`sw`×`sh` PBGRA).
#[allow(clippy::too_many_arguments)]
pub fn crop_px(gfx: &Gfx, px: &[u8], sw: u32, sh: u32, x: u32, y: u32, w: u32, h: u32) -> Result<CompositionDrawingSurface> {
    let (w2, h2) = (w.min(sw.saturating_sub(x)), h.min(sh.saturating_sub(y)));
    gfx.surface(w as f32, h as f32, |dc| unsafe {
        let start = ((y * sw + x) * 4) as usize;
        if w2 == 0 || h2 == 0 || px.len() < start + (((h2 - 1) * sw + w2) * 4) as usize {
            return Ok(());
        }
        blit(dc, &px[start..], sw * 4, w2, h2)
    })
}

/// Yatay şeridin `n` karesini ayrı fırçalara böler (kare genişliği `w / n`).
pub fn frames(gfx: &Gfx, s: &Sheet, n: u32) -> Result<Vec<CompositionSurfaceBrush>> {
    let fw = s.w / n.max(1);
    (0..n).map(|i| brush(gfx, &crop(gfx, s, i * fw, 0, fw, s.h)?, true)).collect()
}

/// CPU'da çizilen PBGRA tuval; `upload` aynı yüzeyin içeriğini yeniler
/// (yeni yüzey ya da fırça oluşturulmaz).
pub struct Canvas {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u32>,
    surf: CompositionDrawingSurface,
    pub brush: CompositionSurfaceBrush,
}

impl Canvas {
    pub fn new(gfx: &Gfx, w: u32, h: u32) -> Result<Self> {
        let surf = gfx.gdev.CreateDrawingSurface(Size { Width: w as f32, Height: h as f32 }, DirectXPixelFormat::B8G8R8A8UIntNormalized, DirectXAlphaMode::Premultiplied)?;
        let brush = brush(gfx, &surf, true)?;
        Ok(Self { w, h, px: vec![0; (w * h) as usize], surf, brush })
    }

    /// Tuvali yüzeye yazar.
    pub fn upload(&self) -> Result<()> {
        let interop: ICompositionDrawingSurfaceInterop = self.surf.cast()?;
        unsafe {
            let mut off = POINT::default();
            let dc: ID2D1DeviceContext = interop.BeginDraw(None, &mut off)?;
            dc.SetDpi(96.0, 96.0);
            dc.SetTransform(&Matrix3x2::translation(off.x as f32, off.y as f32));
            dc.Clear(Some(&D2D1_COLOR_F::default()));
            let bytes = std::slice::from_raw_parts(self.px.as_ptr() as *const u8, self.px.len() * 4);
            let drawn = blit(&dc, bytes, self.w * 4, self.w, self.h);
            interop.EndDraw()?;
            drawn
        }
    }
}
