//! İkonlar: uygulama ikonları shell'den, sistem glifleri Direct2D ile vektörel.

use windows::{
    core::*,
    Foundation::Numerics::Matrix3x2,
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
    // Windows 10 shell'i Store uygulamalarının logosunu vurgu rengindeki bir
    // plakanın üstüne küçücük çiziyor (Windows 11 plakasız sürümü seçer).
    // Paketin plakasız logosu varsa doğrudan o yüklenir.
    if !carlui_core::os::is_win11() {
        if let Some(png) = unplated_for_path(path, req) {
            if let Ok(c) = png_file(wic, &png) {
                return Ok(c);
            }
        }
    }
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

/// Paket ailesinin (AUMID'nin `!` öncesi) kurulu klasörü.
fn package_dir(family: &str) -> Option<std::path::PathBuf> {
    use windows::Win32::Storage::Packaging::Appx::{GetPackagePathByFullName, GetPackagesByPackageFamily};
    unsafe {
        let fam = HSTRING::from(family);
        let (mut count, mut len) = (0u32, 0u32);
        let _ = GetPackagesByPackageFamily(&fam, &mut count, None, &mut len, PWSTR::null());
        if count == 0 || len == 0 {
            return None;
        }
        let mut names = vec![PWSTR::null(); count as usize];
        let mut buf = vec![0u16; len as usize];
        if GetPackagesByPackageFamily(&fam, &mut count, Some(names.as_mut_ptr()), &mut len, PWSTR(buf.as_mut_ptr())) != ERROR_SUCCESS {
            return None;
        }
        let full = names.first()?;
        let mut plen = 0u32;
        let _ = GetPackagePathByFullName(PCWSTR(full.0), &mut plen, PWSTR::null());
        let mut p = vec![0u16; plen.max(1) as usize];
        if GetPackagePathByFullName(PCWSTR(full.0), &mut plen, PWSTR(p.as_mut_ptr())) != ERROR_SUCCESS {
            return None;
        }
        Some(String::from_utf16_lossy(&p[..plen.saturating_sub(1) as usize]).into())
    }
}

/// `shell:AppsFolder\Aile!Id` ya da `...\WindowsApps\<paket>\...\x.exe` yolu için
/// paketin plakasız logosu.
fn unplated_for_path(path: &str, req: i32) -> Option<std::path::PathBuf> {
    if let Some(aumid) = path.strip_prefix("shell:AppsFolder\\") {
        let (family, app) = aumid.split_once('!')?;
        return unplated_logo(&package_dir(family)?, &format!("Id=\"{app}\""), req);
    }
    // Electron gibi kendi AUMID'sini koyan paketli uygulamalarda dock exe yolunu verir.
    let lower = path.to_lowercase();
    let i = lower.find("\\windowsapps\\")? + "\\windowsapps\\".len();
    let (pkg, exe) = path[i..].split_once('\\')?;
    let dir = std::path::PathBuf::from(&path[..i]).join(pkg);
    unplated_logo(&dir, &format!("Executable=\"{exe}\""), req)
}

/// Paket klasöründeki manifestte `key` ile seçilen uygulamanın plakasız
/// (`altform-unplated`) Square44x44 logosu: `req` pikselden büyük en küçük boyut,
/// yoksa en büyüğü.
fn unplated_logo(dir: &std::path::Path, key: &str, req: i32) -> Option<std::path::PathBuf> {
    let manifest = std::fs::read_to_string(dir.join("AppxManifest.xml")).ok()?;
    // Uygulamanın kendi <Application ...> bölümündeki Square44x44Logo.
    let key_at = manifest.to_lowercase().find(&key.to_lowercase())?;
    let start = manifest[..key_at].rfind("<Application ")?;
    let rest = &manifest[start..];
    let rest = &rest[..rest.find("</Application>").unwrap_or(rest.len())];
    let attr = "Square44x44Logo=\"";
    let a = rest.find(attr)? + attr.len();
    let logo = dir.join(rest[a..].split('"').next()?.replace('\\', "/"));
    let stem = logo.file_stem()?.to_string_lossy().to_lowercase();
    let mut best: Option<(i32, std::path::PathBuf)> = None;
    for e in std::fs::read_dir(logo.parent()?).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().to_lowercase();
        let Some(size) = name
            .strip_prefix(&format!("{stem}.targetsize-"))
            .and_then(|s| s.strip_suffix("_altform-unplated.png"))
            .and_then(|s| s.parse::<i32>().ok())
        else {
            continue;
        };
        let better = match &best {
            None => true,
            Some((b, _)) => (size >= req && (*b < req || size < *b)) || (*b < req && size > *b),
        };
        if better {
            best = Some((size, e.path()));
        }
    }
    best.map(|(_, p)| p)
}

