//! Canlı dock bölümündeki "Fotoğraftan hayvan yap" akışının çizimi ve
//! eylemleri. Kapalıyken tek satırlık bir düğme; açılınca fotoğraf, ad,
//! boyut, yön, canlı önizleme ve Oluştur/Kapat. Arka plan silme modeli
//! yoksa Yapay zekâ sayfasındaki indirme yoluyla (`carlui-convert --fetch`)
//! kurulur. Durum ve arka plan işi `dock_live_photo_job.rs`'de.

use super::dock_live_photo_job::{self as job, State, SIZES};
use crate::{
    input::{Form, Input},
    pages::Page,
    ui::{Flow, P},
    Act, App,
};
use carlui_ai::catalog;
use carlui_ui::paint::{Align, ORANGE, R, TEXT};
use std::cell::RefCell;
use windows::Win32::Graphics::{
    Direct2D::{Common::*, *},
    Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
};

/// Model yoksa önerilen arka plan silme modeli: 28 piksellik bir hayvan için
/// küçük model yeter (en hızlısı, 214 MB, MIT). Büyükleri Yapay zekâ sayfasında.
pub const MODEL_ID: &str = "birefnet-tiny";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PhotoAct {
    /// Akışı aç ve fotoğraf seçtir.
    Open,
    Close,
    Pick,
    /// Ad alanına yazmaya başla.
    Name,
    Size(u32),
    FacesLeft,
    Create,
    /// Aynı adlı paketin üzerine yaz (onay).
    Overwrite,
    /// Üzerine yazma onayından vazgeç (ad değiştirilebilir).
    KeepOld,
    /// Süren arka plan silmeyi durdur.
    Stop,
    /// Hatadan sonra yeniden dene.
    Retry,
}

const SIZE_LABELS: [&str; 9] = ["12", "16", "20", "24", "28", "32", "40", "48", "64"];

thread_local! {
    /// Önizleme bit eşlemi: (önizleme sürümü, çizim hedefi sürümü, bit eşlem).
    static BMP: RefCell<Option<(u64, u64, ID2D1Bitmap)>> = const { RefCell::new(None) };
    /// Çizim hedefi her yeniden yaratıldığında artar. Hedefin bellek adresi
    /// anahtar olamaz: yeni hedef eskisinin adresine düşebilir.
    static RT_GEN: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Pencere yeni bir çizim hedefi yarattı: önbellekteki bit eşlem atılır.
pub fn render_target_changed() {
    RT_GEN.with(|g| g.set(g.get() + 1));
    BMP.with(|c| *c.borrow_mut() = None);
}

fn a(x: PhotoAct) -> Act {
    Act::PetPhoto(x)
}

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let fetching = app.fetching.lock().map(|v| v.clone()).unwrap_or_default();
    let model = job::model_ready(fetching.len());
    job::with(|s| {
        s.poll(app.hwnd, model.is_ok());
        if s.open {
            draw_open(app, p, f, s, &model, fetching.contains(&MODEL_ID));
        } else {
            draw_closed(p, f, s);
        }
    });
}

const INFO: (&str, &str) = (
    "Bir evcil hayvan fotoğrafından dock'ta yaşayan piksel bir hayvan yapar. Her şey bu bilgisayarda olur: yapay zekâ arka planı siler, hayvan küçük bir piksel çizime dönüşür, oturma, yürüme, uyku ve zıplama kareleri kendiliğinden çıkar. Paket pets klasörüne yazılır ve seçilir.\nEn iyi sonuç: hayvanın tamamı görünen, yandan çekilmiş, sade arka planlı bir fotoğraf.",
    "Turns a photo of your pet into a pixel pet that lives on the dock. Everything happens on this computer: AI removes the background, the pet becomes a small pixel drawing, and sitting, walking, sleeping and hopping frames are made automatically. The pack is written to the pets folder and selected.\nBest results: a side-on photo showing the whole animal on a plain background.",
);

