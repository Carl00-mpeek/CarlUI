//! Canlı dock hayvan seçimindeki küçük önizlemeler. Her yerleşik hayvanın
//! dinlenme şeridinin ilk karesi exe'ye gömülüdür (dock sandığından
//! bağımsız, aynı PNG'ler); WIC ile bir kez çözülür, opak sınırına kırpılır
//! ve düğmenin soluna en yakın komşu büyütmesiyle (keskin piksel) çizilir.
//! Kullanıcı paketlerinin ve "Yok"un önizlemesi yoktur.

use crate::{
    ui::{Flow, LeadRow, P},
    Act,
};
use carlui_ui::paint::R;
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use windows::Win32::{
    Graphics::{
        Direct2D::{Common::*, *},
        Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
        Imaging::*,
    },
    System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
};

/// Önizleme kutusu (düğme iç yüksekliği 26): kedi/köpek kareleri 25-29×24
/// olduğundan 24 yüksek ve 46 geniş; hepsi 1× ya da tam sayı katıyla sığar.
const MAX_H: f32 = 24.0;
const MAX_W: f32 = 46.0;
/// Düğmede iki yandaki pay (simge ile yazı arası 6'yı `segments_lead` ekler).
const PAD: f32 = 16.0;
/// Bir seçim satırındaki en çok düğme (9 düğme sayfa genişliğine sığmaz).
pub const PER_ROW: usize = 5;

/// (kimlik, gömülü şerit, kare genişliği, kare yüksekliği). Şerit yoksa
/// dock'un durağan hâlde kullandığı ilk kare (yılan: `coil`, yunus: `swim`).
const SRC: &[(&str, &[u8], u32, u32)] = &[
    ("cat", include_bytes!("../../../carlui-dock/assets/pets/cat/idle.png"), 32, 24),
    ("cat_black", include_bytes!("../../../carlui-dock/assets/pets/cat_black/idle.png"), 32, 24),
    ("cat_gray", include_bytes!("../../../carlui-dock/assets/pets/cat_gray/idle.png"), 32, 24),
    ("cat_white", include_bytes!("../../../carlui-dock/assets/pets/cat_white/idle.png"), 32, 24),
    ("dog", include_bytes!("../../../carlui-dock/assets/pets/dog/idle.png"), 32, 24),
    ("dog_black", include_bytes!("../../../carlui-dock/assets/pets/dog_black/idle.png"), 32, 24),
    ("dog_brown", include_bytes!("../../../carlui-dock/assets/pets/dog_brown/idle.png"), 32, 24),
    ("dog_white", include_bytes!("../../../carlui-dock/assets/pets/dog_white/idle.png"), 32, 24),
    ("mouse", include_bytes!("../../../carlui-dock/assets/pets/mouse/idle.png"), 32, 24),
    ("squirrel", include_bytes!("../../../carlui-dock/assets/pets/squirrel/idle.png"), 32, 24),
    ("lizard", include_bytes!("../../../carlui-dock/assets/pets/lizard/idle.png"), 40, 20),
    ("spider", include_bytes!("../../../carlui-dock/assets/pets/spider/idle.png"), 24, 18),
    ("snake", include_bytes!("../../../carlui-dock/assets/pets/snake/coil.png"), 54, 44),
    ("dolphin", include_bytes!("../../../carlui-dock/assets/pets/dolphin/swim.png"), 48, 28),
];

/// Kırpılmış ilk kare: PBGRA (önceden çarpılmış) pikseller.
struct Thumb {
    w: u32,
    h: u32,
    px: Vec<u8>,
}

thread_local! {
    /// Çözülmüş kareler (çözülemeyen `None`); her kimlik bir kez çözülür.
    static THUMBS: RefCell<HashMap<String, Option<Rc<Thumb>>>> = RefCell::new(HashMap::new());
    /// Çizim hedefine bağlı bit eşlemler; hedef değişince atılır.
    static BITMAPS: RefCell<HashMap<String, ID2D1Bitmap>> = RefCell::new(HashMap::new());
}

/// Pencere yeni bir çizim hedefi yarattı: eski hedefin bit eşlemleri atılır.
pub fn render_target_changed() {
    BITMAPS.with(|b| b.borrow_mut().clear());
}

/// PNG → PBGRA (yalnızca gömülü, bilinen dosyalar).
fn decode_png(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    unsafe {
        let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let stream = wic.CreateStream().ok()?;
        stream.InitializeFromMemory(bytes).ok()?;
        let dec = wic.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnLoad).ok()?;
        let frame = dec.GetFrame(0).ok()?;
        let conv = wic.CreateFormatConverter().ok()?;
        conv.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut).ok()?;
        let (mut w, mut h) = (0, 0);
        conv.GetSize(&mut w, &mut h).ok()?;
        if w == 0 || h == 0 || w > 1024 || h > 256 {
            return None;
        }
        let mut px = vec![0u8; (w * h * 4) as usize];
        conv.CopyPixels(std::ptr::null(), w * 4, &mut px).ok()?;
        Some((w, h, px))
    }
}

/// Şeridin ilk karesini (`fw`×`fh`) alıp opak piksellerin sınırına kırpar.
fn first_frame(sheet: (u32, u32, Vec<u8>), fw: u32, fh: u32) -> Option<Thumb> {
    let (w, h, px) = sheet;
    if fw > w || fh > h {
        return None;
    }
    let a = |x: u32, y: u32| px[((y * w + x) * 4 + 3) as usize];
    let (mut x0, mut y0, mut x1, mut y1) = (fw, fh, 0, 0);
    for y in 0..fh {
        for x in 0..fw {
            if a(x, y) > 0 {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
            }
        }
    }
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let (tw, th) = (x1 - x0, y1 - y0);
    let mut out = Vec::with_capacity((tw * th * 4) as usize);
    for y in y0..y1 {
        let s = ((y * w + x0) * 4) as usize;
        out.extend_from_slice(&px[s..s + (tw * 4) as usize]);
    }
    Some(Thumb { w: tw, h: th, px: out })
}

