//! İkonlar: uygulama ikonları shell'den, sistem glifleri Direct2D ile vektörel.

use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::{
            Direct2D::{Common::*, *},
            Gdi::DeleteObject,
            Imaging::*,
        },
        UI::Shell::*,
    },
};

fn color(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a }
}

fn pt(x: f32, y: f32) -> D2D_POINT_2F {
    D2D_POINT_2F { x, y }
}

/// Exe yolu ya da `shell:AppsFolder\<AUMID>` için ikon yükler.
///
/// Hedefin iki katı istenir: shell küçük boyutlarda (ör. 32→30) kaba ölçekleyip
/// ikonu kenara yapıştırıyor. Büyük alıp Direct2D ile küçültmek hem daha keskin
/// hem de kesiksiz sonuç veriyor.
pub fn shell_icon(wic: &IWICImagingFactory, path: &str, px: i32) -> Result<IWICFormatConverter> {
    let req = (px * 2).max(64);
    unsafe {
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(path), None)?;
        let hbmp = factory.GetImage(SIZE { cx: req, cy: req }, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK)?;
        // Shell düz (premultiplied olmayan) alpha döndürüyor; dönüştürücü PBGRA'ya çevirir.
        let bmp = wic.CreateBitmapFromHBITMAP(hbmp, None, WICBitmapUseAlpha);
        let _ = DeleteObject(hbmp);
        let conv = wic.CreateFormatConverter()?;
        conv.Initialize(
            &bmp?,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeMedianCut,
        )?;
        Ok(conv)
    }
}

/// Pencere ikonundan (HICON) yükler; shell ikonu bulunamayan pencereler için.
pub fn hicon_bitmap(wic: &IWICImagingFactory, icon: windows::Win32::UI::WindowsAndMessaging::HICON) -> Result<IWICFormatConverter> {
    unsafe {
        let bmp = wic.CreateBitmapFromHICON(icon)?;
        let conv = wic.CreateFormatConverter()?;
        conv.Initialize(&bmp, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut)?;
        Ok(conv)
    }
}

/// Önizleme kartındaki kapat (×) düğmesi.
pub fn draw_close(dc: &ID2D1DeviceContext, px: f32, stroke: &ID2D1StrokeStyle) -> Result<()> {
    unsafe {
        let bg = dc.CreateSolidColorBrush(&color(0xE8, 0x48, 0x55, 0.92), None)?;
        let e = D2D1_ELLIPSE { point: pt(px / 2.0, px / 2.0), radiusX: px / 2.0, radiusY: px / 2.0 };
        dc.FillEllipse(&e, &bg);
        let fg = dc.CreateSolidColorBrush(&color(255, 255, 255, 1.0), None)?;
        let (a, b) = (px * 0.34, px * 0.66);
        let w = px * 0.1;
        dc.DrawLine(pt(a, a), pt(b, b), &fg, w, stroke);
        dc.DrawLine(pt(b, a), pt(a, b), &fg, w, stroke);
    }
    Ok(())
}

/// Dock göstergesi: yuvarlak nokta veya (genişse) hap. `accent` verilirse degrade.
pub fn draw_dot(dc: &ID2D1DeviceContext, w: f32, h: f32, accent: Option<((u8, u8, u8), (u8, u8, u8))>) -> Result<()> {
    unsafe {
        let rr = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: 0.0, top: 0.0, right: w, bottom: h }, radiusX: h / 2.0, radiusY: h / 2.0 };
        match accent {
            Some((a, b)) => {
                let stops = [
                    D2D1_GRADIENT_STOP { position: 0.0, color: color(a.0, a.1, a.2, 1.0) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: color(b.0, b.1, b.2, 1.0) },
                ];
                let rt: &ID2D1RenderTarget = dc;
                let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
                let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: pt(0.0, 0.0), endPoint: pt(w, 0.0) };
                let brush = dc.CreateLinearGradientBrush(&props, None, &coll)?;
                dc.FillRoundedRectangle(&rr, &brush);
            }
            None => {
                let brush = dc.CreateSolidColorBrush(&color(255, 255, 255, 0.7), None)?;
                dc.FillRoundedRectangle(&rr, &brush);
            }
        }
    }
    Ok(())
}

pub fn draw_bitmap(dc: &ID2D1DeviceContext, src: &IWICFormatConverter, px: f32) -> Result<()> {
    unsafe {
        let bmp = dc.CreateBitmapFromWicBitmap(src, None)?;
        let dest = D2D_RECT_F { left: 0.0, top: 0.0, right: px, bottom: px };
        dc.DrawBitmap(&bmp, Some(&dest), 1.0, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, None, None);
    }
    Ok(())
}

/// Windows logosu: dört yumuşak köşeli kare, mavi degrade.
pub fn draw_start(dc: &ID2D1DeviceContext, px: f32) -> Result<()> {
    unsafe {
        let stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: color(0x6C, 0xC4, 0xFF, 1.0) },
            D2D1_GRADIENT_STOP { position: 1.0, color: color(0x1E, 0x78, 0xE6, 1.0) },
        ];
        let rt: &ID2D1RenderTarget = dc;
        let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
        let total = px * 0.74;
        let o = (px - total) / 2.0;
        let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: pt(o, o), endPoint: pt(o + total, o + total) };
        let brush = dc.CreateLinearGradientBrush(&props, None, &coll)?;
        let gap = (px * 0.07).max(1.0);
        let sq = (total - gap) / 2.0;
        for (ix, iy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
            let x = o + ix * (sq + gap);
            let y = o + iy * (sq + gap);
            let rr = D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: x, top: y, right: x + sq, bottom: y + sq },
                radiusX: sq * 0.2,
                radiusY: sq * 0.2,
            };
            dc.FillRoundedRectangle(&rr, &brush);
        }
    }
    Ok(())
}

pub fn draw_search(dc: &ID2D1DeviceContext, px: f32, stroke: &ID2D1StrokeStyle) -> Result<()> {
    unsafe {
        let brush = dc.CreateSolidColorBrush(&color(255, 255, 255, 0.92), None)?;
        let w = px * 0.08;
        let c = px * 0.44;
        let r = px * 0.25;
        let e = D2D1_ELLIPSE { point: pt(c, c), radiusX: r, radiusY: r };
        dc.DrawEllipse(&e, &brush, w, stroke);
        let k = c + r * 0.72;
        dc.DrawLine(pt(k, k), pt(px * 0.8, px * 0.8), &brush, w, stroke);
    }
    Ok(())
}

/// Tray paneli düğmesi: koyu cam kare üzerinde yukarı ok.
pub fn draw_chevron(dc: &ID2D1DeviceContext, px: f32, stroke: &ID2D1StrokeStyle) -> Result<()> {
    unsafe {
        let bg = dc.CreateSolidColorBrush(&color(255, 255, 255, 0.07), None)?;
        let rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: 0.0, top: 0.0, right: px, bottom: px },
            radiusX: px * 0.3,
            radiusY: px * 0.3,
        };
        dc.FillRoundedRectangle(&rr, &bg);
        let fg = dc.CreateSolidColorBrush(&color(255, 255, 255, 0.85), None)?;
        let w = px * 0.065;
        let (l, m, r) = (px * 0.36, px * 0.5, px * 0.64);
        let (top, bottom) = (px * 0.43, px * 0.57);
        dc.DrawLine(pt(l, bottom), pt(m, top), &fg, w, stroke);
        dc.DrawLine(pt(m, top), pt(r, bottom), &fg, w, stroke);
    }
    Ok(())
}