fn draw_closed(p: &mut P, f: &mut Flow, s: &State) {
    let r = f.row(p, t!("Fotoğraftan hayvan yap", "Make a pet from a photo"), t!(INFO.0, INFO.1), 170.0);
    p.button(R::new(r.right() - 16.0 - 160.0, r.y + (r.h - 32.0) / 2.0, 160.0, 32.0), Some('\u{EB9F}'), t!("Fotoğraf seç…", "Choose a photo…"), a(PhotoAct::Open), true, false);
    if let Some(d) = &s.done {
        f.note(p, &t!(format!("\"{d}\" oluşturuldu ve seçildi."), format!("\"{d}\" was created and selected.")));
    }
}

fn draw_open(app: &App, p: &mut P, f: &mut Flow, s: &State, model: &Result<(), String>, downloading: bool) {
    f.section_info(p, t!("Fotoğraftan hayvan", "Pet from a photo"), t!(INFO.0, INFO.1));
    if model.is_err() {
        model_row(p, f, downloading);
    }

    // Fotoğraf
    let file = s.photo.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| t!("Seçilmedi", "None chosen").into());
    let r = f.row_sub(p, t!("Fotoğraf", "Photo"), t!("PNG, JPEG, WebP, BMP, GIF ya da TIFF. Telefonun yön bilgisi uygulanır; büyük fotoğraflar küçültülerek okunur.", "PNG, JPEG, WebP, BMP, GIF or TIFF. The phone's orientation is applied; large photos are scaled down when read."), &file, None, 150.0);
    let busy = s.progress().is_some();
    p.button(R::new(r.right() - 16.0 - 140.0, r.y + (r.h - 32.0) / 2.0, 140.0, 32.0), Some('\u{EB9F}'), if s.photo.is_some() { t!("Değiştir…", "Change…") } else { t!("Seç…", "Choose…") }, a(PhotoAct::Pick), true, false);

    name_row(app, p, f, s);

    let sel = SIZES.iter().position(|v| *v == s.size).unwrap_or(usize::MAX);
    let items: Vec<(&str, Act)> = SIZES.iter().zip(SIZE_LABELS).map(|(v, l)| (l, a(PhotoAct::Size(*v)))).collect();
    f.choice(
        p,
        t!("Boyut", "Size"),
        t!(
            "Hayvanın en uzun kenarı, piksel çizim karesi olarak. Küçük boyut daha sade ve \"piksel\" görünür; büyük boyut daha çok ayrıntı tutar. Yerleşik kedi 28 civarındadır.",
            "The pet's longest side, in pixel-art squares. Smaller looks simpler and more \"pixel\"; larger keeps more detail. The built-in cat is about 28."
        ),
        &items,
        sel,
        40.0,
    );

    let r = f.row(
        p,
        t!("Fotoğrafta sola bakıyor", "Faces left in the photo"),
        t!("Dock'taki hayvanlar sağa bakar; fotoğraftaki hayvan sola bakıyorsa çizim aynalanır.", "Pets on the dock face right; if the animal in the photo faces left, the drawing is mirrored."),
        60.0,
    );
    p.hit(r, a(PhotoAct::FacesLeft));
    p.switch(r.right() - 16.0 - 34.0, r.y + (r.h - 18.0) / 2.0, s.faces_left, true, a(PhotoAct::FacesLeft));

    preview_card(p, f, s, model.is_ok());

    if let Some(folder) = &s.confirm {
        let r = f.row_sub(
            p,
            &t!(format!("\"{folder}\" adında bir hayvan zaten var"), format!("A pet named \"{folder}\" already exists")),
            "",
            t!("Üzerine yazılsın mı? Ya da adı değiştirin.", "Overwrite it? Or change the name."),
            Some(ORANGE),
            240.0,
        );
        let y = r.y + (r.h - 32.0) / 2.0;
        let ok = R::new(r.right() - 16.0 - 120.0, y, 120.0, 32.0);
        p.button(ok, Some('\u{E7BA}'), t!("Üzerine yaz", "Overwrite"), a(PhotoAct::Overwrite), true, true);
        p.button(R::new(ok.x - 8.0 - 100.0, y, 100.0, 32.0), None, t!("Vazgeç", "Cancel"), a(PhotoAct::KeepOld), true, false);
    }

    // Alt düğmeler: Oluştur / Kapat.
    f.flush(p);
    let can = !busy && s.confirm.is_none() && !s.name.trim().is_empty() && matches!(s.pet, Some(Ok(_)));
    let ok = R::new(f.x + f.w - 130.0, f.y, 130.0, 34.0);
    p.button(ok, Some('\u{E73E}'), t!("Oluştur", "Create"), a(PhotoAct::Create), can, false);
    p.button(R::new(ok.x - 8.0 - 110.0, f.y, 110.0, 34.0), None, t!("Kapat", "Close"), a(PhotoAct::Close), true, false);
    f.y += 34.0 + 6.0;
}

