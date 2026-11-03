//! Menünün çizimi: üstte arama çubuğu (uygulama eklerken süzgeç), ortada
//! sabitlenmiş uygulama/klasör ızgarası ya da uygulama listesi, altta
//! kullanıcı adı, İndirilenler / Geri Dönüşüm Kutusu ve Ayarlar / Görev
//! yöneticisi / Güç düğmeleri. Tıklanabilir
//! her şey `Act` olarak kaydedilir; sürükleme sırasında bırakılacak yer
//! vurgulanır.

use crate::{
    store::{Item, Tile},
    Act, App, View,
};
use carlui_ui::paint::{Align, Painter, R, TEXT};
use windows::Win32::Graphics::Direct2D::*;

pub type P<'a> = Painter<'a, Act>;

pub const W: f32 = 560.0;
pub const H: f32 = 490.0;
const GX: f32 = 20.0;
pub const GY: f32 = 124.0;
const COLS: usize = 6;
const CW: f32 = (W - 2.0 * GX) / COLS as f32;
const CH: f32 = 94.0;
const BAR: f32 = 60.0;
const ROW: f32 = 44.0;

/// Izgaranın / listenin kaydırılan ve kırpılan alanı.
pub fn clip() -> R {
    R::new(0.0, GY - 6.0, W, H - BAR - GY + 2.0)
}

pub fn cell(i: usize, scroll: f32) -> R {
    R::new(GX + (i % COLS) as f32 * CW, GY + (i / COLS) as f32 * CH - scroll, CW, CH)
}

/// Bırakma yeri: (araya girilecek konum, ortasına bırakılan öğe).
pub fn drop_at(pt: (f32, f32), scroll: f32, n: usize) -> (usize, Option<usize>) {
    let col = ((pt.0 - GX) / CW).floor().clamp(0.0, COLS as f32 - 1.0) as usize;
    let row = ((pt.1 - GY + scroll) / CH).floor().max(0.0) as usize;
    let i = row * COLS + col;
    let c = cell(i, scroll);
    let center = (pt.0 - (c.x + c.w / 2.0)).abs() < c.w * 0.28 && (pt.1 - (c.y + 34.0)).abs() < 26.0;
    let onto = (center && i < n).then_some(i);
    // Hücrenin sağ yarısı: arkasına.
    let slot = if pt.0 > c.x + c.w / 2.0 { i + 1 } else { i };
    (slot.min(n), onto)
}

fn clip_all() -> windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F {
    windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F { left: -1e5, top: -1e5, right: 1e5, bottom: 1e5 }
}

fn bitmap(p: &P, bmp: &ID2D1Bitmap, r: R, alpha: f32) {
    unsafe { p.rt.DrawBitmap(bmp, Some(&r.d2d()), alpha, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None) };
}

fn app_icon(app: &App, p: &P, path: &str, r: R, alpha: f32) {
    match app.icons.borrow_mut().get(p.rt, path) {
        Some(b) => bitmap(p, &b, r, alpha),
        None => p.glyph('\u{E71D}', r, r.h * 0.6, TEXT, 0.8 * alpha),
    }
}

/// Izgaradaki tek öğe: simge + ad. Klasörde ilk dört uygulamanın küçük simgeleri.
fn tile(app: &App, p: &P, r: R, item: &Item, alpha: f32) {
    let ic = R::new(r.x + (r.w - 40.0) / 2.0, r.y + 12.0, 40.0, 40.0);
    match item {
        Item::App(t) => app_icon(app, p, &t.path, ic, alpha),
        Item::Folder { apps, .. } => {
            p.round(ic.inset(-4.0), 11.0, 0xFFFFFF, 0.11 * alpha);
            p.outline(ic.inset(-4.0), 11.0, 0xFFFFFF, 0.10 * alpha);
            for (k, t) in apps.iter().take(4).enumerate() {
                let m = R::new(ic.x + 1.0 + (k % 2) as f32 * 20.0, ic.y + 1.0 + (k / 2) as f32 * 20.0, 18.0, 18.0);
                app_icon(app, p, &t.path, m, alpha);
            }
            if apps.is_empty() {
                p.glyph('\u{E8B7}', ic, 20.0, TEXT, 0.55 * alpha);
            }
        }
    }
    p.text(item.name(), R::new(r.x + 4.0, r.y + 60.0, r.w - 8.0, 22.0), 12.0, false, TEXT, 0.92 * alpha, Align::Center);
}

