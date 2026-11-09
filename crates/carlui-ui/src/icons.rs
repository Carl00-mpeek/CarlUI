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
    let (c, a) = crate::tone::ink((r as u32) << 16 | (g as u32) << 8 | b as u32, a);
    D2D1_COLOR_F { r: (c >> 16) as f32 / 255.0, g: ((c >> 8) & 0xFF) as f32 / 255.0, b: (c & 0xFF) as f32 / 255.0, a }
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

/// Çizgi göstergesi: `len` x `thick` boyunda, uçları yuvarlak bir çizgi; pencere
/// sayısı kadar (`n`) parçaya çapraz kesiklerle bölünür. Toplam boy hep aynıdır.
/// `accent` verilirse degrade, yoksa yarı saydam beyaz (gri görünür).
/// `vertical`: yüzey `thick` x `len` boyundadır, çizgi dikey çizilir.
pub fn draw_split_line(dc: &ID2D1DeviceContext, len: f32, thick: f32, n: usize, accent: Option<((u8, u8, u8), (u8, u8, u8))>, vertical: bool) -> Result<()> {
    unsafe {
        if vertical {
            // Yatay çiziz, 90° döndürürüz: (x, y) -> (thick - y, x).
            let mut t = windows::Foundation::Numerics::Matrix3x2::default();
            dc.GetTransform(&mut t);
            let r = windows::Foundation::Numerics::Matrix3x2 { M11: 0.0, M12: 1.0, M21: -1.0, M22: 0.0, M31: thick + t.M31, M32: t.M32 };
            dc.SetTransform(&r);
        }
        let brush: ID2D1Brush = match accent {
            Some((a, b)) => {
                let stops = [
                    D2D1_GRADIENT_STOP { position: 0.0, color: color(a.0, a.1, a.2, 1.0) },
                    D2D1_GRADIENT_STOP { position: 1.0, color: color(b.0, b.1, b.2, 1.0) },
                ];
                let rt: &ID2D1RenderTarget = dc;
                let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
                let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: pt(0.0, 0.0), endPoint: pt(len, 0.0) };
                dc.CreateLinearGradientBrush(&props, None, &coll)?.cast()?
            }
            None => dc.CreateSolidColorBrush(&color(255, 255, 255, 0.55), None)?.cast()?,
        };
        let n = n.max(1);
        let rr = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: 0.0, top: 0.0, right: len, bottom: thick }, radiusX: thick / 2.0, radiusY: thick / 2.0 };
        if n == 1 {
            dc.FillRoundedRectangle(&rr, &brush);
            return Ok(());
        }
        // Parçalar paralelkenar; dış uçları yuvarlak hatlı maske keser.
        let factory = dc.GetFactory()?;
        let mask = factory.CreateRoundedRectangleGeometry(&rr)?;
        let path = factory.CreatePathGeometry()?;
        let sink = path.Open()?;
        let slant = thick * 0.9;
        let gap = (thick * 0.7).max(1.5);
        let cut = |j: usize| j as f32 * len / n as f32;
        for i in 0..n {
            // Sol kenar: ilk parçada düz (maskenin dışına taşar), sonra "/" kesik.
            let (tl, bl) = if i == 0 { (-thick, -thick) } else { (cut(i) + slant / 2.0 + gap / 2.0, cut(i) - slant / 2.0 + gap / 2.0) };
            let (tr, br) = if i + 1 == n { (len + thick, len + thick) } else { (cut(i + 1) + slant / 2.0 - gap / 2.0, cut(i + 1) - slant / 2.0 - gap / 2.0) };
            sink.BeginFigure(pt(tl, 0.0), D2D1_FIGURE_BEGIN_FILLED);
            sink.AddLines(&[pt(tr, 0.0), pt(br, thick), pt(bl, thick)]);
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        }
        sink.Close()?;
        let params = D2D1_LAYER_PARAMETERS1 {
            contentBounds: D2D_RECT_F { left: -1e6, top: -1e6, right: 1e6, bottom: 1e6 },
            geometricMask: std::mem::ManuallyDrop::new(Some(mask.cast()?)),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: windows::Foundation::Numerics::Matrix3x2::identity(),
            opacity: 1.0,
            opacityBrush: std::mem::ManuallyDrop::new(None),
            layerOptions: D2D1_LAYER_OPTIONS1_NONE,
        };
        dc.PushLayer(&params, None);
        dc.FillGeometry(&path, &brush, None);
        dc.PopLayer();
        let _ = std::mem::ManuallyDrop::into_inner(params.geometricMask);
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