/// Model kurulu değil: boyutu ve lisansıyla "Kur" düğmesi.
fn model_row(p: &mut P, f: &mut Flow, downloading: bool) {
    let Some(m) = catalog::get(MODEL_ID) else { return };
    let desc = format!("{} · {} · {}", m.name, super::ai::mb(m.size()), m.license);
    let r = f.row_lines(
        p,
        t!("Arka plan silme modeli gerekli", "A background removal model is needed"),
        t!(
            "Fotoğraftaki hayvanı arka plandan ayırmak için bir kez indirilir (resmi kaynağından, doğrulanarak). Yapay zekâ motoru kurulu değilse o da iner (yaklaşık 12 MB). Daha büyük ve daha doğru modeller Yapay zekâ sayfasında; hangisi kuruluysa o kullanılır.",
            "Downloaded once to separate the animal from the background (from its official source, verified). If the AI engine isn't installed it's downloaded too (about 12 MB). Larger, more accurate models are on the AI page; whichever is installed is used."
        ),
        &[(&desc, None)],
        250.0,
    );
    let y = r.y + (r.h - 32.0) / 2.0;
    let x = r.right() - 16.0 - 112.0;
    if downloading {
        p.button(R::new(x, y, 112.0, 32.0), Some('\u{E896}'), t!("İndiriliyor…", "Downloading…"), Act::Scroll, false, false);
    } else {
        p.button(R::new(x, y, 112.0, 32.0), Some('\u{E896}'), t!("Kur", "Install"), Act::Model(MODEL_ID, true), true, false);
    }
    p.button(R::new(x - 8.0 - 120.0, y, 120.0, 32.0), None, t!("Diğer modeller", "Other models"), Act::Page(Page::Ai), true, false);
}

/// Ad alanı: tıklanınca sayfa içi metin girişi (`Form::PetName`) açılır.
fn name_row(app: &App, p: &mut P, f: &mut Flow, s: &State) {
    let folder = carlui_ai::petgen::pack::folder_name(&s.name);
    // Klasör adı yazılandan farklıysa (Türkçe harf, boşluk) kartta görünsün.
    let sub = if !s.name.trim().is_empty() && folder != s.name.trim() { t!(format!("Klasör: {folder}"), format!("Folder: {folder}")) } else { String::new() };
    let r = f.row_sub(
        p,
        t!("Ad", "Name"),
        t!(
            "Dock'ta ve paket listesinde görünen ad. Klasör adı bundan çıkarılır: Türkçe harfler sadeleşir, boşluklar alt çizgi olur.",
            "The name shown on the dock and in the pack list. The folder name is derived from it: accented letters are simplified, spaces become underscores."
        ),
        &sub,
        None,
        260.0,
    );
    let field = R::new(r.right() - 16.0 - 240.0, r.y + (r.h - 34.0) / 2.0, 240.0, 34.0);
    let editing = app.input.as_ref().filter(|i| i.kind == Form::PetName);
    p.round(field, 6.0, 0x000000, if editing.is_some() { 0.26 } else { 0.18 });
    p.outline(field, 6.0, if editing.is_some() { p.accent } else { 0xFFFFFF }, if editing.is_some() { 0.62 } else { 0.12 });
    let tr = R::new(field.x + 10.0, field.y, field.w - 20.0, field.h);
    let val = editing.map(|i| i.name.as_str()).unwrap_or(&s.name);
    if val.is_empty() {
        p.text(t!("ör. Pamuk", "e.g. Biscuit"), tr, 13.0, false, TEXT, 0.3, Align::Left);
    } else {
        p.text(val, tr, 13.0, false, TEXT, 0.95, Align::Left);
    }
    if let Some(i) = editing {
        if (i.since.elapsed().as_millis() / 530) % 2 == 0 {
            let cx = (tr.x + p.fonts.width(val, 13.0, false) + 1.0).min(tr.right());
            p.fill(R::new(cx, field.y + 9.0, 1.5, field.h - 18.0), p.accent, 1.0);
        }
    }
    p.hit(field, a(PhotoAct::Name));
}

