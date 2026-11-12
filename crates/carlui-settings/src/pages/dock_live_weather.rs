//! Canlı dock bölümündeki "Canlı hava" alt bölümü: sahne gerçek havayı
//! izlesin, şehir arama (Open-Meteo konum arama; arka plan iş parçacığında),
//! gece/gündüz döngüsü ve kaynak notu. Gizlilik açıklaması başlığın "i"
//! baloncuğunda. Hava çekme işi dock'tadır; burada yalnızca ayar yazılır.

use crate::{
    input::{Form, Input},
    ui::{Flow, P},
    Act, App,
};
use carlui_core::weather::{self, Place};
use carlui_ui::paint::{Align, ORANGE, R, TEXT};
use std::{
    cell::RefCell,
    sync::{Arc, Mutex},
};
use windows::Win32::{Foundation::HWND, Graphics::Gdi::InvalidateRect};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WeatherAct {
    /// Şehir alanına yazmaya başla.
    Edit,
    /// Yazılan şehri ara.
    Search,
    /// Sonuç listesinden `i`. yeri seç.
    Pick(usize),
}

const INFO: (&str, &str) = (
    "Açıkken yalnızca seçtiğin konumun yaklaşık koordinatları (yaklaşık 1 km'ye yuvarlanmış) ve aramada yazdığın şehir adı Open-Meteo hava servisine gönderilir. Başka hiçbir veri gönderilmez. Kapalıyken hiçbir istek yapılmaz. Bağlantı yoksa seçtiğin sahne gösterilir.",
    "While on, only the approximate coordinates of the place you chose (rounded to about 1 km) and the city name you type in the search are sent to the Open-Meteo weather service. No other data is sent. While off, no requests are made. If there is no connection, the scene you picked is shown.",
);

#[derive(Default)]
struct State {
    /// Alana yazılan metin (alan kapansa da kalır).
    query: String,
    results: Vec<Place>,
    searching: bool,
    /// En az bir arama bitti (boş sonuç mesajı için).
    searched: bool,
    /// Arka plan aramasının sonucu (pencere iş parçacığı boyamada alır).
    shared: Arc<Mutex<Option<Vec<Place>>>>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

fn a(x: WeatherAct) -> Act {
    Act::Weather(x)
}

/// "Ad, bölge, ülke"; boş ya da yinelenen parçalar atlanır.
fn place_label(name: &str, region: &str, country: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for p in [name, region, country] {
        let p = p.trim();
        if !p.is_empty() && !parts.contains(&p) {
            parts.push(p);
        }
    }
    parts.join(", ")
}

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let w = &app.cfg.dock.live.weather;
    // Biten arama sonucunu al.
    with(|s| {
        if s.searching {
            if let Some(r) = s.shared.lock().ok().and_then(|mut v| v.take()) {
                s.results = r;
                s.searching = false;
                s.searched = true;
            }
        }
    });
    f.section_info(p, t!("Canlı hava", "Live weather"), t!(INFO.0, INFO.1));
    f.toggle(
        p,
        t!("Sahne gerçek havayı izlesin", "Scene follows real weather"),
        t!(
            "Seçtiğin konumun şimdiki havasına göre yağmur, kar ya da bulut gösterilir; seçtiğin sahnenin yerine geçer. Bağlantı yoksa seçtiğin sahne gösterilir.",
            "Shows rain, snow or clouds to match the current weather at your chosen place, replacing the scene you picked. Without a connection the scene you picked is shown."
        ),
        w.enabled,
        |c| c.dock.live.weather.enabled ^= true,
    );
    if !w.enabled {
        return;
    }
    place_row(app, p, f);
    results(app, p, f);
    f.toggle(
        p,
        t!("Gece ve gündüz döngüsü", "Day and night cycle"),
        t!(
            "Açıkken gece olunca yağışın ve bulutun altında ay ve yıldızlar görünür; gündüz yağmuru cam damlaları olarak çizilir. Kapalıyken hep gündüz sayılır.",
            "When on, the moon and stars show under the rain and clouds at night; daytime rain is drawn as drops on a window pane. When off it is always treated as day."
        ),
        w.day_night,
        |c| c.dock.live.weather.day_night ^= true,
    );
    f.note(p, t!("Hava verisi: Open-Meteo.com (CC BY 4.0)", "Weather data by Open-Meteo.com (CC BY 4.0)"));
}

/// Konum satırı: seçili yer (yuvarlak koordinatlarıyla) ve şehir alanı + Ara.
fn place_row(app: &App, p: &mut P, f: &mut Flow) {
    let w = &app.cfg.dock.live.weather;
    let sub = if w.place.trim().is_empty() {
        t!("Henüz konum seçilmedi; hava gösterilmez.", "No place chosen yet; no weather is shown.").to_string()
    } else {
        format!("{} ({:.2}, {:.2})", w.place, w.lat, w.lon)
    };
    let color = w.place.trim().is_empty().then_some(ORANGE);
    let r = f.row_sub(p, t!("Konum", "Location"), "", &sub, color, 330.0);
    let (searching, empty) = with(|s| (s.searching, s.query.trim().is_empty()));
    let btn = R::new(r.right() - 16.0 - 84.0, r.y + (r.h - 34.0) / 2.0, 84.0, 34.0);
    let field = R::new(btn.x - 8.0 - 230.0, btn.y, 230.0, 34.0);
    let editing = app.input.as_ref().filter(|i| i.kind == Form::City);
    p.round(field, 6.0, 0x000000, if editing.is_some() { 0.26 } else { 0.18 });
    p.outline(field, 6.0, if editing.is_some() { p.accent } else { 0xFFFFFF }, if editing.is_some() { 0.62 } else { 0.12 });
    let tr = R::new(field.x + 10.0, field.y, field.w - 20.0, field.h);
    let val = editing.map(|i| i.name.clone()).unwrap_or_else(|| with(|s| s.query.clone()));
    if val.is_empty() {
        p.text(t!("Şehir adı", "City name"), tr, 13.0, false, TEXT, 0.3, Align::Left);
    } else {
        p.text(&val, tr, 13.0, false, TEXT, 0.95, Align::Left);
    }
    if let Some(i) = editing {
        if (i.since.elapsed().as_millis() / 530) % 2 == 0 {
            let cx = (tr.x + p.fonts.width(&val, 13.0, false) + 1.0).min(tr.right());
            p.fill(R::new(cx, field.y + 9.0, 1.5, field.h - 18.0), p.accent, 1.0);
        }
    }
    p.hit(field, a(WeatherAct::Edit));
    p.button(btn, Some('\u{E721}'), t!("Ara", "Search"), a(WeatherAct::Search), !searching && !empty, false);
}

