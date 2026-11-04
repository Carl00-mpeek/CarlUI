//! Pencere durumu ve eylemler. Görünümler: kurulum (ilk parola), kurtarma
//! anahtarını gösterme, kilit, kurtarma anahtarıyla açma, yeni parola, liste,
//! düzenleyici ve güvenlik. Kasa açıkken listede yalnızca adlar vardır; bir
//! kaydın alanları "Düzenle"ye basınca çözülüp `Editor`'e konur, düzenleyici
//! kapanınca (kaydet/vazgeç/kilit) bellekten silinir.

use crate::{
    dialog,
    ui::{
        clip,
        textbox::{Key as TKey, TextBox},
    },
};
use carlui_core::{config::Config, info, t, warn};
use carlui_vault::{
    crypto::{Error, KdfParams},
    entry::{now, Attachment, Body, Category, Field, FieldKind, Meta},
    vault::{Vault, MAX_FILE},
};
use std::{path::PathBuf, time::Instant};
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{KillTimer, SetTimer},
};
use zeroize::Zeroizing;

pub const TIMER_CLIP: usize = 3;
/// En az parola uzunluğu.
const MIN_PASSWORD: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Focus {
    /// Form kutuları: 0 parola, 1 tekrar, 2 mevcut parola.
    Form(usize),
    Search,
    Name,
    Label(usize),
    Value(usize),
}

#[derive(Clone, Debug)]
pub enum Act {
    Focus(Focus),
    Create,
    ToggleRecovery,
    Unlock,
    GoRecover,
    Recover,
    BackToLock,
    SetPassword,
    RecoveryDone,
    CopyRecovery,
    Filter(Option<Category>),
    Edit(String),
    New,
    Lock,
    Security,
    SetCategory(Category),
    Reveal(usize),
    Copy(usize),
    RemoveField(usize),
    AddField(FieldKind),
    PickFile,
    ExportFile,
    RemoveFile,
    Save,
    Cancel,
    Delete,
    ChangePassword,
    NewRecovery,
    RemoveRecovery,
    OpenSettings,
    /// Kaydırılabilir alan (tekerlek hedefi, tıklanmaz).
    Scroll,
}

pub struct EdField {
    pub label: TextBox,
    pub value: TextBox,
    pub kind: FieldKind,
    /// Gizli alan şu an açık mı gösteriliyor.
    pub shown: bool,
}

pub struct Editor {
    pub id: Option<String>,
    pub name: TextBox,
    pub category: Category,
    pub fields: Vec<EdField>,
    pub file: Option<Attachment>,
    /// Kaydedilince eklenecek yeni dosya (ad, içerik).
    pub new_file: Option<(String, Zeroizing<Vec<u8>>)>,
    pub drop_file: bool,
    pub confirm_delete: bool,
}

impl Editor {
    fn fields_of(list: &[Field]) -> Vec<EdField> {
        list.iter().map(|f| EdField { label: TextBox::new(&f.label, false), value: TextBox::new(&f.value, f.kind == FieldKind::Multiline), kind: f.kind, shown: false }).collect()
    }

    fn new(category: Category) -> Self {
        Self { id: None, name: TextBox::new("", false), category, fields: Self::fields_of(&category.template()), file: None, new_file: None, drop_file: false, confirm_delete: false }
    }

    fn open(id: &str, meta: &Meta, body: &Body) -> Self {
        Self { id: Some(id.to_string()), name: TextBox::new(&meta.name, false), category: meta.category, fields: Self::fields_of(&body.fields), file: body.file.clone(), new_file: None, drop_file: false, confirm_delete: false }
    }

    fn body(&self) -> Body {
        Body {
            fields: self.fields.iter().map(|f| Field { label: f.label.as_str().trim().to_string(), value: f.value.as_str().to_string(), kind: f.kind }).collect(),
            file: if self.drop_file { None } else { self.file.clone() },
        }
    }

    fn untouched(&self) -> bool {
        self.fields.iter().all(|f| f.value.text.is_empty())
    }
}