/// Önizleme kartı: ilerleme, hata ya da şeritler (oturma, yürüme, uyku, zıplama).
fn preview_card(p: &mut P, f: &mut Flow, s: &State, model_ok: bool) {
    f.flush(p);
    let pad = 16.0;
    let inner = f.w - 2.0 * pad;
    let msg = |p: &mut P, f: &mut Flow, text: &str, color: u32, alpha: f32, btn: Option<(&str, PhotoAct)>| {
        let bw = if btn.is_some() { 130.0 } else { 0.0 };
        let lines = crate::ui::lines(p, text, 13.0, false, inner - bw - 12.0);
        let h = (lines.len() as f32 * 19.0 + 28.0).max(64.0);
        let card = R::new(f.x, f.y, f.w, h);
        p.card(card);
        let top = card.y + (h - lines.len() as f32 * 19.0) / 2.0;
        for (i, l) in lines.iter().enumerate() {
            p.text(l, R::new(card.x + pad, top + i as f32 * 19.0, inner - bw - 12.0, 19.0), 13.0, false, color, alpha, Align::Left);
        }
        if let Some((label, act)) = btn {
            p.button(R::new(card.right() - pad - (bw - 10.0), card.y + (h - 32.0) / 2.0, bw - 10.0, 32.0), None, label, a(act), true, false);
        }
        f.y += h + 6.0;
    };
    if let Some(frac) = s.progress() {
        let h = 84.0;
        let card = R::new(f.x, f.y, f.w, h);
        p.card(card);
        let label = t!(format!("Arka plan siliniyor… %{:.0}", frac * 100.0), format!("Removing the background… {:.0}%", frac * 100.0));
        p.text(&label, R::new(card.x + pad, card.y + 14.0, inner - 130.0, 20.0), 13.0, false, TEXT, 0.85, Align::Left);
        p.bar(R::new(card.x + pad, card.y + 50.0, inner - 130.0, 4.0), frac, p.accent);
        p.button(R::new(card.right() - pad - 110.0, card.y + (h - 32.0) / 2.0, 110.0, 32.0), Some('\u{E711}'), t!("Durdur", "Stop"), a(PhotoAct::Stop), true, false);
        f.y += h + 6.0;
        return;
    }
    if let Some(e) = &s.error {
        let retry = (s.photo.is_some() && !s.has_cut()).then_some((t!("Yeniden dene", "Try again"), PhotoAct::Retry));
        msg(p, f, e, ORANGE, 0.95, retry);
        return;
    }
    if s.photo.is_none() {
        msg(p, f, t!("Önizleme, fotoğraf seçilince burada görünür.", "The preview appears here once you choose a photo."), TEXT, 0.45, None);
        return;
    }
    if !model_ok && !s.has_cut() {
        msg(p, f, t!("Model kurulunca fotoğraf hazırlanır.", "The photo is prepared once the model is installed."), TEXT, 0.45, None);
        return;
    }
    match (&s.pet, &s.preview) {
        (Some(Err(e)), _) => msg(p, f, e, ORANGE, 0.95, None),
        (Some(Ok(pet)), Some(img)) => {
            // Tam sayı büyütme: piksel çizim bulanıklaşmasın.
            let k = ((inner / img.w as f32).min(260.0 / img.h as f32)).floor().clamp(1.0, 8.0);
            let (w, h) = (img.w as f32 * k, img.h as f32 * k);
            let ch = h + 2.0 * 14.0 + 20.0;
            let card = R::new(f.x, f.y, f.w, ch);
            p.card(card);
            let info = t!(
                format!("{}×{} piksel kare · oturma, yürüme, uyku, zıplama", pet.fw, pet.fh),
                format!("{}×{} pixel frames · sit, walk, sleep, hop", pet.fw, pet.fh)
            );
            p.text(&info, R::new(card.x + pad, card.y + 10.0, inner, 18.0), 12.0, false, TEXT, 0.55, Align::Left);
            let dest = R::new(card.x + pad, card.y + 34.0, w, h);
            draw_bitmap(p, img, s.preview_gen, dest);
            f.y += ch + 6.0;
        }
        _ => msg(p, f, t!("Hazırlanıyor…", "Preparing…"), TEXT, 0.45, None),
    }
}