/// PNG dosyasını PBGRA olarak yükler.
fn png_file(wic: &IWICImagingFactory, path: &std::path::Path) -> Result<IWICFormatConverter> {
    unsafe {
        let dec = wic.CreateDecoderFromFilename(&HSTRING::from(path.as_os_str()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)?;
        let frame = dec.GetFrame(0)?;
        let conv = wic.CreateFormatConverter()?;
        conv.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut)?;
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

/// Çizgi göstergesinin parça biçimi (dock görünümüne göre).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineStyle {
    /// Minimal (yüzen dock): her parça kendi başına yuvarlak uçlu hap.
    Round,
    /// Compact: uçları yuvarlak tek çizgi, parçalar çapraz "/" kesikle ayrılır.
    Slant,
    /// Full: köşeleri hafif yuvarlatılmış düz çubuklar, dik kesikle ayrılır.
    Flat,
}

/// Çizgi göstergesi: `len` x `thick` boyunda bir çizgi; pencere sayısı kadar
/// (`n`) parçaya bölünür, toplam boy hep aynıdır. Parçaların biçimi `style`'a
/// göre değişir. `accent` verilirse degrade, yoksa `alpha` saydamlığında beyaz
/// (kapalı uygulamada düşük: gri görünür). `vertical`: yüzey `thick` x `len`
/// boyundadır, çizgi dikey çizilir.
pub fn draw_split_line(dc: &ID2D1DeviceContext, len: f32, thick: f32, n: usize, accent: Option<((u8, u8, u8), (u8, u8, u8))>, alpha: f32, style: LineStyle, vertical: bool) -> Result<()> {
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
            None => dc.CreateSolidColorBrush(&color(255, 255, 255, alpha), None)?.cast()?,
        };
        let n = n.max(1);
        let radius = if style == LineStyle::Flat { (thick * 0.25).max(0.5) } else { thick / 2.0 };
        let rr = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: 0.0, top: 0.0, right: len, bottom: thick }, radiusX: radius, radiusY: radius };
        if n == 1 {
            dc.FillRoundedRectangle(&rr, &brush);
            return Ok(());
        }
        if style != LineStyle::Slant {
            // Yan yana ayrı parçalar; aradaki boşluk biçime göre.
            let gap = if style == LineStyle::Round { thick.max(2.0) } else { (thick * 0.7).max(1.5) };
            let w = (len - gap * (n - 1) as f32) / n as f32;
            for i in 0..n {
                let x = i as f32 * (w + gap);
                let r = radius.min(w / 2.0);
                let seg = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: x, top: 0.0, right: x + w, bottom: thick }, radiusX: r, radiusY: r };
                dc.FillRoundedRectangle(&seg, &brush);
            }
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