pub enum View {
    Setup,
    /// Kurtarma anahtarı bir kez gösterilir; `security` ise Güvenlik'e döner.
    ShowRecovery { key: Zeroizing<String>, security: bool },
    Locked,
    Recover,
    NewPassword,
    List,
    Edit(Box<Editor>),
    Security,
}

pub struct App {
    pub hwnd: HWND,
    pub cfg: Config,
    pub dir: PathBuf,
    pub vault: Option<Vault>,
    pub view: View,
    pub form: [TextBox; 3],
    pub want_recovery: bool,
    pub focus: Option<Focus>,
    pub search: TextBox,
    pub filter: Option<Category>,
    pub error: Option<String>,
    pub notice: Option<(String, Instant)>,
    pub scroll: f32,
    pub max_scroll: f32,
    pub caret_since: Instant,
    pub clip_seq: Option<u32>,
    /// Art arda yanlış parola: bekleme süresi.
    fails: u32,
    pub retry_at: Option<Instant>,
    pub confirm_remove_recovery: bool,
}

fn check_new(a: &str, b: &str) -> Result<(), String> {
    if a.chars().count() < MIN_PASSWORD {
        return Err(t!(format!("Parola en az {MIN_PASSWORD} karakter olmalı."), format!("The password must be at least {MIN_PASSWORD} characters.")));
    }
    if a != b {
        return Err(t!("Parolalar aynı değil.", "The passwords don't match.").into());
    }
    Ok(())
}

impl App {
    pub fn new(hwnd: HWND, cfg: Config) -> Self {
        let dir = carlui_vault::vault::default_dir();
        let (vault, view, error) = if Vault::exists(&dir) {
            match Vault::load(&dir) {
                Ok(v) => (Some(v), View::Locked, None),
                Err(e) => {
                    warn!("kasa okunamadı: {e}");
                    (None, View::Locked, Some(e.to_string()))
                }
            }
        } else {
            (None, View::Setup, None)
        };
        Self {
            hwnd,
            cfg,
            dir,
            vault,
            view,
            form: [TextBox::new("", false), TextBox::new("", false), TextBox::new("", false)],
            want_recovery: true,
            focus: Some(Focus::Form(0)),
            search: TextBox::new("", false),
            filter: None,
            error,
            notice: None,
            scroll: 0.0,
            max_scroll: 0.0,
            caret_since: Instant::now(),
            clip_seq: None,
            fails: 0,
            retry_at: None,
            confirm_remove_recovery: false,
        }
    }

    pub fn is_open(&self) -> bool {
        self.vault.as_ref().is_some_and(|v| v.is_open())
    }

    fn clear_forms(&mut self) {
        for f in &mut self.form {
            f.clear();
        }
    }

    fn go(&mut self, v: View) {
        self.view = v;
        self.scroll = 0.0;
        self.error = None;
        self.confirm_remove_recovery = false;
        self.focus = match self.view {
            View::Setup | View::Locked | View::Recover | View::NewPassword => Some(Focus::Form(0)),
            View::Security => Some(Focus::Form(2)),
            View::Edit(ref e) if e.id.is_none() => Some(Focus::Name),
            _ => None,
        };
    }

    fn toast(&mut self, s: impl Into<String>) {
        self.notice = Some((s.into(), Instant::now()));
    }

    /// Kasayı kilitler: ana anahtar, adlar, açık düzenleyici ve formlar silinir.
    pub fn lock(&mut self, why: &str) {
        if !self.is_open() {
            return;
        }
        if let Some(v) = self.vault.as_mut() {
            v.lock();
        }
        self.clear_forms();
        self.search.clear();
        self.clear_clipboard();
        self.go(View::Locked);
        info!("kasa kilitlendi ({why})");
    }

    pub fn clear_clipboard(&mut self) {
        if let Some(seq) = self.clip_seq.take() {
            clip::clear_if(self.hwnd, seq);
        }
    }

