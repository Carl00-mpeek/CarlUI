//! Kısayollar: her genel kısayol tuş kapakları olarak gösterilir; tıklayınca
//! kayıt başlar, basılan tuşlar `keys` doğrulayıcısından geçerse kaydedilir.
//! Fare tuşları, değiştiricisiz tek tuş, Windows'un temel kısayolları ve başka
//! bir CarlUI kısayoluyla çakışan değerler reddedilir. Her satırda "Kapat" ve
//! "Varsayılan" düğmeleri vardır.

use crate::{
    recorder,
    ui::{edit, Flow, P},
    Act, App,
};
use carlui_core::{
    config::{Config, ShortcutsConfig, WinKey},
    keys::{self, Mods},
};
use carlui_ui::paint::{Align, ORANGE, R, TEXT};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Search,
    Clipboard,
    Screenshot,
    Converter,
    AppNumbers,
}

impl Key {
    const COMBOS: [Key; 3] = [Key::Search, Key::Clipboard, Key::Screenshot];

    pub fn label(self) -> &'static str {
        match self {
            Key::Search => "Arama",
            Key::Clipboard => "Pano geçmişi",
            Key::Screenshot => "Ekran görüntüsü",
            Key::Converter => "Dönüştürücü",
            Key::AppNumbers => "Dock uygulamaları",
        }
    }

    fn desc(self) -> &'static str {
        match self {
            Key::Search => "Adadaki aramayı açar/kapatır.",
            Key::Clipboard => "Kopyalanan öğelerin listesi.",
            Key::Screenshot => "Ekranın bir bölgesini seçip yakalar.",
            Key::Converter => "Seçili dosyalar için; tuşlar birlikte basılıp bırakılır.",
            Key::AppNumbers => "Bu tuşlar + 1…9 dock'taki uygulamayı açar.",
        }
    }

    /// Yalnızca değiştiricilerden oluşan kısayol mu (ör. "Win+Alt").
    pub fn mods_only(self) -> bool {
        matches!(self, Key::Converter | Key::AppNumbers)
    }

    pub fn get(self, c: &Config) -> String {
        let s = &c.shortcuts;
        match self {
            Key::Search => s.search.clone(),
            Key::Clipboard => s.clipboard.clone(),
            Key::Screenshot => s.screenshot.clone(),
            Key::Converter => s.converter.clone(),
            Key::AppNumbers => s.app_numbers.clone(),
        }
    }

    pub fn set(self, c: &mut Config, v: String) {
        let s = &mut c.shortcuts;
        match self {
            Key::Search => s.search = v,
            Key::Clipboard => s.clipboard = v,
            Key::Screenshot => s.screenshot = v,
            Key::Converter => s.converter = v,
            Key::AppNumbers => s.app_numbers = v,
        }
    }

    pub fn default_value(self) -> String {
        let mut c = Config::default();
        c.shortcuts = ShortcutsConfig::default();
        self.get(&c)
    }

    /// Kaydedilen tuşları ayar metnine çevirir ve doğrular.
    pub fn accept(self, cfg: &Config, mods: Mods, vk: u32) -> Result<String, String> {
        if self.mods_only() {
            if vk != 0 {
                return Err("Yalnızca değiştirici tuşlar olmalı (ör. Win+Alt).".into());
            }
            let s = recorder::mods_text(mods);
            let m = keys::parse_mods(&s)?;
            if self == Key::Converter && [m.ctrl, m.alt, m.shift, m.win].iter().filter(|b| **b).count() < 2 {
                return Err("En az iki değiştirici gerekli (ör. Win+Alt).".into());
            }
            return Ok(s);
        }
        let name = recorder::vk_name(vk).ok_or("Bu tuş kısayolda kullanılamaz.")?;
        let s = if mods.any() { format!("{}+{name}", recorder::mods_text(mods)) } else { name };
        let combo = keys::parse_combo(&s).map_err(|e| format!("{s}: {e}"))?;
        for other in Key::COMBOS.into_iter().filter(|k| *k != self) {
            if keys::parse_combo(&other.get(cfg)).is_ok_and(|o| o == combo) {
                return Err(format!("{s} zaten \"{}\" için kullanılıyor.", other.label()));
            }
        }
        Ok(s)
    }
}

