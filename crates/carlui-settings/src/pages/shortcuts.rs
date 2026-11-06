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
    Vault,
    ColorPicker,
    Rename,
    Disk,
    Unlock,
    Layouts,
    RecordStop,
    Replay,
    ReplayPanel,
    Dictate,
    Speak,
    Assist,
}

impl Key {
    const COMBOS: [Key; 15] = [Key::Search, Key::Clipboard, Key::Screenshot, Key::Vault, Key::ColorPicker, Key::Rename, Key::Disk, Key::Unlock, Key::Layouts, Key::RecordStop, Key::Replay, Key::ReplayPanel, Key::Dictate, Key::Speak, Key::Assist];

    pub fn label(self) -> &'static str {
        match self {
            Key::Search => t!("Arama", "Search"),
            Key::Clipboard => t!("Pano geçmişi", "Clipboard history"),
            Key::Screenshot => t!("Ekran görüntüsü", "Screenshot"),
            Key::Converter => t!("Dönüştürücü", "Converter"),
            Key::AppNumbers => t!("Dock uygulamaları", "Dock apps"),
            Key::Vault => "Carl Vault",
            Key::ColorPicker => t!("Renk seçici", "Color picker"),
            Key::Rename => t!("Toplu yeniden adlandırma", "Batch rename"),
            Key::Disk => t!("Disk analizörü", "Disk analyzer"),
            Key::Unlock => t!("Dosya kilidi açıcı", "File unlocker"),
            Key::Layouts => t!("Pencere düzenleri", "Window layouts"),
            Key::RecordStop => t!("Alan kaydını durdur", "Stop area recording"),
            Key::Replay => t!("Anlık tekrarı kaydet", "Save instant replay"),
            Key::ReplayPanel => t!("Anlık tekrar paneli", "Instant replay panel"),
            Key::Dictate => t!("Sesle yazma", "Voice typing"),
            Key::Speak => t!("Sesli okuma", "Read aloud"),
            Key::Assist => t!("Sesli asistan", "Voice assistant"),
        }
    }

    fn desc(self) -> &'static str {
        match self {
            Key::Search => t!("Adadaki aramayı açar/kapatır.", "Opens or closes search on the island."),
            Key::Clipboard => t!("Kopyalanan öğelerin listesi.", "List of copied items."),
            Key::Screenshot => t!("Ekranın bir bölgesini seçip yakalar.", "Select and capture an area of the screen."),
            Key::Converter => t!("Seçili dosyalar için; tuşlar birlikte basılıp bırakılır.", "For selected files; press the keys together and release."),
            Key::AppNumbers => t!("Bu tuşlar + 1…9 dock'taki uygulamayı açar.", "These keys + 1…9 open the app on the dock."),
            Key::Vault => t!("Şifreli kasayı açar ya da öne getirir.", "Opens the encrypted vault or brings it to the front."),
            Key::ColorPicker => t!("Ekrandaki bir pikselin rengini panoya kopyalar.", "Copies the color of a pixel on screen."),
            Key::Rename => t!("Gezgin'de seçili dosyalar için.", "For files selected in Explorer."),
            Key::Disk => t!("Diskte yer kaplayanları gösterir.", "Shows what takes up disk space."),
            Key::Unlock => t!("Gezgin'de seçili öğeyi hangi uygulamanın tuttuğunu gösterir.", "Shows which app holds the item selected in Explorer."),
            Key::Layouts => t!("Pencere düzenini kaydeder ya da kayıtlı düzeni uygular.", "Saves the window layout or applies a saved one."),
            Key::RecordStop => t!("Ekran görüntüsü aracından başlatılan kaydı bitirir.", "Ends a recording started from the screenshot tool."),
            Key::Replay => t!("Son dakikaları video olarak kaydeder (oyun içinde de).", "Saves the last minutes as a video (in games too)."),
            Key::ReplayPanel => t!("Kaydı başlatma, mikrofon ve kalite paneli; oyunun odağını almaz.", "Panel for recording, microphone and quality; never takes focus from the game."),
            Key::Dictate => t!("Bas, konuş, yeniden bas; söylediğin imlecin olduğu yere yazılır (yerel, buluta gitmez).", "Press, speak, press again; what you said is typed at the cursor (local, never sent to the cloud)."),
            Key::Speak => t!("Seçili metni (yoksa panodakini) yerel sesle okur; yeniden basınca susar.", "Reads the selected text (or the clipboard) with a local voice; press again to stop."),
            Key::Assist => t!("Basılı tutup komutu söyleyin, bırakın (\"YouTube'u aç\"). Yapay zekâ ayarlarından açılır.", "Hold, say a command, release (\"open YouTube\"). Turn it on in AI settings."),
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
            Key::Vault => s.vault.clone(),
            Key::ColorPicker => s.color_picker.clone(),
            Key::Rename => s.rename.clone(),
            Key::Disk => s.disk.clone(),
            Key::Unlock => s.unlock.clone(),
            Key::Layouts => s.layouts.clone(),
            Key::RecordStop => s.record_stop.clone(),
            Key::Replay => s.replay.clone(),
            Key::ReplayPanel => s.replay_panel.clone(),
            Key::Dictate => s.dictate.clone(),
            Key::Speak => s.speak.clone(),
            Key::Assist => s.assist.clone(),
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
            Key::Vault => s.vault = v,
            Key::ColorPicker => s.color_picker = v,
            Key::Rename => s.rename = v,
            Key::Disk => s.disk = v,
            Key::Unlock => s.unlock = v,
            Key::Layouts => s.layouts = v,
            Key::RecordStop => s.record_stop = v,
            Key::Replay => s.replay = v,
            Key::ReplayPanel => s.replay_panel = v,
            Key::Dictate => s.dictate = v,
            Key::Speak => s.speak = v,
            Key::Assist => s.assist = v,
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
                return Err(t!("Yalnızca değiştirici tuşlar olmalı (ör. Win+Alt).", "Use modifier keys only (e.g. Win+Alt).").into());
            }
            let s = recorder::mods_text(mods);
            let m = keys::parse_mods(&s)?;
            if self == Key::Converter && [m.ctrl, m.alt, m.shift, m.win].iter().filter(|b| **b).count() < 2 {
                return Err(t!("En az iki değiştirici gerekli (ör. Win+Alt).", "At least two modifiers are needed (e.g. Win+Alt).").into());
            }
            return Ok(s);
        }
        let name = recorder::vk_name(vk).ok_or(t!("Bu tuş kısayolda kullanılamaz.", "This key can't be used in a shortcut."))?;
        let s = if mods.any() { format!("{}+{name}", recorder::mods_text(mods)) } else { name };
        let combo = keys::parse_combo(&s).map_err(|e| format!("{s}: {e}"))?;
        for other in Key::COMBOS.into_iter().filter(|k| *k != self) {
            if keys::parse_combo(&other.get(cfg)).is_ok_and(|o| o == combo) {
                return Err(t!(format!("{s} zaten \"{}\" için kullanılıyor.", other.label()), format!("{s} is already used for \"{}\".", other.label())));
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
        p.text(t!("Tuşlara bas…  Esc: iptal", "Press keys…  Esc: cancel"), r, 12.5, false, TEXT, 0.9, Align::Center);
        return;
    }
    p.round(r, 7.0, 0xFFFFFF, if hot { 0.08 } else { 0.0 });
    if value.trim().is_empty() {
        p.text(t!("Kapalı", "Off"), r, 13.0, false, TEXT, 0.45, Align::Center);
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
    f.note(p, t!("Değiştirmek için tuşlara tıkla ve yeni kısayola bas. ✕ kapatır, ↺ varsayılana döndürür.", "Click the keys and press a new shortcut to change it. ✕ turns it off, ↺ restores the default."));
    f.section(p, t!("Genel", "General"));
    key_row(app, p, f, Key::Search);
    let sel = match app.cfg.shortcuts.win_key {
        WinKey::Menu => 0,
        WinKey::Search => 1,
        WinKey::Windows => 2,
    };
    f.choice(
        p,
        t!("Windows tuşu", "Windows key"),
        t!("Win'e tek başına basınca ne açılsın.", "What opens when you press Win on its own."),
        &[
            (t!("CarlUI menüsü", "CarlUI menu"), edit(|c| c.shortcuts.win_key = WinKey::Menu)),
            (t!("Arama", "Search"), edit(|c| c.shortcuts.win_key = WinKey::Search)),
            ("Windows", edit(|c| c.shortcuts.win_key = WinKey::Windows)),
        ],
        sel,
        120.0,
    );
    key_row(app, p, f, Key::Clipboard);
    key_row(app, p, f, Key::Screenshot);
    key_row(app, p, f, Key::Converter);
    key_row(app, p, f, Key::Vault);
    f.section(p, t!("Araçlar", "Tools"));
    key_row(app, p, f, Key::ColorPicker);
    key_row(app, p, f, Key::Rename);
    key_row(app, p, f, Key::Disk);
    key_row(app, p, f, Key::Unlock);
    key_row(app, p, f, Key::Layouts);
    key_row(app, p, f, Key::RecordStop);
    key_row(app, p, f, Key::Replay);
    key_row(app, p, f, Key::ReplayPanel);
    key_row(app, p, f, Key::Dictate);
    key_row(app, p, f, Key::Speak);
    key_row(app, p, f, Key::Assist);
    f.section(p, "Dock");
    key_row(app, p, f, Key::AppNumbers);
    f.flush(p);
    f.y += 6.0;
    f.note(p, t!("Fare tuşları kısayol olamaz. Değiştiricisiz tek tuş ve Windows'un temel kısayolları (Ctrl+C, Alt+Tab, Win+L …) kabul edilmez.", "Mouse buttons can't be shortcuts. A single key without modifiers and core Windows shortcuts (Ctrl+C, Alt+Tab, Win+L …) are not accepted."));
}