fn thumb(id: &str) -> Option<Rc<Thumb>> {
    THUMBS.with(|c| {
        c.borrow_mut()
            .entry(id.to_string())
            .or_insert_with(|| {
                let &(_, bytes, fw, fh) = SRC.iter().find(|s| s.0 == id)?;
                first_frame(decode_png(bytes)?, fw, fh).map(Rc::new)
            })
            .clone()
    })
}

/// Ekranda kaplanan boyut: kutuya sığan en büyük tam sayı kat (keskin kalır);
/// 1× bile sığmıyorsa tam sayıya bölünerek küçülür.
fn fit(t: &Thumb) -> (f32, f32) {
    fit_in(t, MAX_W, MAX_H)
}

/// `fit`, verilen kutu için (karşılama turundaki büyük kartlar da kullanır).
fn fit_in(t: &Thumb, max_w: f32, max_h: f32) -> (f32, f32) {
    let (w, h) = (t.w as f32, t.h as f32);
    if h > max_h || w > max_w {
        let k = (h / max_h).max(w / max_w).ceil();
        (w / k, h / k)
    } else {
        let s = (max_h / h).min(max_w / w).floor().max(1.0);
        (w * s, h * s)
    }
}

/// Önizlemenin kapladığı genişlik; önizlemesi olmayanda (Yok, kullanıcı paketi) 0.
fn lead(id: &str) -> f32 {
    thumb(id).map_or(0.0, |t| fit(&t).0.ceil())
}

fn draw(p: &P, id: &str, r: R) {
    draw_in(p, id, r, MAX_W, MAX_H);
}

/// Hayvanın ilk karesini `r` içinde ortalar; en çok `max_w`×`max_h`, keskin
/// tam sayı katıyla. Önizlemesi olmayan kimlikte (Yok, kullanıcı paketi) hiçbir şey çizmez.
pub fn draw_in<A: Clone>(p: &carlui_ui::paint::Painter<A>, id: &str, r: R, max_w: f32, max_h: f32) {
    let Some(t) = thumb(id) else { return };
    let bmp = BITMAPS.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(b) = c.get(id) {
            return Some(b.clone());
        }
        let props = D2D1_BITMAP_PROPERTIES { pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED }, dpiX: 96.0, dpiY: 96.0 };
        let b = unsafe { p.rt.CreateBitmap(D2D_SIZE_U { width: t.w, height: t.h }, Some(t.px.as_ptr() as *const _), t.w * 4, &props) }.ok()?;
        c.insert(id.to_string(), b.clone());
        Some(b)
    });
    if let Some(b) = bmp {
        let (dw, dh) = fit_in(&t, max_w, max_h);
        let dest = R::new((r.x + (r.w - dw) / 2.0).round(), (r.y + (r.h - dh) / 2.0).round(), dw, dh);
        unsafe { p.rt.DrawBitmap(&b, Some(&dest.d2d()), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, None) };
    }
}

/// Seçim satırı; `ids[i]` düğme i'nin hayvan kimliği ("" ya da kullanıcı
/// paketi: önizleme yok). `PER_ROW`'dan çok düğme aynı kartta alt satırlara
/// iner; düğme genişliği satırın en geniş (önizleme + yazı) çiftine göre.
pub fn choice(p: &mut P, f: &mut Flow, label: &str, info: &str, items: &[(&str, Act)], ids: &[&str], selected: usize) {
    let leads: Vec<f32> = ids.iter().map(|id| lead(id)).collect();
    let rows: Vec<LeadRow> = items
        .chunks(PER_ROW)
        .zip(leads.chunks(PER_ROW))
        .enumerate()
        .map(|(i, (it, ld))| {
            let each = it.iter().zip(ld).map(|((name, _), l)| p.fonts.width(name, 13.0, true) + if *l > 0.0 { l + 6.0 } else { 0.0 } + PAD).fold(78.0_f32, f32::max).ceil();
            let selected = selected.checked_sub(i * PER_ROW).filter(|s| *s < it.len()).unwrap_or(usize::MAX);
            LeadRow { items: it, selected, each, lead_w: ld }
        })
        .collect();
    f.choice_lead_rows(p, label, info, &rows, &|p, row, i, r| draw(p, ids[row * PER_ROW + i], r));
}

#[cfg(test)]
mod tests {
    use super::*;
    use carlui_core::live::BUILTIN_PETS;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};

    #[test]
    fn her_yerlesik_hayvanin_onizlemesi_var() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        for id in BUILTIN_PETS {
            let t = thumb(id).unwrap_or_else(|| panic!("{id}: önizleme çözülemedi"));
            assert!(t.w > 0 && t.h > 0, "{id}");
            assert_eq!(t.px.len(), (t.w * t.h * 4) as usize, "{id}");
            assert!(t.px.iter().skip(3).step_by(4).any(|&a| a > 0), "{id}: tamamen saydam");
            let (dw, dh) = fit(&t);
            assert!(dw >= 1.0 && dw <= MAX_W && dh >= 1.0 && dh <= MAX_H, "{id}: {dw}x{dh}");
        }
        assert!(thumb("benim_paketim").is_none());
        assert_eq!(lead(""), 0.0);
    }

    #[test]
    fn kaynak_listesi_yerlesiklerle_ayni() {
        assert_eq!(SRC.len(), BUILTIN_PETS.len());
        assert!(BUILTIN_PETS.iter().all(|id| SRC.iter().any(|s| s.0 == *id)));
    }
}