fn keycaps(p: &mut P, r: R, value: &str, recording: bool) {
    let hot = p.hover(r);
    if recording {
        p.round(r, 7.0, p.accent, 0.18);
        p.outline(r, 7.0, p.accent, 0.9);
        p.text("Tuşlara bas…  Esc: iptal", r, 12.5, false, TEXT, 0.9, Align::Center);
        return;
    }
    p.round(r, 7.0, 0xFFFFFF, if hot { 0.08 } else { 0.0 });
    if value.trim().is_empty() {
        p.text("Kapalı", r, 13.0, false, TEXT, 0.45, Align::Center);
        return;
    }
    let parts: Vec<&str> = value.split('+').map(str::trim).filter(|s| !s.is_empty()).collect();
    let widths: Vec<f32> = parts.iter().map(|s| p.fonts.width(s, 12.5, true) + 18.0).collect();
    let total: f32 = widths.iter().sum::<f32>() + 6.0 * (parts.len().saturating_sub(1)) as f32;
    let mut x = r.right() - 8.0 - total;
    for (s, w) in parts.iter().zip(widths) {
        let k = R::new(x, r.y + (r.h - 26.0) / 2.0, w, 26.0);
        p.round(k, 5.0, 0xFFFFFF, 0.10);
        p.outline(k, 5.0, 0xFFFFFF, 0.18);
        p.fill(R::new(k.x + 3.0, k.bottom() - 2.0, k.w - 6.0, 1.0), 0xFFFFFF, 0.12);
        p.text(s, k, 12.5, true, TEXT, 0.92, Align::Center);
        x += w + 6.0;
    }
}

fn icon_button(p: &mut P, r: R, glyph: char, act: Act) {
    if p.hover(r) {
        p.round(r, 6.0, 0xFFFFFF, 0.10);
    }
    p.glyph(glyph, r, 13.0, TEXT, 0.75);
    p.hit(r, act);
}

fn key_row(app: &App, p: &mut P, f: &mut Flow, key: Key) {
    let value = key.get(&app.cfg);
    // Hata satırın açıklamasının yerine yazılır: alttaki satırlar kaymaz.
    let error = app.error.as_ref().filter(|(k, _)| *k == key).map(|(_, e)| e.as_str());
    let r = f.row_c(p, key.label(), error.unwrap_or(key.desc()), 300.0, error.map(|_| ORANGE));
    let reset = R::new(r.right() - 12.0 - 32.0, r.y + (r.h - 32.0) / 2.0, 32.0, 32.0);
    let off = R::new(reset.x - 4.0 - 32.0, reset.y, 32.0, 32.0);
    let caps = R::new(off.x - 8.0 - 210.0, r.y + (r.h - 36.0) / 2.0, 210.0, 36.0);
    let recording = app.recording == Some(key);
    keycaps(p, caps, &value, recording);
    p.hit(caps, Act::Record(key));
    icon_button(p, off, '\u{E711}', edit(move |c| key.set(c, String::new())));
    icon_button(p, reset, '\u{E777}', edit(move |c| key.set(c, key.default_value())));
}

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    f.note(p, "Değiştirmek için tuşlara tıkla ve yeni kısayola bas. ✕ kapatır, ↺ varsayılana döndürür.");
    f.section(p, "Genel");
    key_row(app, p, f, Key::Search);
    let sel = match app.cfg.shortcuts.win_key {
        WinKey::Menu => 0,
        WinKey::Search => 1,
        WinKey::Windows => 2,
    };
    f.choice(
        p,
        "Windows tuşu",
        "Win'e tek başına basınca ne açılsın.",
        &[
            ("CarlUI menüsü", edit(|c| c.shortcuts.win_key = WinKey::Menu)),
            ("Arama", edit(|c| c.shortcuts.win_key = WinKey::Search)),
            ("Windows", edit(|c| c.shortcuts.win_key = WinKey::Windows)),
        ],
        sel,
        120.0,
    );
    key_row(app, p, f, Key::Clipboard);
    key_row(app, p, f, Key::Screenshot);
    key_row(app, p, f, Key::Converter);
    f.section(p, "Dock");
    key_row(app, p, f, Key::AppNumbers);
    f.y += 6.0;
    f.note(p, "Fare tuşları kısayol olamaz. Değiştiricisiz tek tuş ve Windows'un temel kısayolları (Ctrl+C, Alt+Tab, Win+L …) kabul edilmez.");
}