fn icon_button(p: &mut P, r: R, g: char, act: Act) {
    if p.hover(r) {
        p.round(r, 7.0, 0xFFFFFF, 0.10);
    }
    p.glyph(g, r, 15.0, TEXT, 0.9);
    p.hit(r, act);
}

fn small_button(p: &mut P, right: f32, g: char, label: &str, act: Act) -> f32 {
    let w = p.fonts.width(label, 13.0, false) + 44.0;
    p.button(R::new(right - w, 80.0, w, 30.0), Some(g), label, act, true, false);
    right - w - 8.0
}

/// Üstteki çubuk: normalde aramayı açar; uygulama eklerken süzgeç kutusudur.
fn top_field(app: &App, p: &mut P) {
    let r = R::new(GX, 20.0, W - 2.0 * GX, 42.0);
    let typing = matches!(app.view, View::Add(_));
    p.round(r, 21.0, 0xFFFFFF, if p.hover(r) && !typing { 0.12 } else { 0.08 });
    p.outline(r, 21.0, if typing { p.accent } else { 0xFFFFFF }, if typing { 0.8 } else { 0.10 });
    p.glyph('\u{E721}', R::new(r.x + 12.0, r.y, 24.0, r.h), 14.0, TEXT, 0.7);
    let t = R::new(r.x + 44.0, r.y, r.w - 60.0, r.h);
    if typing {
        if app.filter.is_empty() {
            p.text(t!("Uygulama ara…", "Search apps…"), t, 14.0, false, TEXT, 0.45, Align::Left);
        } else {
            p.text(&app.filter, t, 14.0, false, TEXT, 0.95, Align::Left);
        }
        if app.rename.is_none() {
            let cx = t.x + p.fonts.width(&app.filter, 14.0, false) + 1.0;
            p.fill(R::new(cx, r.y + 11.0, 1.5, r.h - 22.0), TEXT, 0.85);
        }
        p.hit(r, Act::Scroll);
    } else {
        p.text(t!("Uygulama, ayar ya da dosya ara", "Search apps, settings or files"), t, 14.0, false, TEXT, 0.5, Align::Left);
        p.hit(r, Act::Search);
    }
}

