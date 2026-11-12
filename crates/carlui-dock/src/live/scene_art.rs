//! Sahne parçacıklarının bitmap'leri: her tür için bir kez D2D ile çizilir,
//! fırça paylaşılır (bkz. `scene.rs`). Burada yalnızca çizim ve renkler var.

use super::particles::Sprite;
use carlui_core::theme::{Rgba, Theme};
use windows::{
    core::*,
    Win32::Graphics::Direct2D::{Common::*, *},
};

/// Bitmap'lerin piksel boyutu (genişlik, yükseklik). Görsel boyutu her karede
/// ayrıca verilir; bitmap ona gerilir (yumuşak doku, gerilmeye dayanıklı).
pub fn sprite_px(sp: Sprite) -> (f32, f32) {
    match sp {
        Sprite::Drop => (32.0, 32.0),
        Sprite::Streak => (8.0, 64.0),
        Sprite::Flake | Sprite::Star => (16.0, 16.0),
        Sprite::Firefly => (24.0, 24.0),
        Sprite::Cloud => (96.0, 48.0),
        Sprite::Moon => (64.0, 64.0),
    }
}

/// Parçacığın ekrandaki görsel boyutu (yarıçap ve uzamadan).
pub fn visual_size(sp: Sprite, r: f32, len: f32) -> (f32, f32) {
    match sp {
        Sprite::Cloud => (r * 2.4, r * 1.2),
        Sprite::Firefly => (r * 6.0, r * 6.0),
        Sprite::Star => (r * 5.0, r * 5.0),
        Sprite::Flake => (r * 3.0, r * 3.0),
        // Ayın bitmap'i haleyle birlikte diskin iki katı.
        Sprite::Moon => (r * 4.0, r * 4.0),
        _ => (r * 2.0, r * 2.0 * len),
    }
}

type C = (u8, u8, u8);

fn col(c: C, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: c.0 as f32 / 255.0, g: c.1 as f32 / 255.0, b: c.2 as f32 / 255.0, a }
}

fn pt(x: f32, y: f32) -> D2D_POINT_2F {
    D2D_POINT_2F { x, y }
}

/// Merkezden kenara sönen yumuşak elips.
#[allow(clippy::too_many_arguments)]
unsafe fn glow(dc: &ID2D1DeviceContext, cx: f32, cy: f32, rx: f32, ry: f32, c: C, a0: f32, a1: f32) -> Result<()> {
    let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: col(c, a0) }, D2D1_GRADIENT_STOP { position: 1.0, color: col(c, a1) }];
    let rt: &ID2D1RenderTarget = dc;
    let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
    let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES { center: pt(cx, cy), gradientOriginOffset: pt(0.0, 0.0), radiusX: rx, radiusY: ry };
    let b = dc.CreateRadialGradientBrush(&props, None, &coll)?;
    dc.FillEllipse(&D2D1_ELLIPSE { point: pt(cx, cy), radiusX: rx, radiusY: ry }, &b);
    Ok(())
}

unsafe fn ellipse(dc: &ID2D1DeviceContext, cx: f32, cy: f32, rx: f32, ry: f32, c: C, a: f32) -> Result<()> {
    let b = dc.CreateSolidColorBrush(&col(c, a), None)?;
    dc.FillEllipse(&D2D1_ELLIPSE { point: pt(cx, cy), radiusX: rx, radiusY: ry }, &b);
    Ok(())
}

/// Sahne renkleri: koyu camda açık, açık (süt beyazı) camda koyu tonlar.
pub struct Palette {
    water: C,
    shadow: C,
    flake: C,
    star: C,
    fly: C,
    cloud: C,
    moon: C,
}

