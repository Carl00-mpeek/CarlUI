//! Bu modül ayarlar penceresinde "listede olmayan bir arama motorunu nasıl
//! ekleriz?" sorununu çözüyor: pencere tamamen kendi çizdiği için hazır metin
//! kutusu yok; "+" düğmesi sayfanın içinde iki alanlı küçük bir form açar
//! (Ad, Adres). Klavye WM_CHAR ile buraya gelir: yazma, Geri, Tab (alan
//! değiştir), Ctrl+V (yapıştır), Enter (ekle), Esc (vazgeç). Adreste `{q}`
//! yoksa ve "=" ile bitiyorsa sona eklenir; değilse hata gösterilir.
//! Aynı form metin genişletici kısaltması eklemek için de kullanılır
//! (Kısaltma, Metin); orada yapıştırılan satır sonları korunur.

use carlui_core::config::{Config, Snippet};
use std::time::Instant;
use windows::Win32::{
    Foundation::{HANDLE, HGLOBAL, HWND},
    System::{
        DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard},
        Memory::{GlobalLock, GlobalUnlock},
    },
};

const CF_UNICODETEXT: u32 = 13;

/// Formun ne eklediği.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    Engine,
    Snippet,
    /// Tek alanlı uygulama listeleri: bildirim engeli, pano ve genişletici hariç tutma.
    NotifyApp,
    ClipApp,
    ExpandApp,
    /// Ekran görüntüsü dosya adı şablonu (tek alan; boş = varsayılan).
    ShotName,
    /// Özel vurgu rengi ("#RRGGBB"; tek alan).
    AccentHex,
    /// Görünüm adı: şu anki görünüşü kaydet / kullanıcı görünümünü yeniden
    /// adlandır (tek alan; `pages::looks::submit` işler).
    LookName,
    LookRename,
    /// Fotoğraftan yapılan hayvanın adı (tek alan; `pages::dock_live_photo`
    /// işler, alan kendi satırında çizilir).
    PetName,
    /// Canlı hava: aranacak şehir adı (tek alan; `pages::dock_live_weather`
    /// işler, alan kendi satırında çizilir).
    City,
}

pub struct Input {
    pub kind: Form,
    pub name: String,
    pub url: String,
    /// 0 ad, 1 adres.
    pub field: u8,
    pub error: Option<String>,
    /// İmleç yanıp sönmesi için.
    pub since: Instant,
}

impl Input {
    pub fn new(kind: Form) -> Self {
        Self { kind, name: String::new(), url: String::new(), field: 0, error: None, since: Instant::now() }
    }

    fn cur(&mut self) -> &mut String {
        if self.field == 0 { &mut self.name } else { &mut self.url }
    }

    /// Bir karakteri işler. Dönüş: Some(true) ekle, Some(false) vazgeç, None sürüyor.
    pub fn char(&mut self, hwnd: HWND, c: u32) -> Option<bool> {
        self.since = Instant::now();
        match c {
            0x0D => return Some(true),
            0x1B => return Some(false),
            0x08 => {
                self.cur().pop();
            }
            0x09 if !self.kind.single() => self.field ^= 1,
            0x16 => {
                if let Some(t) = clipboard(hwnd) {
                    // Kısaltma metninde satır sonları kalır.
                    let multi = self.kind == Form::Snippet && self.field == 1;
                    let t: String = t.replace("\r\n", "\n").chars().filter(|c| !c.is_control() || (multi && *c == '\n')).collect();
                    self.cur().push_str(if multi { &t } else { t.trim() });
                }
            }
            c if c >= 0x20 => {
                if let Some(ch) = char::from_u32(c) {
                    if self.cur().chars().count() < 400 {
                        self.cur().push(ch);
                    }
                }
            }
            _ => {}
        }
        self.error = None;
        None
    }