fn header(app: &App, p: &mut P) {
    let y = 80.0;
    match app.view {
        View::Home => {
            p.text(t!("Sabitlenmiş", "Pinned"), R::new(GX + 6.0, y, 200.0, 30.0), 14.5, true, TEXT, 0.95, Align::Left);
            let x = small_button(p, W - GX, '\u{E710}', t!("Ekle", "Add"), Act::Add);
            small_button(p, x, '\u{E8F4}', t!("Klasör", "Folder"), Act::NewFolder);
        }
        View::Folder(f) => {
            if app.dragging() {
                // Klasörden çıkarmak için başlığa bırak.
                let r = R::new(GX, y - 4.0, W - 2.0 * GX, 38.0);
                let over = r.contains(p.mouse) || p.mouse.1 < r.y;
                p.round(r, 9.0, p.accent, if over { 0.30 } else { 0.12 });
                p.outline(r, 9.0, p.accent, 0.7);
                p.text(t!("Klasörden çıkarmak için buraya bırak", "Drop here to remove from the folder"), r, 13.0, false, TEXT, 0.92, Align::Center);
                return;
            }
            icon_button(p, R::new(GX, y, 34.0, 30.0), '\u{E72B}', Act::Back);
            let name = app.store.items.get(f).map(|i| i.name().to_string()).unwrap_or_default();
            let nr = R::new(GX + 42.0, y, 250.0, 30.0);
            match &app.rename {
                Some(s) => {
                    p.round(nr, 6.0, 0xFFFFFF, 0.08);
                    p.outline(nr, 6.0, p.accent, 0.9);
                    let t = R::new(nr.x + 10.0, nr.y, nr.w - 16.0, nr.h);
                    p.text(s, t, 14.0, true, TEXT, 0.95, Align::Left);
                    let cx = t.x + p.fonts.width(s, 14.0, true) + 1.0;
                    p.fill(R::new(cx, nr.y + 7.0, 1.5, nr.h - 14.0), TEXT, 0.85);
                }
                None => {
                    if p.hover(nr) {
                        p.round(nr, 6.0, 0xFFFFFF, 0.06);
                    }
                    p.text(&name, R::new(nr.x + 10.0, nr.y, nr.w - 16.0, nr.h), 14.5, true, TEXT, 0.95, Align::Left);
                    p.hit(nr, Act::Rename);
                }
            }
            let x = small_button(p, W - GX, '\u{E710}', t!("Ekle", "Add"), Act::Add);
            small_button(p, x, '\u{E7A7}', t!("Dağıt", "Ungroup"), Act::Dissolve);
        }
        View::Add(target) => {
            icon_button(p, R::new(GX, y, 34.0, 30.0), '\u{E72B}', Act::Back);
            let title = match target.and_then(|f| app.store.items.get(f)) {
                Some(i) => t!(format!("“{}” klasörüne ekle", i.name()), format!("Add to “{}”", i.name())),
                None => t!("Uygulama ekle", "Add app").to_string(),
            };
            p.text(&title, R::new(GX + 46.0, y, 360.0, 30.0), 14.5, true, TEXT, 0.95, Align::Left);
            p.button(R::new(W - GX - 80.0, y, 80.0, 30.0), None, t!("Bitti", "Done"), Act::Back, true, false);
        }
    }
}

fn grid(app: &App, p: &mut P, items: &[Item], folder: Option<usize>) -> f32 {
    let clip = clip();
    let drag = app.dragging().then(|| app.press.map(|(_, i, _)| i)).flatten();
    let target = drag.map(|_| drop_at(p.mouse, app.scroll, items.len()));
    for (i, item) in items.iter().enumerate() {
        let r = cell(i, app.scroll);
        if r.bottom() < clip.y || r.y > clip.bottom() {
            continue;
        }
        let ghost = drag == Some(i);
        if !ghost && drag.is_none() && p.hover(r) && clip.contains(p.mouse) {
            p.round(r.inset(3.0), 10.0, 0xFFFFFF, 0.08);
        }
        // Ana listede uygulamayı başka öğenin üstüne bırakmak: klasör yap / içine koy.
        let merge = folder.is_none() && matches!(target, Some((_, Some(o))) if o == i) && !ghost && matches!(drag.map(|d| &items[d]), Some(Item::App(_)));
        if merge {
            p.round(r.inset(3.0), 10.0, p.accent, 0.22);
            p.outline(r.inset(3.0), 10.0, p.accent, 0.9);
        }
        tile(app, p, r, item, if ghost { 0.3 } else { 1.0 });
        p.hit(r, Act::Tile(folder, i));
    }
    let n = items.len();
    match target {
        Some((slot, onto)) if !(folder.is_none() && onto.is_some() && matches!(drag.map(|d| &items[d]), Some(Item::App(_)))) => {
            // Araya girme çizgisi.
            let r = cell(slot, app.scroll);
            let (x, y) = if slot > 0 && slot % COLS == 0 { (cell(slot - 1, app.scroll).right(), cell(slot - 1, app.scroll).y) } else { (r.x, r.y) };
            p.round(R::new(x - 1.5, y + 10.0, 3.0, 60.0), 1.5, p.accent, 0.95);
        }
        Some(_) => {}
        None => {
            // Sonda "+" kutusu: uygulama ekle.
            let r = cell(n, app.scroll);
            if r.y < clip.bottom() {
                let ic = R::new(r.x + (r.w - 44.0) / 2.0, r.y + 10.0, 44.0, 44.0);
                let hot = p.hover(r) && clip.contains(p.mouse);
                p.round(ic, 12.0, 0xFFFFFF, if hot { 0.10 } else { 0.04 });
                p.outline(ic, 12.0, 0xFFFFFF, 0.16);
                p.glyph('\u{E710}', ic, 16.0, TEXT, 0.75);
                p.text(t!("Ekle", "Add"), R::new(r.x + 4.0, r.y + 60.0, r.w - 8.0, 22.0), 12.0, false, TEXT, 0.6, Align::Center);
                p.hit(r, Act::Add);
            }
        }
    }
    ((n + 1).div_ceil(COLS)) as f32 * CH
}

