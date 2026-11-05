//! Bu modül "slaytı ekrana (ya da küçük resme) çizelim" sorununu çözüyor:
//! arka plan → asıl/düzen sabit şekilleri → slayt şekilleri sırasıyla çizilir.
//! Metin kutusunun paragrafları DirectWrite ile dizilir (`lay_shape`); aynı
//! dizilim düzenleyicide imleç ve seçim için de kullanılır. Ölçek `k`:
//! punto başına piksel.

use super::{Bg, Deck, Kind, Shape, Theme};
use crate::{
    img,
    rich::{Fmt, PAlign},
    tl::{self, Laid},
};
use carlui_core::t;
use carlui_ui::paint::{rgb, R};
use windows::Win32::Graphics::Direct2D::{Common::D2D_POINT_2F, *};

pub struct PLay {
    pub laid: Option<Laid>,
    /// Şeklin sol üstüne göre (px).
    pub x: f32,
    pub y: f32,
    pub h: f32,
}

pub struct ShapeLay {
    pub paras: Vec<PLay>,
    /// Metnin toplam yüksekliği (px).
    pub h: f32,
}

pub fn prompt_text(s: &Shape) -> String {
    match s.ph.as_ref().map(|p| p.ty.as_str()) {
        Some("title") => t!("Başlık eklemek için tıklayın", "Click to add title").to_string(),
        _ => t!("Metin eklemek için tıklayın", "Click to add text").to_string(),
    }
}

fn base_fmt(s: &Shape, l: usize) -> Fmt {
    let f = s.lv.fmt[l];
    Fmt { size: Some(f.size.unwrap_or(18.0) * s.scale), ..f }
}

/// Şeklin paragraflarını dizer. `prompt`: boş yer tutucuda ipucu yazısı.
pub fn lay_shape(rt: &ID2D1RenderTarget, th: &Theme, s: &Shape, k: f32, prompt: bool) -> ShapeLay {
    let family = if s.title { th.major.as_str() } else { th.minor.as_str() };
    let family = if family.is_empty() { "Calibri" } else { family };
    let inner_w = ((s.w - s.ins[0] - s.ins[2]) * k).max(4.0);
    let mut out = ShapeLay { paras: Vec::with_capacity(s.paras.len()), h: 0.0 };
    let show_prompt = prompt && s.ph.is_some() && s.is_empty();
    let mut y = 0.0;
    for (i, p) in s.paras.iter().enumerate() {
        let l = (p.level as usize).min(8);
        let mut base = base_fmt(s, l);
        let align = p.align.or(s.lv.algn[l]).unwrap_or(PAlign::Left);
        let bullet = p.bullet.as_deref().filter(|b| !b.is_empty());
        let size_px = base.size.unwrap_or(18.0) * k;
        let mar = s.lv.mar[l].unwrap_or(if bullet.is_some() { 20.0 * (l as f32 + 1.0) } else { 0.0 }) * k;
        let x = if bullet.is_some() { (mar - size_px * 1.6).max(0.0) } else { mar };
        let width = if s.wrap || align != PAlign::Left { (inner_w - x).max(4.0) } else { 100_000.0 };
        if i > 0 && !s.title {
            y += size_px * 0.2;
        }
        let laid = if show_prompt && i == 0 {
            base.color = Some(0x8A8A8A);
            let st = tl::Style { family, base, px_per_pt: k, align, width, line: 1.0 };
            tl::plain(rt, &prompt_text(s), &st)
        } else {
            let st = tl::Style { family, base, px_per_pt: k, align, width, line: if s.title { 0.9 } else { 1.0 } };
            tl::para(rt, p, &st, bullet)
        };
        let h = laid.as_ref().map(|t| t.height).unwrap_or(size_px * 1.2);
        out.paras.push(PLay { laid, x, y, h });
        y += h;
    }
    out.h = y;
    out
}

/// Metnin şekil içindeki dikey kayması (px).
pub fn text_origin(s: &Shape, lay: &ShapeLay, k: f32) -> (f32, f32) {
    let avail = (s.h - s.ins[1] - s.ins[3]) * k;
    let dy = match s.anchor {
        1 => (avail - lay.h) / 2.0,
        2 => avail - lay.h,
        _ => 0.0,
    };
    (s.ins[0] * k, s.ins[1] * k + if s.grow { dy.max(0.0) } else { dy })
}

/// Şeklin ekrandaki dikdörtgeni; metne göre uzayan kutu metni sığdırır.
pub fn shape_rect(s: &Shape, ox: f32, oy: f32, k: f32, lay: Option<&ShapeLay>) -> R {
    let mut h = s.h * k;
    if let (true, Some(l)) = (s.grow, lay) {
        h = h.max(l.h + (s.ins[1] + s.ins[3]) * k);
    }
    R::new(ox + s.x * k, oy + s.y * k, (s.w * k).max(1.0), h.max(1.0))
}

fn brush(rt: &ID2D1RenderTarget, c: u32, a: f32) -> Option<ID2D1SolidColorBrush> {
    unsafe { rt.CreateSolidColorBrush(&rgb(c, a), None).ok() }
}

pub fn fill_rect(rt: &ID2D1RenderTarget, r: R, c: u32, a: f32) {
    if let Some(b) = brush(rt, c, a) {
        unsafe { rt.FillRectangle(&r.d2d(), &b) };
    }
}

fn draw_bitmap(rt: &ID2D1RenderTarget, deck: &Deck, imgs: &mut img::Cache, path: &str, r: R) -> bool {
    let zip = &deck.zip;
    match imgs.get(rt, path, || zip.read(path)) {
        Some(bm) => {
            unsafe { rt.DrawBitmap(&bm, Some(&r.d2d()), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None) };
            true
        }
        None => false,
    }
}