    /// Ayara ekler; olmazsa hatayı yazar ve false döner.
    pub fn apply(&mut self, cfg: &mut Config) -> bool {
        // Görünüm formları ayara değil görünüm listesine yazar (bkz. `pages::looks`).
        if matches!(self.kind, Form::LookName | Form::LookRename | Form::PetName | Form::City) {
            return false;
        }
        if self.kind.single() {
            return self.apply_app(cfg);
        }
        if self.kind == Form::Snippet {
            return self.apply_snippet(cfg);
        }
        let name = self.name.trim().to_string();
        let mut url = self.url.trim().to_string();
        if name.is_empty() {
            self.error = Some(carlui_core::t!("Bir ad yazın (ör. YouTube, Wikipedia).", "Enter a name (e.g. YouTube, Wikipedia).").into());
            self.field = 0;
            return false;
        }
        if !url.starts_with("http://") && !url.starts_with("https://") {
            if url.contains('.') && !url.contains(' ') {
                url = format!("https://{url}");
            } else {
                self.error = Some(carlui_core::t!("Adres http:// ya da https:// ile başlamalı.", "The address must start with http:// or https://.").into());
                self.field = 1;
                return false;
            }
        }
        if !url.contains("{q}") {
            if url.ends_with('=') {
                url.push_str("{q}");
            } else {
                self.error = Some(carlui_core::t!("Aranan metnin yerine {q} yazın (ör. https://site.com/ara?q={q}).", "Put {q} where the search text goes (e.g. https://site.com/search?q={q}).").into());
                self.field = 1;
                return false;
            }
        }
        cfg.launcher.engines.insert(name, url.clone());
        cfg.launcher.web_search = url;
        true
    }
}

impl Input {
    fn apply_snippet(&mut self, cfg: &mut Config) -> bool {
        let key = self.name.trim().to_string();
        if key.chars().count() < 2 || key.chars().any(char::is_whitespace) {
            self.error = Some(carlui_core::t!("Kısaltma en az 2 karakter ve boşluksuz olmalı (ör. ;adres).", "The abbreviation needs at least 2 characters and no spaces (e.g. ;addr).").into());
            self.field = 0;
            return false;
        }
        if self.url.trim().is_empty() {
            self.error = Some(carlui_core::t!("Yerine yazılacak metni girin.", "Enter the text to type instead.").into());
            self.field = 1;
            return false;
        }
        let text = self.url.clone();
        let list = &mut cfg.tools.snippets;
        match list.iter_mut().find(|s| s.key == key) {
            Some(s) => s.text = text,
            None => list.push(Snippet { key, text }),
        }
        cfg.tools.expander = true;
        true
    }
}

fn clipboard(hwnd: HWND) -> Option<String> {
    unsafe {
        OpenClipboard(hwnd).ok()?;
        let out = (|| {
            let h: HANDLE = GetClipboardData(CF_UNICODETEXT).ok()?;
            let g = HGLOBAL(h.0);
            let p = GlobalLock(g) as *const u16;
            if p.is_null() {
                return None;
            }
            let n = carlui_core::win::global_wide_len(g, p, 4096);
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
            let _ = GlobalUnlock(g);
            Some(s)
        })();
        let _ = CloseClipboard();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engines() {
        let mut c = Config::default();
        let mut i = Input::new(Form::Engine);
        i.name = "Wiki".into();
        i.url = "tr.wikipedia.org/w/index.php?search=".into();
        assert!(i.apply(&mut c));
        assert_eq!(c.launcher.web_search, "https://tr.wikipedia.org/w/index.php?search={q}");
        let mut j = Input::new(Form::Engine);
        j.name = "X".into();
        j.url = "https://x.com/ara".into();
        assert!(!j.apply(&mut c));
    }

    #[test]
    fn snippets() {
        let mut c = Config::default();
        let n = c.tools.snippets.len();
        let mut i = Input::new(Form::Snippet);
        i.name = "a b".into();
        i.url = "x".into();
        assert!(!i.apply(&mut c));
        i.name = ";adr".into();
        assert!(i.apply(&mut c));
        i.url = "y".into();
        assert!(i.apply(&mut c));
        assert_eq!(c.tools.snippets.len(), n + 1);
        assert_eq!(c.tools.snippets.last().unwrap().text, "y");
    }
}

impl Form {
    /// Tek alanlı form mu (yalnızca ad).
    pub fn single(self) -> bool {
        matches!(self, Form::NotifyApp | Form::ClipApp | Form::ExpandApp | Form::ShotName | Form::AccentHex | Form::LookName | Form::LookRename | Form::PetName | Form::City)
    }

    /// Onay düğmesinin yazısı.
    pub fn ok_label(self) -> &'static str {
        match self {
            Form::LookName | Form::LookRename => carlui_core::t!("Kaydet", "Save"),
            Form::PetName => carlui_core::t!("Oluştur", "Create"),
            Form::City => carlui_core::t!("Ara", "Search"),
            _ => carlui_core::t!("Ekle", "Add"),
        }
    }

    /// Klavye ipucundaki Enter açıklaması; düğme yazısıyla aynı fiil.
    pub fn ok_hint(self) -> &'static str {
        match self {
            Form::LookName | Form::LookRename => carlui_core::t!("kaydet", "save"),
            Form::PetName => carlui_core::t!("oluştur", "create"),
            Form::City => carlui_core::t!("ara", "search"),
            _ => carlui_core::t!("ekle", "add"),
        }
    }

    /// Formun altındaki klavye ipucu (Enter, onay düğmesini izler).
    pub fn keys_hint(self, multi: bool) -> String {
        let ok = self.ok_hint();
        if multi {
            format!("{} · {} · Enter: {ok} · {}", carlui_core::t!("Tab: alan değiştir", "Tab: next field"), carlui_core::t!("Ctrl+V: yapıştır", "Ctrl+V: paste"), carlui_core::t!("Esc: vazgeç", "Esc: cancel"))
        } else {
            format!("{} · Enter: {ok} · {}", carlui_core::t!("Ctrl+V: yapıştır", "Ctrl+V: paste"), carlui_core::t!("Esc: vazgeç", "Esc: cancel"))
        }
    }
}