fn list(app: &App, p: &mut P, target: Option<usize>) -> f32 {
    let clip = clip();
    if app.list.is_empty() {
        let msg = if app.filter.is_empty() { t!("Uygulama listesi hazırlanıyor…", "Preparing the app list…") } else { t!("Eşleşen uygulama yok", "No matching apps") };
        p.text(msg, R::new(0.0, GY + 20.0, W, 30.0), 13.0, false, TEXT, 0.55, Align::Center);
        return 0.0;
    }
    for (i, a) in app.list.iter().enumerate() {
        let r = R::new(GX, GY + i as f32 * ROW - app.scroll, W - 2.0 * GX, ROW);
        if r.bottom() < clip.y || r.y > clip.bottom() {
            continue;
        }
        if p.hover(r) && clip.contains(p.mouse) {
            p.round(r.inset(2.0), 8.0, 0xFFFFFF, 0.08);
        }
        app_icon(app, p, &a.launch_path(), R::new(r.x + 10.0, r.y + 9.0, 26.0, 26.0), 1.0);
        p.text(&a.name, R::new(r.x + 48.0, r.y, r.w - 100.0, r.h), 13.5, false, TEXT, 0.92, Align::Left);
        let c = R::new(r.right() - 36.0, r.y + 11.0, 22.0, 22.0);
        if app.store.contains(target, &Tile { name: a.name.clone(), path: a.launch_path(), args: String::new() }) {
            p.round(c, 11.0, p.accent, 1.0);
            p.glyph('\u{E73E}', c, 11.0, 0x000000, 0.85);
        } else {
            p.outline(c, 11.0, 0xFFFFFF, 0.35);
            p.glyph('\u{E710}', c, 10.0, TEXT, 0.6);
        }
        p.hit(r, Act::Pick(i));
    }
    app.list.len() as f32 * ROW
}

fn bar(p: &mut P) {
    let y = H - BAR;
    p.fill(R::new(0.0, y, W, BAR), 0x000000, 0.20);
    p.fill(R::new(0.0, y, W, 1.0), 0xFFFFFF, 0.07);
    let av = R::new(GX, y + 14.0, 32.0, 32.0);
    p.round(av, 16.0, 0xFFFFFF, 0.12);
    p.glyph('\u{E77B}', av, 14.0, TEXT, 0.85);
    let user = std::env::var("USERNAME").unwrap_or_default();
    p.text(&user, R::new(av.right() + 10.0, y, 260.0, BAR), 13.5, false, TEXT, 0.9, Align::Left);
    let b = |k: f32| R::new(W - GX - 38.0 - k * 44.0, y + 11.0, 38.0, 38.0);
    icon_button(p, b(0.0), '\u{E7E8}', Act::Power);
    icon_button(p, b(1.0), '\u{E9D9}', Act::TaskMgr);
    icon_button(p, b(2.0), '\u{E713}', Act::Settings);
    // Yerler: İndirilenler ve Geri Dönüşüm Kutusu (sistem düğmelerinden ince bir çizgiyle ayrı).
    p.fill(R::new(b(2.0).x - 10.0, y + 20.0, 1.0, 20.0), 0xFFFFFF, 0.12);
    let place = |k: f32| R::new(b(k).x - 14.0, y + 11.0, 38.0, 38.0);
    icon_button(p, place(3.0), '\u{E896}', Act::Place(0));
    icon_button(p, place(4.0), '\u{E74D}', Act::Place(1));
}