fn text_color(th: &Theme) -> u32 {
    th.colors.get("tx1").or_else(|| th.colors.get("dk1")).copied().unwrap_or(0)
}

#[allow(clippy::too_many_arguments)]
pub fn draw_shape(rt: &ID2D1RenderTarget, deck: &Deck, th: &Theme, s: &Shape, ox: f32, oy: f32, k: f32, imgs: &mut img::Cache, prompt: bool) {
    let lay = if s.has_text() { Some(lay_shape(rt, th, s, k, prompt)) } else { None };
    let r = shape_rect(s, ox, oy, k, lay.as_ref());
    unsafe {
        let d = r.d2d();
        if s.geom.starts_with("line") {
            if let Some((c, w)) = s.line {
                if let Some(b) = brush(rt, c, 1.0) {
                    let (a, z) = if s.geom == "line/" { ((d.left, d.bottom), (d.right, d.top)) } else { ((d.left, d.top), (d.right, d.bottom)) };
                    rt.DrawLine(D2D_POINT_2F { x: a.0, y: a.1 }, D2D_POINT_2F { x: z.0, y: z.1 }, &b, (w * k).max(0.6), None);
                }
            }
            return;
        }
        let ell = D2D1_ELLIPSE { point: D2D_POINT_2F { x: r.x + r.w / 2.0, y: r.y + r.h / 2.0 }, radiusX: r.w / 2.0, radiusY: r.h / 2.0 };
        let rad = r.w.min(r.h) * 0.16;
        let rr = D2D1_ROUNDED_RECT { rect: d, radiusX: rad, radiusY: rad };
        if let Some(c) = s.fill {
            if let Some(b) = brush(rt, c, 1.0) {
                match s.geom.as_str() {
                    "ellipse" => rt.FillEllipse(&ell, &b),
                    "roundRect" => rt.FillRoundedRectangle(&rr, &b),
                    _ => rt.FillRectangle(&d, &b),
                }
            }
        }
        match &s.kind {
            Kind::Pic(path) => {
                if !draw_bitmap(rt, deck, imgs, path, r) {
                    fill_rect(rt, r, 0xE0E0E0, 1.0);
                }
            }
            Kind::Table(t) => super::table::draw(rt, th, t, r, k),
            Kind::Frame(what) => {
                fill_rect(rt, r, 0xF0F0F0, 1.0);
                let label = match what.as_str() {
                    "chart" => t!("Grafik", "Chart"),
                    "smartart" => t!("SmartArt", "SmartArt"),
                    _ => t!("Nesne", "Object"),
                };
                let st = tl::Style { family: "Segoe UI", base: Fmt { size: Some(12.0), ..Default::default() }, px_per_pt: k, align: PAlign::Center, width: r.w, line: 1.0 };
                if let Some(l) = tl::plain(rt, label, &st) {
                    l.draw(rt, r.x, r.y + (r.h - l.height) / 2.0, 0x707070, 1.0);
                }
            }
            Kind::Text => {}
        }
        if let Some((c, w)) = s.line {
            if let Some(b) = brush(rt, c, 1.0) {
                let w = (w * k).max(0.6);
                match s.geom.as_str() {
                    "ellipse" => rt.DrawEllipse(&ell, &b, w, None),
                    "roundRect" => rt.DrawRoundedRectangle(&rr, &b, w, None),
                    _ => rt.DrawRectangle(&d, &b, w, None),
                }
            }
        }
    }
    if let Some(lay) = lay {
        let (tx, ty) = text_origin(s, &lay, k);
        let col = text_color(th);
        unsafe { rt.PushAxisAlignedClip(&R::new(r.x - 2.0, r.y - 400.0 * k, r.w + 4.0, r.h + 800.0 * k).d2d(), D2D1_ANTIALIAS_MODE_ALIASED) };
        for p in &lay.paras {
            if let Some(l) = &p.laid {
                l.draw(rt, r.x + tx + p.x, r.y + ty + p.y, col, 1.0);
            }
        }
        unsafe { rt.PopAxisAlignedClip() };
    }
}

pub fn mix(a: u32, b: u32, t: f32) -> u32 {
    carlui_ui::paint::mix(a, b, t)
}

/// Bütün slaytı (ox, oy) noktasına `k` ölçeğiyle çizer.
/// `editing`: düzenlenen şekil ipucu yazısı göstermez.
#[allow(clippy::too_many_arguments)]
pub fn draw_slide(rt: &ID2D1RenderTarget, deck: &Deck, idx: usize, ox: f32, oy: f32, k: f32, imgs: &mut img::Cache, prompt: bool, editing: Option<usize>) {
    let Some(s) = deck.slides.get(idx) else { return };
    let area = R::new(ox, oy, deck.w * k, deck.h * k);
    let th = deck.theme_for(&s.layout);
    unsafe { rt.PushAxisAlignedClip(&area.d2d(), D2D1_ANTIALIAS_MODE_ALIASED) };
    match deck.bg(s) {
        Bg::Solid(c) => fill_rect(rt, area, c, 1.0),
        Bg::Image(p) => {
            fill_rect(rt, area, 0xFFFFFF, 1.0);
            draw_bitmap(rt, deck, imgs, &p, area);
        }
        Bg::None => fill_rect(rt, area, th.colors.get("bg1").copied().unwrap_or(0xFFFFFF), 1.0),
    }
    for sh in deck.statics(s) {
        draw_shape(rt, deck, &th, sh, ox, oy, k, imgs, false);
    }
    for (i, sh) in s.shapes.iter().enumerate() {
        draw_shape(rt, deck, &th, sh, ox, oy, k, imgs, prompt && editing != Some(i));
    }
    unsafe { rt.PopAxisAlignedClip() };
}