/// Arama durumu ve sonuç listesi (her sonuç tıklanabilir bir satır).
fn results(app: &App, p: &mut P, f: &mut Flow) {
    let w = &app.cfg.dock.live.weather;
    let (searching, searched, list) = with(|s| (s.searching, s.searched, s.results.clone()));
    if searching {
        f.note(p, t!("Aranıyor…", "Searching…"));
        return;
    }
    if searched && list.is_empty() {
        f.note(p, t!("Sonuç bulunamadı ya da bağlantı kurulamadı.", "No results, or the connection failed."));
        return;
    }
    for (i, pl) in list.iter().enumerate() {
        let label = place_label(&pl.name, &pl.region, &pl.country);
        let r = f.row(p, &label, "", 40.0);
        p.hit(r, a(WeatherAct::Pick(i)));
        let chosen = (pl.lat - w.lat).abs() < 1e-6 && (pl.lon - w.lon).abs() < 1e-6;
        if chosen {
            p.glyph('\u{E73E}', R::new(r.right() - 16.0 - 20.0, r.y, 20.0, r.h.min(50.0)), 14.0, p.accent, 0.9);
        }
    }
}

/// Eylemler (pencere iş parçacığında).
pub fn act(app: &mut App, x: WeatherAct) {
    // Alana yazarken başka bir yere tıklamak yazmayı bitirir (metin zaten eşlenmiş).
    if x != WeatherAct::Edit && app.input.as_ref().is_some_and(|i| i.kind == Form::City) {
        app.input = None;
    }
    match x {
        WeatherAct::Edit => {
            let mut i = Input::new(Form::City);
            i.name = with(|s| s.query.clone());
            app.input = Some(i);
        }
        WeatherAct::Search => search(app),
        WeatherAct::Pick(i) => pick(app, i),
    }
}

/// Şehir alanında Enter: ara.
pub fn submit(app: &mut App) {
    search(app);
}

/// Alan her tuşta durumla eşlenir (Esc'te de yazılan metin kalır).
pub fn sync(app: &App) {
    if let Some(i) = app.input.as_ref().filter(|i| i.kind == Form::City) {
        let q = i.name.clone();
        with(|s| s.query = q);
    }
}

/// Aramayı arka plan iş parçacığında başlatır; pencere donmaz.
fn search(app: &mut App) {
    let start = with(|s| {
        let q = s.query.trim().to_string();
        if q.is_empty() || s.searching {
            return None;
        }
        s.searching = true;
        s.searched = false;
        s.results.clear();
        if let Ok(mut v) = s.shared.lock() {
            *v = None;
        }
        Some((q, s.shared.clone()))
    });
    let Some((query, shared)) = start else { return };
    let h = app.hwnd.0 as isize;
    let lang = if carlui_core::lang::is_tr() { "tr" } else { "en" };
    std::thread::spawn(move || {
        let found = weather::search(&query, lang);
        if let Ok(mut v) = shared.lock() {
            *v = Some(found);
        }
        unsafe {
            let _ = InvalidateRect(HWND(h as _), None, false);
        }
    });
}

fn pick(app: &mut App, i: usize) {
    let Some(pl) = with(|s| s.results.get(i).cloned()) else { return };
    let label = place_label(&pl.name, &pl.region, &pl.country);
    let (lat, lon) = (pl.lat, pl.lon);
    let l2 = label.clone();
    // Yalnızca konum alanları yazılır; ayar dosyasının geri kalanına dokunulmaz.
    let upd = carlui_core::config::update(move |c| {
        c.dock.live.weather.place = l2;
        c.dock.live.weather.lat = lat;
        c.dock.live.weather.lon = lon;
    });
    match upd {
        Some(c) => app.cfg.dock.live.weather = c.dock.live.weather,
        None => {
            let w = &mut app.cfg.dock.live.weather;
            (w.place, w.lat, w.lon) = (label, lat, lon);
            app.save();
        }
    }
    // Seçim yapıldı: liste kapanır.
    with(|s| {
        s.results.clear();
        s.searched = false;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etiket_yinelenen_ve_bos_parcalari_atlar() {
        assert_eq!(place_label("İstanbul", "İstanbul", "Türkiye"), "İstanbul, Türkiye");
        assert_eq!(place_label("Paris", "Île-de-France", "France"), "Paris, Île-de-France, France");
        assert_eq!(place_label("Monaco", "", "Monaco"), "Monaco");
        assert_eq!(place_label(" ", "", ""), "");
    }
}