    fn copy(&mut self, text: &str, what: &str) {
        let Some(seq) = clip::copy_secret(self.hwnd, text) else {
            self.toast(t!("Panoya kopyalanamadı.", "Couldn't copy to the clipboard."));
            return;
        };
        self.clip_seq = Some(seq);
        let secs = self.cfg.vault.clear_clipboard_seconds;
        unsafe {
            let _ = KillTimer(self.hwnd, TIMER_CLIP);
            if secs > 0 {
                SetTimer(self.hwnd, TIMER_CLIP, (secs * 1000) as u32, None);
            }
        }
        self.toast(if secs > 0 { t!(format!("{what} kopyalandı · {secs} sn sonra panodan silinecek"), format!("{what} copied · cleared from the clipboard in {secs} s")) } else { t!(format!("{what} kopyalandı"), format!("{what} copied")) });
    }

    fn fail(&mut self, e: Error) {
        if e == Error::WrongKey {
            self.fails += 1;
            // 3. hatadan sonra artan bekleme: 5, 10, 20 … sn (en çok 5 dk).
            if self.fails >= 3 {
                let secs = (5u64 << (self.fails - 3).min(6)).min(300);
                self.retry_at = Some(Instant::now() + std::time::Duration::from_secs(secs));
            }
        }
        self.error = Some(e.to_string());
    }

    pub fn wait_left(&self) -> Option<u64> {
        self.retry_at.and_then(|at| at.checked_duration_since(Instant::now())).map(|d| d.as_secs() + 1)
    }