impl Input {
    /// Uygulama adını ilgili listeye ekler (aynısı varsa eklemez).
    fn apply_app(&mut self, cfg: &mut Config) -> bool {
        if self.kind == Form::ShotName {
            cfg.shots.name = self.name.trim().to_string();
            return true;
        }
        if self.kind == Form::AccentHex {
            return self.apply_accent(cfg);
        }
        let name = self.name.trim().trim_matches('"').to_string();
        if name.is_empty() {
            self.error = Some(carlui_core::t!("Bir uygulama adı yazın.", "Enter an app name.").into());
            return false;
        }
        let list = match self.kind {
            Form::NotifyApp => &mut cfg.notify.blocked_apps,
            Form::ClipApp => &mut cfg.clipboard.excluded_apps,
            _ => &mut cfg.tools.expand_excluded,
        };
        if !list.iter().any(|a| a.eq_ignore_ascii_case(&name)) {
            list.push(name);
        }
        true
    }

    /// Yazılan renk kodunu özel vurgu yapar; geçersizse hata.
    fn apply_accent(&mut self, cfg: &mut Config) -> bool {
        match carlui_core::accent::parse_hex(&self.name) {
            Some(c) => {
                cfg.theme.custom_accent = carlui_core::accent::to_hex(c);
                cfg.theme.accent_source = carlui_core::config::AccentSource::Custom;
                true
            }
            None => {
                self.error = Some(carlui_core::t!("Renk kodu #RRGGBB biçiminde olmalı (ör. #4F8BFF).", "The color code must look like #RRGGBB (e.g. #4F8BFF).").into());
                false
            }
        }
    }
}

#[cfg(test)]
mod accent_tests {
    use super::*;
    use carlui_core::config::AccentSource;

    #[test]
    fn accent_hex_form() {
        let mut c = Config::default();
        let mut i = Input::new(Form::AccentHex);
        i.name = "mavi".into();
        assert!(!i.apply(&mut c));
        assert!(i.error.is_some());
        assert_eq!(c.theme.accent_source, AccentSource::Palette);
        i.name = " #ff8800 ".into();
        assert!(i.apply(&mut c));
        assert_eq!(c.theme.custom_accent, "#FF8800");
        assert_eq!(c.theme.accent_source, AccentSource::Custom);
    }

    /// Görünüm adı formları ayara hiçbir şey yazmaz (uygulama listelerine düşmez).
    #[test]
    fn look_forms_leave_config() {
        for kind in [Form::LookName, Form::LookRename] {
            let mut c = Config::default();
            let before = (c.notify.blocked_apps.len(), c.clipboard.excluded_apps.len(), c.tools.expand_excluded.len());
            let mut i = Input::new(kind);
            i.name = "Gece".into();
            assert!(!i.apply(&mut c));
            assert_eq!(before, (c.notify.blocked_apps.len(), c.clipboard.excluded_apps.len(), c.tools.expand_excluded.len()));
            assert!(kind.single());
            assert_eq!(kind.ok_label(), carlui_core::t!("Kaydet", "Save"));
        }
    }

    /// İpucundaki Enter fiili onay düğmesiyle aynı olmalı.
    #[test]
    fn hint_follows_ok_label() {
        for kind in [Form::Engine, Form::Snippet, Form::NotifyApp, Form::AccentHex, Form::LookName, Form::LookRename, Form::PetName, Form::City] {
            let hint = kind.keys_hint(!kind.single());
            assert!(hint.contains(&format!("Enter: {}", kind.ok_label().to_lowercase())), "{kind:?}: {hint}");
        }
        assert!(Form::LookName.keys_hint(false).contains(carlui_core::t!("Enter: kaydet", "Enter: save")));
        assert!(!Form::LookRename.keys_hint(false).contains(carlui_core::t!("ekle", "add")));
    }
}