/// Bütün menüyü çizer; kaydırılabilir içeriğin yüksekliğini döner.
pub fn paint(app: &App, p: &mut P) -> f32 {
    p.fill(R::new(0.0, 0.0, W, H), 0x16161A, 0.42);
    top_field(app, p);
    // Sayfa geçişi: başlık ve içerik yana kayarak belirir.
    let (ta, dx) = app.trans();
    let mut old = windows::Foundation::Numerics::Matrix3x2::default();
    unsafe {
        p.rt.GetTransform(&mut old);
        p.rt.SetTransform(&(windows::Foundation::Numerics::Matrix3x2::translation(dx, 0.0) * old));
        // HwndRenderTarget katmansız (None) PushLayer'ı kabul etmez: gerçek katman ver.
        let layer = p.rt.CreateLayer(None).ok();
        p.rt.PushLayer(&D2D1_LAYER_PARAMETERS { contentBounds: clip_all(), maskTransform: windows::Foundation::Numerics::Matrix3x2::identity(), opacity: ta, ..Default::default() }, layer.as_ref());
    }
    header(app, p);
    let clip = clip();
    unsafe { p.rt.PushAxisAlignedClip(&clip.d2d(), D2D1_ANTIALIAS_MODE_ALIASED) };
    let start = p.hits.len();
    let content = match app.view {
        View::Home => grid(app, p, &app.store.items, None),
        View::Folder(f) => match app.store.items.get(f) {
            Some(Item::Folder { apps, .. }) => {
                let items: Vec<Item> = apps.iter().cloned().map(Item::App).collect();
                grid(app, p, &items, Some(f))
            }
            _ => 0.0,
        },
        View::Add(t) => list(app, p, t),
    };
    unsafe {
        p.rt.PopAxisAlignedClip();
        p.rt.PopLayer();
        p.rt.SetTransform(&old);
    }
    // Kırpılan (görünmeyen) kısımlar tıklanamaz.
    let tail = p.hits.split_off(start);
    p.hit(clip, Act::Scroll);
    for (r, a) in tail {
        let (top, bottom) = (r.y.max(clip.y), r.bottom().min(clip.bottom()));
        if bottom > top {
            p.hit(R::new(r.x, top, r.w, bottom - top), a);
        }
    }
    bar(p);
    // Sürüklenen öğe imleci izler.
    if let (true, Some((folder, i, _))) = (app.dragging(), app.press) {
        let item = match folder {
            None => app.store.items.get(i).cloned(),
            Some(f) => match app.store.items.get(f) {
                Some(Item::Folder { apps, .. }) => apps.get(i).cloned().map(Item::App),
                _ => None,
            },
        };
        if let Some(item) = item {
            let r = R::new(p.mouse.0 - CW / 2.0, p.mouse.1 - 32.0, CW, CH);
            p.round(r.inset(4.0), 10.0, 0x2A2A30, 0.85);
            tile(app, p, r, &item, 0.95);
        }
    }
    content
}

pub fn tile_of(app: &App, folder: Option<usize>, i: usize) -> Option<Tile> {
    match folder {
        None => match app.store.items.get(i) {
            Some(Item::App(t)) => Some(t.clone()),
            _ => None,
        },
        Some(f) => match app.store.items.get(f) {
            Some(Item::Folder { apps, .. }) => apps.get(i).cloned(),
            _ => None,
        },
    }
}