    pub fn act(&mut self, a: Act) {
        if !matches!(a, Act::Delete) {
            if let View::Edit(e) = &mut self.view {
                e.confirm_delete = false;
            }
        }
        if !matches!(a, Act::RemoveRecovery) {
            self.confirm_remove_recovery = false;
        }
        match a {
            Act::Focus(f) => self.focus = Some(f),
            Act::Scroll => {}
            Act::ToggleRecovery => self.want_recovery ^= true,
            Act::Create => {
                if let Err(e) = check_new(self.form[0].as_str(), self.form[1].as_str()) {
                    self.error = Some(e);
                    return;
                }
                let made = KdfParams::new().and_then(|k| Vault::create(&self.dir, self.form[0].as_str(), k));
                match made {
                    Ok(mut v) => {
                        info!("yeni kasa oluşturuldu");
                        self.clear_forms();
                        let key = if self.want_recovery { v.new_recovery().ok() } else { None };
                        self.vault = Some(v);
                        match key {
                            Some(key) => self.go(View::ShowRecovery { key, security: false }),
                            None => self.go(View::List),
                        }
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            Act::Unlock => {
                if self.wait_left().is_some() {
                    return;
                }
                let Some(v) = self.vault.as_mut() else { return };
                match v.unlock(self.form[0].as_str()) {
                    Ok(()) => {
                        self.fails = 0;
                        self.retry_at = None;
                        self.clear_forms();
                        self.go(View::List);
                        info!("kasa açıldı ({} kayıt)", self.vault.as_ref().map(|v| v.items().len()).unwrap_or(0));
                    }
                    Err(e) => {
                        self.form[0].clear();
                        self.fail(e);
                    }
                }
            }
            Act::GoRecover => {
                self.clear_forms();
                self.go(View::Recover);
            }
            Act::BackToLock => {
                self.clear_forms();
                self.go(View::Locked);
            }
            Act::Recover => {
                if self.wait_left().is_some() {
                    return;
                }
                let Some(v) = self.vault.as_mut() else { return };
                match v.unlock_recovery(self.form[0].as_str()) {
                    Ok(()) => {
                        self.fails = 0;
                        self.clear_forms();
                        info!("kasa kurtarma anahtarıyla açıldı");
                        self.go(View::NewPassword);
                    }
                    Err(e) => {
                        self.fail(e);
                        if self.error.is_some() {
                            self.error = Some(t!("Kurtarma anahtarı yanlış.", "Wrong recovery key.").into());
                        }
                    }
                }
            }
            Act::SetPassword => {
                if let Err(e) = check_new(self.form[0].as_str(), self.form[1].as_str()) {
                    self.error = Some(e);
                    return;
                }
                let r = self.vault.as_mut().map(|v| v.change_password(self.form[0].as_str()));
                match r {
                    Some(Ok(())) => {
                        self.clear_forms();
                        self.go(View::List);
                        self.toast(t!("Yeni parola kaydedildi", "New password saved"));
                    }
                    Some(Err(e)) => self.error = Some(e.to_string()),
                    None => {}
                }
            }
            Act::CopyRecovery => {
                if let View::ShowRecovery { key, .. } = &self.view {
                    let k = key.clone();
                    self.copy(&k, t!("Kurtarma anahtarı", "Recovery key"));
                }
            }
            Act::RecoveryDone => {
                let back = matches!(self.view, View::ShowRecovery { security: true, .. });
                self.clear_clipboard();
                self.go(if back { View::Security } else { View::List });
            }
            Act::Filter(c) => {
                self.filter = c;
                self.scroll = 0.0;
            }
            Act::Edit(id) => {
                let Some(v) = self.vault.as_ref() else { return };
                let Some(item) = v.items().iter().find(|i| i.id == id) else { return };
                match v.decrypt(&id) {
                    Ok(body) => {
                        let e = Editor::open(&id, &item.meta, &body);
                        self.go(View::Edit(Box::new(e)));
                    }
                    Err(e) => self.toast(e.to_string()),
                }
            }
            Act::New => {
                let c = self.filter.unwrap_or(Category::Password);
                self.go(View::Edit(Box::new(Editor::new(c))));
            }
            Act::Lock => self.lock(t!("elle", "manual")),
            Act::Security => {
                self.clear_forms();
                self.go(View::Security);
            }
            Act::SetCategory(c) => {
                if let View::Edit(e) = &mut self.view {
                    if e.untouched() {
                        e.fields = Editor::fields_of(&c.template());
                    }
                    e.category = c;
                }
            }
            Act::Reveal(i) => {
                if let View::Edit(e) = &mut self.view {
                    if let Some(f) = e.fields.get_mut(i) {
                        f.shown ^= true;
                    }
                }
            }
            Act::Copy(i) => {
                let Some((v, label)) = (match &self.view {
                    View::Edit(e) => e.fields.get(i).map(|f| (Zeroizing::new(f.value.as_str().to_string()), f.label.as_str().to_string())),
                    _ => None,
                }) else {
                    return;
                };
                if !v.is_empty() {
                    self.copy(&v, &label);
                }
            }
            Act::RemoveField(i) => {
                if let View::Edit(e) = &mut self.view {
                    if i < e.fields.len() {
                        e.fields.remove(i);
                        self.focus = None;
                    }
                }
            }
            Act::AddField(kind) => {
                if let View::Edit(e) = &mut self.view {
                    let label = if kind.masked() { t!("Gizli alan", "Secret field") } else { t!("Alan", "Field") };
                    e.fields.push(EdField { label: TextBox::new(label, false), value: TextBox::new("", false), kind, shown: false });
                    self.focus = Some(Focus::Value(e.fields.len() - 1));
                }
            }
            Act::PickFile => self.pick_file(),
            Act::ExportFile => self.export_file(),
            Act::RemoveFile => {
                if let View::Edit(e) = &mut self.view {
                    e.new_file = None;
                    e.drop_file = e.file.is_some();
                }
            }
            Act::Save => self.save(),
            Act::Cancel => self.go(View::List),
            Act::Delete => {
                let View::Edit(e) = &mut self.view else { return };
                let Some(id) = e.id.clone() else {
                    self.go(View::List);
                    return;
                };
                if !e.confirm_delete {
                    e.confirm_delete = true;
                    return;
                }
                match self.vault.as_mut().map(|v| v.delete(&id)) {
                    Some(Ok(())) => {
                        self.go(View::List);
                        self.toast(t!("Kayıt silindi", "Item deleted"));
                    }
                    Some(Err(e)) => self.error = Some(e.to_string()),
                    None => {}
                }
            }
            Act::ChangePassword => {
                let Some(v) = self.vault.as_ref() else { return };
                if self.wait_left().is_some() {
                    return;
                }
                if !v.verify_password(self.form[2].as_str()) {
                    self.form[2].clear();
                    self.fail(Error::WrongKey);
                    self.error = Some(t!("Mevcut parola yanlış.", "Current password is wrong.").into());
                    return;
                }
                if let Err(e) = check_new(self.form[0].as_str(), self.form[1].as_str()) {
                    self.error = Some(e);
                    return;
                }
                match self.vault.as_mut().map(|v| v.change_password(self.form[0].as_str())) {
                    Some(Ok(())) => {
                        self.fails = 0;
                        self.clear_forms();
                        self.error = None;
                        self.toast(t!("Parola değiştirildi", "Password changed"));
                    }
                    Some(Err(e)) => self.error = Some(e.to_string()),
                    None => {}
                }
            }
            Act::NewRecovery => match self.vault.as_mut().map(|v| v.new_recovery()) {
                Some(Ok(key)) => self.go(View::ShowRecovery { key, security: true }),
                Some(Err(e)) => self.error = Some(e.to_string()),
                None => {}
            },
            Act::RemoveRecovery => {
                if !self.confirm_remove_recovery {
                    self.confirm_remove_recovery = true;
                    return;
                }
                self.confirm_remove_recovery = false;
                if let Some(Err(e)) = self.vault.as_mut().map(|v| v.remove_recovery()) {
                    self.error = Some(e.to_string());
                } else {
                    self.toast(t!("Kurtarma anahtarı kaldırıldı", "Recovery key removed"));
                }
            }
            Act::OpenSettings => {
                let exe = carlui_core::paths::sibling_exe("carlui-settings.exe");
                let _ = std::process::Command::new(exe).args(["--page", "vault"]).spawn();
            }
        }
    }

    fn save(&mut self) {
        let View::Edit(e) = &self.view else { return };
        let name = e.name.as_str().trim().to_string();
        if name.is_empty() {
            self.error = Some(t!("Kayda bir ad verin.", "Give the item a name.").into());
            self.focus = Some(Focus::Name);
            return;
        }
        let meta = Meta { name, category: e.category, updated: now() };
        let mut body = e.body();
        let (id, new_file, drop_file) = (e.id.clone(), e.new_file.as_ref().map(|(n, d)| (n.clone(), d.clone())), e.drop_file);
        let Some(v) = self.vault.as_mut() else { return };
        let r = (|| -> Result<(), Error> {
            let id = match id {
                Some(id) => {
                    v.update(&id, meta.clone(), &body)?;
                    id
                }
                None => v.add(meta.clone(), &body)?,
            };
            if drop_file {
                v.remove_file(&id);
            }
            if let Some((n, d)) = new_file {
                body.file = Some(v.attach(&id, &n, &d)?);
                v.update(&id, meta, &body)?;
            }
            Ok(())
        })();
        match r {
            Ok(()) => {
                self.go(View::List);
                self.toast(t!("Kaydedildi", "Saved"));
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn pick_file(&mut self) {
        let Some(path) = dialog::open(self.hwnd) else { return };
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if size > MAX_FILE {
            self.error = Some(t!("Dosya 25 MB'tan büyük.", "File is larger than 25 MB.").into());
            return;
        }
        match std::fs::read(&path) {
            Ok(d) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if let View::Edit(e) = &mut self.view {
                    if e.name.text.is_empty() {
                        e.name = TextBox::new(&name, false);
                    }
                    e.new_file = Some((name, Zeroizing::new(d)));
                    e.drop_file = false;
                }
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn export_file(&mut self) {
        let View::Edit(e) = &self.view else { return };
        let data = match (&e.new_file, &e.id, &e.file) {
            (Some((n, d)), _, _) => Some((n.clone(), d.clone())),
            (None, Some(id), Some(f)) => match self.vault.as_ref().map(|v| v.read_file(id)) {
                Some(Ok(d)) => Some((f.name.clone(), d)),
                Some(Err(err)) => {
                    self.error = Some(err.to_string());
                    None
                }
                None => None,
            },
            _ => None,
        };
        let Some((name, d)) = data else { return };
        let Some(path) = dialog::save(self.hwnd, &name) else { return };
        match std::fs::write(&path, &d[..]) {
            Ok(()) => self.toast(t!("Dosya dışa aktarıldı (şifresiz)", "File exported (unencrypted)")),
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    /// Bu görünümde Tab ile dolaşılan kutular.
    fn order(&self) -> Vec<Focus> {
        match &self.view {
            View::Setup | View::NewPassword => vec![Focus::Form(0), Focus::Form(1)],
            View::Locked | View::Recover => vec![Focus::Form(0)],
            View::Security => vec![Focus::Form(2), Focus::Form(0), Focus::Form(1)],
            View::List => vec![Focus::Search],
            View::Edit(e) => std::iter::once(Focus::Name).chain((0..e.fields.len()).map(Focus::Value)).collect(),
            View::ShowRecovery { .. } => vec![],
        }
    }

    pub fn textbox(&mut self, f: Focus) -> Option<&mut TextBox> {
        match (f, &mut self.view) {
            (Focus::Form(i), _) => self.form.get_mut(i),
            (Focus::Search, _) => Some(&mut self.search),
            (Focus::Name, View::Edit(e)) => Some(&mut e.name),
            (Focus::Label(i), View::Edit(e)) => e.fields.get_mut(i).map(|f| &mut f.label),
            (Focus::Value(i), View::Edit(e)) => e.fields.get_mut(i).map(|f| &mut f.value),
            _ => None,
        }
    }

    /// Klavyeden gelen karakter (WM_CHAR).
    pub fn char(&mut self, c: u32) -> bool {
        self.caret_since = Instant::now();
        // Ctrl+L: kilitle.
        if c == 0x0C && self.is_open() {
            self.lock(t!("Ctrl+L", "Ctrl+L"));
            return true;
        }
        let Some(f) = self.focus else {
            if c == 0x1B && matches!(self.view, View::Edit(_)) {
                self.act(Act::Cancel);
            }
            return c == 0x1B;
        };
        let hwnd = self.hwnd;
        let Some(tb) = self.textbox(f) else { return false };
        match tb.char(hwnd, c) {
            TKey::Changed => {
                if f == Focus::Search {
                    self.scroll = 0.0;
                }
                if !matches!(self.view, View::Edit(_)) {
                    self.error = None;
                }
            }
            TKey::Next => self.next_focus(),
            TKey::Submit => match &self.view {
                View::Setup => self.act(Act::Create),
                View::Locked => self.act(Act::Unlock),
                View::Recover => self.act(Act::Recover),
                View::NewPassword => self.act(Act::SetPassword),
                View::Security if matches!(f, Focus::Form(1)) => self.act(Act::ChangePassword),
                View::Edit(_) if crate::ui::ctrl_down() => self.act(Act::Save),
                _ => self.next_focus(),
            },
            TKey::Cancel => match &self.view {
                View::Edit(_) => self.act(Act::Cancel),
                View::Recover => self.act(Act::BackToLock),
                View::List if f == Focus::Search => {
                    self.search.clear();
                    self.focus = None;
                }
                _ => self.focus = None,
            },
            TKey::None => return false,
        }
        true
    }

    fn next_focus(&mut self) {
        let order = self.order();
        if order.is_empty() {
            return;
        }
        let i = self.focus.and_then(|f| order.iter().position(|o| *o == f)).map(|i| (i + 1) % order.len()).unwrap_or(0);
        self.focus = Some(order[i]);
    }

    /// Ayar dosyası değişti.
    pub fn reload(&mut self, cfg: Config) {
        self.cfg = cfg;
    }

    /// Boşta kalma denetimi (zamanlayıcıdan).
    pub fn check_idle(&mut self) {
        if !self.is_open() {
            return;
        }
        let limit = self.cfg.vault.auto_lock_minutes.max(1) as u64 * 60_000;
        if crate::ui::idle_ms() >= limit {
            self.lock(t!("boşta", "idle"));
        }
    }
}
