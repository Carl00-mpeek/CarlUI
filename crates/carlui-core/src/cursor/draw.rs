//! İmleç çizimi: `shapes` içindeki gruplar istenen boyuta ölçeklenip D2D
//! (yazılım, WIC hedefi) ile çizilir. Işıma ve gölge CPU'da Gauss
//! bulanıklığıyla eklenir; sonuç düz (ön çarpımsız) BGRA'dır.

use super::shapes::{self, Group, Role};
use crate::config::CursorStyle;
use windows::core::{w, Result};
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;

type Rgb = (f32, f32, f32);

#[derive(Clone, Copy)]
pub struct Look {
    pub style: CursorStyle,
    /// Neon/Mürekkep/Yumuşak: gövde rengi; Zen: ışıma rengi.
    pub color: (f32, f32, f32),
    pub glow: bool,
    /// Dış kenar; None ise gövdeye göre beyaz ya da koyu.
    pub edge: Option<(f32, f32, f32)>,
}

fn pt(x: f32, y: f32) -> D2D_POINT_2F {
    D2D_POINT_2F { x, y }
}

fn col((r, g, b): (f32, f32, f32), a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
}

fn luminance((r, g, b): (f32, f32, f32)) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn to_hsl((r, g, b): (f32, f32, f32)) -> (f32, f32, f32) {
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let l = (mx + mn) / 2.0;
    if mx == mn {
        return (0.0, 0.0, l);
    }
    let d = mx - mn;
    let s = if l > 0.5 { d / (2.0 - mx - mn) } else { d / (mx + mn) };
    let h = if mx == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if mx == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, s, l)
}

fn from_hsl((h, s, l): (f32, f32, f32)) -> (f32, f32, f32) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    (r + m, g + m, b + m)
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t)
}

/// Gövde degradesinin durakları (konum, renk).
fn gradient(look: &Look) -> Vec<(f32, Rgb)> {
    let (h, s, l) = to_hsl(look.color);
    match look.style {
        CursorStyle::Ink => vec![(0.0, from_hsl((h, s, (l + 0.07).min(1.0)))), (1.0, from_hsl((h, s, (l - 0.06).max(0.0))))],
        // Neon: renkten 45° geri dönmüş, biraz koyu bir tona.
        CursorStyle::Neon => vec![(0.0, look.color), (1.0, from_hsl((h - 45.0, s, l * 0.8)))],
        // Yumuşak: neredeyse düz, alta doğru çok hafif koyulaşır.
        CursorStyle::Soft => vec![(0.0, from_hsl((h, s, (l + 0.02).min(1.0)))), (1.0, from_hsl((h, s, (l - 0.08).max(0.0))))],
        // Zen: fırçalanmış gümüş; açık-koyu bantlar, ışıma rengine hafif çalar.
        CursorStyle::Zen => [(0.0, 0.98), (0.34, 0.66), (0.52, 0.94), (0.78, 0.5), (1.0, 0.84)]
            .iter()
            .map(|&(p, v)| (p, mix((v, v, v + 0.015), look.color, 0.1)))
            .collect(),
    }
}

/// Çizgi (parmak arası, dalga, sakura ortası) rengi ve opaklığı.
fn line_color(look: &Look, edge: Rgb, body: Rgb) -> (Rgb, f32) {
    match look.style {
        CursorStyle::Zen => ((0.1, 0.11, 0.14), 0.6),
        CursorStyle::Soft => (mix(body, (0.0, 0.0, 0.0), 0.45), 0.55),
        CursorStyle::Ink => (edge, 0.35),
        CursorStyle::Neon => (edge, if luminance(edge) < 0.5 { 0.35 } else { 0.55 }),
    }
}

/// Ayrılabilir Gauss bulanıklığı (ön çarpımlı katmanda).
fn blur(src: &[[f32; 4]], n: usize, sigma: f32) -> Vec<[f32; 4]> {
    let r = (sigma * 3.0).ceil() as i32;
    let mut k: Vec<f32> = (-r..=r).map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp()).collect();
    let sum: f32 = k.iter().sum();
    k.iter_mut().for_each(|v| *v /= sum);
    let pass = |src: &[[f32; 4]], horiz: bool| {
        let mut out = vec![[0.0f32; 4]; n * n];
        for y in 0..n {
            for x in 0..n {
                let mut acc = [0.0f32; 4];
                for (j, w) in k.iter().enumerate() {
                    let o = j as i32 - r;
                    let (sx, sy) = if horiz { (x as i32 + o, y as i32) } else { (x as i32, y as i32 + o) };
                    if sx < 0 || sy < 0 || sx >= n as i32 || sy >= n as i32 {
                        continue;
                    }
                    let p = src[sy as usize * n + sx as usize];
                    for c in 0..4 {
                        acc[c] += p[c] * w;
                    }
                }
                out[y * n + x] = acc;
            }
        }
        out
    };
    pass(&pass(src, true), false)
}