/// Yığın simgesi (Hızlı Erişim): içindeki öğelerin küçük simgeleri hafif
/// eğik, birbirinin üstüne taşarak dağınık durur; içinde bir sürü şey olduğu
/// bir bakışta belli olsun. İlk öğe en üstte. En çok dört simge kullanılır.
pub fn draw_pile(dc: &ID2D1DeviceContext, srcs: &[IWICFormatConverter], px: f32) -> Result<()> {
    // (merkez x, merkez y, açı°) px oranında; arkadan öne doğru sırayla çizilir.
    const LAYOUT: [&[(f32, f32, f32)]; 4] = [
        &[(0.5, 0.5, 0.0)],
        &[(0.37, 0.41, -10.0), (0.62, 0.59, 8.0)],
        &[(0.33, 0.35, -12.0), (0.68, 0.38, 10.0), (0.5, 0.65, -4.0)],
        &[(0.31, 0.32, -11.0), (0.69, 0.31, 9.0), (0.32, 0.69, 7.0), (0.67, 0.68, -6.0)],
    ];
    let n = srcs.len().min(4);
    if n == 0 {
        return Ok(());
    }
    let size = if n == 1 { px * 0.8 } else { px * 0.58 };
    unsafe {
        let mut base = Matrix3x2::default();
        dc.GetTransform(&mut base);
        for (k, &(fx, fy, deg)) in LAYOUT[n - 1].iter().enumerate().rev() {
            let bmp = dc.CreateBitmapFromWicBitmap(&srcs[k], None)?;
            let (cx, cy) = (fx * px, fy * px);
            let (s, c) = deg.to_radians().sin_cos();
            // Merkezin çevresinde döndür, sonra yüzeyin kendi kaymasını uygula.
            let rot = Matrix3x2 { M11: c, M12: s, M21: -s, M22: c, M31: cx - cx * c + cy * s, M32: cy - cx * s - cy * c };
            dc.SetTransform(&mul(&rot, &base));
            let h = size / 2.0;
            let dest = D2D_RECT_F { left: cx - h, top: cy - h, right: cx + h, bottom: cy + h };
            dc.DrawBitmap(&bmp, Some(&dest), 1.0, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, None, None);
        }
        dc.SetTransform(&base);
    }
    Ok(())
}

/// İki 2B dönüşümün bileşkesi: önce `a`, sonra `b`.
fn mul(a: &Matrix3x2, b: &Matrix3x2) -> Matrix3x2 {
    Matrix3x2 {
        M11: a.M11 * b.M11 + a.M12 * b.M21,
        M12: a.M11 * b.M12 + a.M12 * b.M22,
        M21: a.M21 * b.M11 + a.M22 * b.M21,
        M22: a.M21 * b.M12 + a.M22 * b.M22,
        M31: a.M31 * b.M11 + a.M32 * b.M21 + b.M31,
        M32: a.M31 * b.M12 + a.M32 * b.M22 + b.M32,
    }
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
    draw_chevron_plate(dc, px)?;
    draw_chevron_mark(dc, px, stroke)
}

/// Okun arkasındaki hafif yuvarlak zemin (dock bunu yalnızca panel açıkken gösterir).
pub fn draw_chevron_plate(dc: &ID2D1DeviceContext, px: f32) -> Result<()> {
    unsafe {
        let bg = dc.CreateSolidColorBrush(&color(255, 255, 255, 0.07), None)?;
        let rr = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: 0.0, top: 0.0, right: px, bottom: px },
            radiusX: px * 0.3,
            radiusY: px * 0.3,
        };
        dc.FillRoundedRectangle(&rr, &bg);
    }
    Ok(())
}

/// Zeminsiz ok.
pub fn draw_chevron_mark(dc: &ID2D1DeviceContext, px: f32, stroke: &ID2D1StrokeStyle) -> Result<()> {
    unsafe {
        let fg = dc.CreateSolidColorBrush(&color(255, 255, 255, 0.85), None)?;
        let w = px * 0.065;
        let (l, m, r) = (px * 0.36, px * 0.5, px * 0.64);
        let (top, bottom) = (px * 0.43, px * 0.57);
        dc.DrawLine(pt(l, bottom), pt(m, top), &fg, w, stroke);
        dc.DrawLine(pt(m, top), pt(r, bottom), &fg, w, stroke);
    }
    Ok(())
}