pub fn palette(light: bool, theme: &Theme) -> Palette {
    let Rgba(r, g, b, _) = theme.accent.0;
    // Ateşböceği: vurgu renginin sıcak sarıya karışımı.
    let fly = (((r as u16 + 255) / 2) as u8, ((g as u16 + 230) / 2) as u8, ((b as u16 + 120) / 2) as u8);
    if light {
        Palette { water: (60, 90, 135), shadow: (30, 45, 70), flake: (110, 130, 170), star: (80, 95, 155), fly, cloud: (150, 165, 195), moon: (120, 125, 160) }
    } else {
        Palette { water: (215, 232, 255), shadow: (20, 30, 50), flake: (250, 252, 255), star: (240, 244, 255), fly, cloud: (255, 255, 255), moon: (255, 248, 220) }
    }
}

pub unsafe fn draw(dc: &ID2D1DeviceContext, sp: Sprite, p: &Palette) -> Result<()> {
    let (w, h) = sprite_px(sp);
    let (cx, cy) = (w / 2.0, h / 2.0);
    match sp {
        Sprite::Drop => {
            // Mercek gibi damla: koyu kenar (alt-sağ gölge), saydam gövde, üst-solda parıltı.
            ellipse(dc, cx + 1.0, cy + 1.5, 14.0, 14.0, p.shadow, 0.35)?;
            glow(dc, cx, cy, 14.5, 14.5, p.water, 0.25, 0.75)?;
            glow(dc, cx - 4.5, cy - 5.0, 5.0, 4.0, (255, 255, 255), 0.95, 0.0)?;
        }
        Sprite::Streak => {
            // Aşağı doğru koyulaşan çizgi; uçta dolu bir baş (gerçek yağmur izi).
            let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: col(p.water, 0.2) }, D2D1_GRADIENT_STOP { position: 1.0, color: col(p.water, 1.0) }];
            let rt: &ID2D1RenderTarget = dc;
            let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: pt(0.0, 0.0), endPoint: pt(0.0, h) };
            let b = dc.CreateLinearGradientBrush(&props, None, &coll)?;
            let rr = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: 0.5, top: 0.0, right: w - 0.5, bottom: h }, radiusX: 3.5, radiusY: 3.5 };
            dc.FillRoundedRectangle(&rr, &b);
        }
        Sprite::Flake => {
            // Dolu çekirdek + yumuşak hale: küçük boyda bile seçilir.
            glow(dc, cx, cy, 7.5, 7.5, p.flake, 0.9, 0.0)?;
            ellipse(dc, cx, cy, 4.6, 4.6, p.flake, 1.0)?;
        }
        Sprite::Star => {
            glow(dc, cx, cy, 7.5, 7.5, p.star, 0.7, 0.0)?;
            ellipse(dc, cx, cy, 3.4, 3.4, p.star, 1.0)?;
        }
        Sprite::Firefly => {
            glow(dc, cx, cy, 11.5, 11.5, p.fly, 0.7, 0.0)?;
            ellipse(dc, cx, cy, 2.2, 2.2, (255, 250, 210), 1.0)?;
        }
        Sprite::Cloud => {
            // Üst üste binen yumuşak öbekler.
            for (x, y, rx, ry) in [(30.0, 28.0, 26.0, 16.0), (52.0, 22.0, 28.0, 20.0), (70.0, 30.0, 24.0, 14.0), (46.0, 34.0, 36.0, 12.0)] {
                glow(dc, x, y, rx, ry, p.cloud, 0.55, 0.0)?;
            }
        }
        Sprite::Moon => {
            // Hale, dolu disk ve hafif kraterler (disk yarıçapı bitmap'in dörtte biri).
            let r = w / 4.0;
            glow(dc, cx, cy, w / 2.0, h / 2.0, p.moon, 0.35, 0.0)?;
            ellipse(dc, cx, cy, r, r, p.moon, 1.0)?;
            for (x, y, rr) in [(-0.3, -0.25, 0.22), (0.3, 0.15, 0.18), (-0.1, 0.4, 0.12)] {
                ellipse(dc, cx + x * r, cy + y * r, rr * r, rr * r, p.shadow, 0.12)?;
            }
        }
    }
    Ok(())
}