/// BGRA önizlemeyi en yakın komşu büyütmesiyle çizer; bit eşlem sürüm ve
/// çizim hedefi değişmedikçe yeniden yaratılmaz.
fn draw_bitmap(p: &P, img: &carlui_ai::Image, gen: u64, dest: R) {
    let rt_key = RT_GEN.with(|g| g.get());
    let bmp = BMP.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((g, k, b)) = c.as_ref() {
            if *g == gen && *k == rt_key {
                return Some(b.clone());
            }
        }
        let props = D2D1_BITMAP_PROPERTIES { pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE }, dpiX: 96.0, dpiY: 96.0 };
        let b = unsafe { p.rt.CreateBitmap(D2D_SIZE_U { width: img.w, height: img.h }, Some(img.bgra.as_ptr() as *const _), img.w * 4, &props) }.ok()?;
        *c = Some((gen, rt_key, b.clone()));
        Some(b)
    });
    if let Some(b) = bmp {
        unsafe { p.rt.DrawBitmap(&b, Some(&dest.d2d()), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR, None) };
    }
}

/// Akış eylemleri (pencere iş parçacığında).
pub fn act(app: &mut App, x: PhotoAct) {
    // Ad yazılırken başka bir yere tıklamak yazmayı bitirir (ad zaten eşlenmiş).
    if x != PhotoAct::Name && app.input.as_ref().is_some_and(|i| i.kind == Form::PetName) {
        app.input = None;
    }
    match x {
        PhotoAct::Open => {
            job::with(|s| {
                s.open = true;
                s.done = None;
            });
            if let Some(path) = job::pick_photo() {
                job::with(|s| s.set_photo(path));
            }
        }
        PhotoAct::Pick => {
            if let Some(path) = job::pick_photo() {
                job::with(|s| s.set_photo(path));
            }
        }
        PhotoAct::Close => job::with(|s| {
            s.done = None;
            s.close();
        }),
        PhotoAct::Name => {
            let mut i = Input::new(Form::PetName);
            i.name = job::with(|s| s.name.clone());
            app.input = Some(i);
        }
        PhotoAct::Size(v) => job::with(|s| {
            s.size = v;
            s.remake();
        }),
        PhotoAct::FacesLeft => job::with(|s| {
            s.faces_left ^= true;
            s.remake();
        }),
        PhotoAct::Stop => job::with(|s| {
            s.stop();
            s.error = Some(t!("Durduruldu.", "Stopped.").into());
        }),
        PhotoAct::Retry => job::with(|s| s.error = None),
        PhotoAct::KeepOld => job::with(|s| s.confirm = None),
        PhotoAct::Create => create(app, false),
        PhotoAct::Overwrite => create(app, true),
    }
}

/// Ad formunda Enter: oluştur.
pub fn submit(app: &mut App) {
    create(app, false);
}

/// Ad formu her tuşta durumla eşlenir (Esc'te de yazılan ad kalır).
pub fn sync_name(app: &App) {
    if let Some(i) = app.input.as_ref().filter(|i| i.kind == Form::PetName) {
        let name = i.name.clone();
        job::with(|s| {
            if s.name != name {
                s.name = name;
                s.confirm = None;
            }
        });
    }
}

fn create(app: &mut App, overwrite: bool) {
    let r = job::with(|s| {
        let r = s.create(overwrite);
        if let Err(e) = &r {
            s.error = Some(e.clone());
        }
        r
    });
    let Ok(Some(folder)) = r else { return };
    // Yalnızca seçili hayvan değişir; ayar dosyasının geri kalanına dokunulmaz.
    let pick = folder.clone();
    match carlui_core::config::update(move |c| c.dock.live.pet = pick) {
        Some(c) => app.cfg.dock.live.pet = c.dock.live.pet,
        None => {
            app.cfg.dock.live.pet = folder.clone();
            app.save();
        }
    }
    super::dock_live::refresh_packs();
    job::with(|s| {
        let shown = s.name.trim().to_string();
        s.close();
        s.done = Some(shown);
    });
    app.saved = Some(std::time::Instant::now());
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::SetTimer(app.hwnd, 1, 1600, None);
    }
}