/// Tek boyut için çizim kaynakları (fabrikalar, kalem, yazı biçimi).
pub struct Renderer {
    factory: ID2D1Factory,
    wic: IWICImagingFactory,
    round: ID2D1StrokeStyle,
    text: IDWriteTextFormat,
    size: u32,
}

impl Renderer {
    pub fn new(size: u32) -> Result<Self> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let round = factory.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES { startCap: D2D1_CAP_STYLE_ROUND, endCap: D2D1_CAP_STYLE_ROUND, lineJoin: D2D1_LINE_JOIN_ROUND, ..Default::default() },
                None,
            )?;
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let text = dw.CreateTextFormat(w!("Segoe UI"), None, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, 8.6, w!("tr-tr"))?;
            text.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
            text.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            Ok(Self { factory, wic: CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?, round, text, size })
        }
    }

    pub fn factory(&self) -> &ID2D1Factory {
        &self.factory
    }

    /// Tek bir katman çizer; ön çarpımlı RGBA (0..1) döner.
    fn layer(&self, draw: impl FnOnce(&ID2D1RenderTarget) -> Result<()>) -> Result<Vec<[f32; 4]>> {
        let n = self.size;
        unsafe {
            let target = self.wic.CreateBitmap(n, n, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad)?;
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            };
            let rt = self.factory.CreateWicBitmapRenderTarget(&target, &props)?;
            rt.BeginDraw();
            rt.Clear(Some(&D2D1_COLOR_F::default()));
            let k = n as f32 / 32.0;
            rt.SetTransform(&Matrix3x2 { M11: k, M12: 0.0, M21: 0.0, M22: k, M31: 0.0, M32: 0.0 });
            draw(&rt)?;
            rt.EndDraw(None, None)?;
            let mut px = vec![0u8; (n * n * 4) as usize];
            target.CopyPixels(std::ptr::null(), n * 4, &mut px)?;
            Ok(px.chunks_exact(4).map(|p| [p[2] as f32 / 255.0, p[1] as f32 / 255.0, p[0] as f32 / 255.0, p[3] as f32 / 255.0]).collect())
        }
    }

    /// Bir grubun gövde fırçası: grubun sınırlarına oturan degrade.
    unsafe fn fill(&self, rt: &ID2D1RenderTarget, g: &Group, look: &Look, grad: &[(f32, Rgb)]) -> Result<ID2D1Brush> {
        if let Some(c) = g.solid {
            return Ok(rt.CreateSolidColorBrush(&col(c, 1.0), None)?.into());
        }
        let mut b = D2D_RECT_F { left: f32::MAX, top: f32::MAX, right: f32::MIN, bottom: f32::MIN };
        for p in &g.parts {
            let r = p.GetBounds(None)?;
            b = D2D_RECT_F { left: b.left.min(r.left), top: b.top.min(r.top), right: b.right.max(r.right), bottom: b.bottom.max(r.bottom) };
        }
        let end = if matches!(look.style, CursorStyle::Ink | CursorStyle::Soft) { pt(b.left, b.bottom) } else { pt(b.right, b.bottom) };
        let stops: Vec<D2D1_GRADIENT_STOP> = grad.iter().map(|&(position, c)| D2D1_GRADIENT_STOP { position, color: col(c, 1.0) }).collect();
        let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
        Ok(rt.CreateLinearGradientBrush(&D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: pt(b.left, b.top), endPoint: end }, None, &coll)?.into())
    }

    /// `role` imlecinin `frame`. karesi: `size`×`size` düz BGRA, satırlar yukarıdan aşağı.
    pub fn render(&self, role: Role, frame: u32, look: &Look) -> Result<Vec<u8>> {
        let style = look.style;
        let groups = shapes::build(&self.factory, role, frame, style == CursorStyle::Zen)?;
        let grad = gradient(look);
        let mid = grad[grad.len() / 2].1;
        let mid = mix(grad[0].1, mid, 0.5);
        let edge = look.edge.unwrap_or(match style {
            // Yumuşak: kenar neredeyse görünmez, gövdenin biraz koyusu (açık zeminde seçilsin diye).
            CursorStyle::Soft if luminance(mid) > 0.5 => mix(mid, (0.0, 0.0, 0.0), 0.5),
            CursorStyle::Soft => mix(mid, (1.0, 1.0, 1.0), 0.25),
            CursorStyle::Zen => (0.07, 0.07, 0.09),
            CursorStyle::Ink if luminance(mid) > 0.55 => (0.09, 0.09, 0.11),
            CursorStyle::Neon if luminance(mid) > 0.8 => (0.09, 0.09, 0.11),
            _ => (1.0, 1.0, 1.0),
        });
        // Kenar kalınlığı; Yumuşak'ta gövde de kalın, yuvarlak birleşimli çizilir (köşeler yumuşar).
        let (edge_w, body_w) = match style {
            CursorStyle::Ink => (1.4 * 1.7, 0.0),
            CursorStyle::Soft => (1.5 + 1.1, 1.5),
            CursorStyle::Zen => (1.1 * 1.7, 0.0),
            CursorStyle::Neon => (1.2 * 1.7, 0.0),
        };
        let (line_c, line_a) = line_color(look, edge, mid);

        let body = self.layer(|rt| unsafe {
            let edge_b = rt.CreateSolidColorBrush(&col(edge, 1.0), None)?;
            let line_b = rt.CreateSolidColorBrush(&col(line_c, line_a), None)?;
            let text_b = if matches!(style, CursorStyle::Soft | CursorStyle::Zen) { rt.CreateSolidColorBrush(&col(line_c, 1.0), None)? } else { edge_b.clone() };
            for g in &groups {
                // Önce hepsi kenar rengiyle kalın çizilir (birleşik siluet), sonra gövde.
                // Soluk iz (dönen yay) yalnızca dış hatla çizilir, altı saydam kalır.
                for p in &g.parts {
                    if g.fill >= 1.0 {
                        rt.FillGeometry(p, &edge_b, None);
                    }
                    rt.DrawGeometry(p, &edge_b, edge_w, &self.round);
                }
                let fill = self.fill(rt, g, look, &grad)?;
                fill.SetOpacity(g.fill);
                for p in &g.parts {
                    rt.FillGeometry(p, &fill, None);
                    if body_w > 0.0 && g.fill >= 1.0 {
                        rt.DrawGeometry(p, &fill, body_w, &self.round);
                    }
                }
                if let Some(a) = &g.accent {
                    fill.SetOpacity(1.0);
                    rt.FillGeometry(a, &fill, None);
                }
                if let Some(l) = &g.lines {
                    rt.DrawGeometry(l, &line_b, 0.9, &self.round);
                }
                if let Some((s, r)) = g.text {
                    let t: Vec<u16> = s.encode_utf16().collect();
                    rt.DrawText(&t, &self.text, &r, &text_b, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                }
            }
            Ok(())
        })?;

        let n = self.size as usize;
        let k = self.size as f32 / 32.0;
        // Gölge: gövdenin alfası `dy` birim aşağı kaydırılıp bulanıklaştırılır.
        let shadow = |dy: f32, sigma: f32, a: f32| {
            let dy = (dy * k).round() as usize;
            let mut sh = vec![[0.0f32; 4]; n * n];
            for y in dy..n {
                for x in 0..n {
                    sh[y * n + x] = [0.0, 0.0, 0.0, body[(y - dy) * n + x][3] * a];
                }
            }
            blur(&sh, n, sigma * k)
        };
        let glow = |width: f32, sigma: f32, a: f32| -> Result<Vec<[f32; 4]>> {
            let g = self.layer(|rt| unsafe {
                let b = rt.CreateSolidColorBrush(&col(look.color, 1.0), None)?;
                for g in &groups {
                    for p in &g.parts {
                        rt.FillGeometry(p, &b, None);
                        rt.DrawGeometry(p, &b, width, &self.round);
                    }
                }
                Ok(())
            })?;
            let mut g = blur(&g, n, sigma * k);
            g.iter_mut().for_each(|p| p.iter_mut().for_each(|c| *c *= a));
            Ok(g)
        };
        let effect = match style {
            CursorStyle::Ink => Some(shadow(1.0, 1.0, 0.45)),
            CursorStyle::Soft => Some(shadow(1.3, 1.5, 0.42)),
            CursorStyle::Neon if look.glow => Some(glow(3.6, 1.35, 0.6)?),
            CursorStyle::Zen if look.glow => Some(glow(3.0, 1.25, 0.55)?),
            CursorStyle::Zen => Some(shadow(1.0, 1.1, 0.35)),
            CursorStyle::Neon => None,
        };

        let mut out = Vec::with_capacity(n * n * 4);
        for i in 0..n * n {
            let b = body[i];
            let p = match &effect {
                Some(e) => {
                    let e = e[i];
                    let t = 1.0 - b[3];
                    [b[0] + e[0] * t, b[1] + e[1] * t, b[2] + e[2] * t, b[3] + e[3] * t]
                }
                None => b,
            };
            let a = p[3].clamp(0.0, 1.0);
            let un = |c: f32| if a > 0.0 { ((c / a).clamp(0.0, 1.0) * 255.0).round() as u8 } else { 0 };
            out.extend_from_slice(&[un(p[2]), un(p[1]), un(p[0]), (a * 255.0).round() as u8]);
        }
        Ok(out)
    }
}
