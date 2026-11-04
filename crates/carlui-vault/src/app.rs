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
use carlui_core::{
    config::Config,
    info, t,
    vaultlink::{Offer, Reply, Request},
    warn,
};
use carlui_vault::{
    crypto::{Error, KdfParams},
    entry::{now, Attachment, Body, Category, Field, FieldKind, Meta},
    token::{TokenError, Tokens},
    totp::{self, Totp},
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

    /// Ekran görüntüsündeki QR'dan gelen 2FA: ad ve hesap otpauth etiketinden,
    /// anahtar alanına adresin tamamı (hane/süre/algoritma korunsun).
    fn import(uri: &str) -> Self {
        let mut e = Self::new(Category::TwoFactor);
        let (issuer, account) = totp::names(uri);
        e.name = TextBox::new(if issuer.is_empty() { &account } else { &issuer }, false);
        let mut account = Some(account);
        for f in e.fields.iter_mut() {
            match f.kind {
                FieldKind::Text => {
                    if let Some(a) = account.take() {
                        f.value = TextBox::new(&a, false);
                    }
                }
                FieldKind::Totp => f.value = TextBox::new(uri, false),
                _ => {}
            }
        }
        e
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
    /// Panoya verilen tek kullanımlık belirteçler (Aşama 2).
    pub tokens: Tokens,
    /// Ekran görüntüsü aracından gelen, henüz açılmamış 2FA (otpauth adresi).
    /// Kasa açılınca ya da açık kayıt kapanınca düzenleyiciye konur.
    import: Option<(Zeroizing<String>, Instant)>,
}

/// Bekleyen 2FA aktarımı bu kadar sonra unutulur.
const IMPORT_TTL: std::time::Duration = std::time::Duration::from_secs(300);

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
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
            tokens: Tokens::default(),
            import: None,
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
        // Listeye dönerken bekleyen 2FA aktarımı varsa onun kaydı açılır.
        let pending = matches!(v, View::List).then(|| self.import.take()).flatten().filter(|(_, at)| at.elapsed() < IMPORT_TTL);
        self.view = match pending {
            Some((uri, _)) => View::Edit(Box::new(Editor::import(&uri))),
            None => v,
        };
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
        self.tokens.clear();
        self.import = None;
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

    /// Düzenleyicideki alanı panoya verir: belirteç olarak (varsayılan) ya da
    /// `token_paste` kapalıysa değerin kendisi olarak.
    fn copy_field(&mut self, i: usize) {
        let Some((entry, label, value)) = (match &self.view {
            View::Edit(e) => e.fields.get(i).map(|f| {
                let name = e.name.as_str().trim();
                let field = f.label.as_str().trim();
                // 2FA alanında anahtar değil, o anki kod kopyalanır.
                let (field, value) = match f.kind {
                    FieldKind::Totp => (t!("2FA kodu", "2FA code"), Totp::parse(f.value.as_str()).map(|t| t.code(now())).unwrap_or_default()),
                    _ => (field, Zeroizing::new(f.value.as_str().to_string())),
                };
                let label = if name.is_empty() { field.to_string() } else { format!("{name} · {field}") };
                (e.id.clone().unwrap_or_default(), label, value)
            }),
            _ => None,
        }) else {
            return;
        };
        if value.is_empty() {
            return;
        }
        if !self.cfg.vault.token_paste {
            let field = label.rsplit(" · ").next().unwrap_or_default().to_string();
            self.copy(&value, &field);
            return;
        }
        let issued = match self.tokens.issue(&entry, i, &label, &value, now_ms()) {
            Ok(t) => t,
            Err(e) => return self.toast(e.to_string()),
        };
        let offer = Offer { token: issued.token.to_string(), label: issued.label, expires: issued.expires };
        let Some(seq) = clip::offer(self.hwnd, &offer) else {
            self.toast(t!("Panoya konamadı.", "Couldn't put it on the clipboard."));
            return;
        };
        self.clip_seq = Some(seq);
        let secs = (carlui_vault::token::TTL_MS / 1000) as u32;
        unsafe {
            let _ = KillTimer(self.hwnd, TIMER_CLIP);
            SetTimer(self.hwnd, TIMER_CLIP, secs * 1000, None);
        }
        self.toast(t!(format!("Hazır · hedef pencerede Win+V ile seç ({secs} sn)"), format!("Ready · pick it with Win+V in the target window ({secs} s)")));
    }

    /// Adadan gelen istek: pano belirteci ya da tarayıcı eklentisi.
    pub fn link(&mut self, req: Request, client_pid: u32) -> Reply {
        if let Request::ImportTotp { uri } = req {
            return self.import_totp(Zeroizing::new(uri));
        }
        if !self.is_open() {
            self.tokens.clear();
            return Reply { locked: true, ..Reply::no(t!("Carl Vault kilitli.", "Carl Vault is locked.")) };
        }
        match req {
            Request::Type { token, hwnd } => self.type_token(&token, hwnd, client_pid),
            Request::Find { url } => self.find_logins(&url),
            Request::Fill { url, id } => self.fill_login(&url, &id),
            Request::ImportTotp { .. } => unreachable!(),
        }
    }

    /// Ekran görüntüsündeki 2FA QR'ı: kasa kilitliyken de kabul edilir, kayıt
    /// açılınca düzenleyicide hazır durur. Hiçbir şey kendiliğinden kaydedilmez.
    fn import_totp(&mut self, uri: Zeroizing<String>) -> Reply {
        if let Err(e) = Totp::parse(&uri) {
            return Reply::no(e.to_string());
        }
        self.import = Some((uri, Instant::now()));
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, ShowWindow, SW_RESTORE};
            let _ = ShowWindow(self.hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(self.hwnd);
        }
        info!("kasa: ekran görüntüsünden 2FA aktarımı geldi");
        let message = match &self.view {
            View::List => {
                self.go(View::List);
                t!("Carl Vault'ta yeni 2FA kaydı açıldı; Kaydet'e bas.", "A new 2FA item is open in Carl Vault; press Save.")
            }
            View::Edit(_) | View::Security => {
                self.toast(t!("Açık sayfayı kapatınca 2FA kaydı açılacak.", "The 2FA item opens when you close this page."));
                t!("Carl Vault'ta açık sayfa kapanınca 2FA kaydı açılacak.", "The 2FA item opens in Carl Vault once the open page is closed.")
            }
            _ => t!("Carl Vault'u açınca 2FA kaydı hazır olacak.", "The 2FA item will be ready once you unlock Carl Vault."),
        };
        Reply { ok: true, message: message.into(), ..Default::default() }
    }

    /// Pano belirteci: hedefi doğrula, belirteci tüket, değeri yaz.
    fn type_token(&mut self, token: &str, hwnd: isize, client_pid: u32) -> Reply {
        let app = match crate::inject::check_target(HWND(hwnd as _), client_pid) {
            Ok(a) => a,
            Err(m) => {
                warn!("kasa: yapıştırma reddedildi: {m}");
                return Reply::no(m);
            }
        };
        let (value, label) = match self.tokens.take(token, now_ms()) {
            Ok(v) => v,
            Err(TokenError::Expired) => return Reply::no(t!("Süresi doldu; kasadan yeniden kopyala.", "It expired; copy it again from the vault.")),
            Err(TokenError::Unknown) => return Reply::no(t!("Bu kayıt artık geçerli değil.", "This item is no longer valid.")),
        };
        // Belirteç kullanıldı: panoda kaldıysa sil.
        self.clear_clipboard();
        if !crate::inject::type_text(&value) {
            return Reply::no(t!("Değer yazılamadı.", "Couldn't type the value."));
        }
        info!("kasa: bir alan {app} penceresine yazıldı");
        let field = label.rsplit(" · ").next().unwrap_or_default().to_string();
        self.toast(t!(format!("{field} → {app}"), format!("{field} → {app}")));
        Reply { ok: true, message: t!(format!("{field} yazıldı"), format!("{field} typed")), ..Default::default() }
    }

    /// Tarayıcı istekleri yalnızca ayar açıkken ve öndeki pencere tarayıcıyken.
    fn browser_gate(&self) -> Result<String, Reply> {
        if !self.cfg.vault.browser_fill {
            return Err(Reply::no(t!("Tarayıcıda doldurma Ayarlar'da kapalı.", "Browser filling is turned off in Settings.")));
        }
        crate::inject::browser_in_front().ok_or_else(|| Reply::no(t!("Öndeki pencere bir tarayıcı değil.", "The window in front isn't a browser.")))
    }

    /// Sayfa adresine uyan kayıtlar (ad ve kullanıcı adı; parola yok).
    fn find_logins(&mut self, url: &str) -> Reply {
        if let Err(r) = self.browser_gate() {
            return r;
        }
        let Some(v) = self.vault.as_ref() else { return Reply::no("") };
        let mut logins = Vec::new();
        for it in v.items() {
            // Kayıt geçici olarak çözülür; `Body` düşerken bellekten silinir.
            let Ok(body) = v.decrypt(&it.id) else { continue };
            if carlui_vault::autofill::body_matches(&body, url) {
                let (username, _) = carlui_vault::autofill::credentials(&body);
                logins.push(carlui_core::vaultlink::Login { id: it.id.clone(), name: it.meta.name.clone(), username });
            }
        }
        let message = if logins.is_empty() { t!("Bu site için kayıt yok.", "No item for this site.").to_string() } else { String::new() };
        Reply { ok: true, message, logins, ..Default::default() }
    }

    /// Seçilen kaydın giriş bilgileri; kayıt adresi sayfaya uymazsa verilmez.
    fn fill_login(&mut self, url: &str, id: &str) -> Reply {
        let browser = match self.browser_gate() {
            Ok(b) => b,
            Err(r) => return r,
        };
        let Some(v) = self.vault.as_ref() else { return Reply::no("") };
        let Ok(body) = v.decrypt(id) else { return Reply::no(t!("Kayıt bulunamadı.", "Item not found.")) };
        if !carlui_vault::autofill::body_matches(&body, url) {
            warn!("kasa: doldurma reddedildi, kayıt adresi sayfaya uymuyor");
            return Reply::no(t!("Bu kayıt bu siteye ait değil.", "This item doesn't belong to this site."));
        }
        let (user, pass) = carlui_vault::autofill::credentials(&body);
        if pass.is_empty() && user.is_empty() {
            return Reply::no(t!("Kayıtta kullanıcı adı ya da parola yok.", "The item has no username or password."));
        }
        let name = v.items().iter().find(|i| i.id == id).map(|i| i.meta.name.clone()).unwrap_or_default();
        let host = url.split("://").nth(1).and_then(|r| r.split(['/', '?', '#']).next()).unwrap_or_default().to_string();
        info!("kasa: {browser} içinde {host} için giriş bilgisi verildi");
        self.toast(t!(format!("{name} → {host}"), format!("{name} → {host}")));
        Reply { ok: true, fill: Some((user, pass)), ..Default::default() }
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

    /// Düzenleyicide 2FA alanı var mı (kod ve geri sayım her saniye yenilenir).
    pub fn ticking(&self) -> bool {
        matches!(&self.view, View::Edit(e) if e.fields.iter().any(|f| f.kind == FieldKind::Totp))
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
            Act::Copy(i) => self.copy_field(i),
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
                    let label = match kind {
                        FieldKind::Totp => t!("2FA anahtarı", "2FA key"),
                        k if k.masked() => t!("Gizli alan", "Secret field"),
                        _ => t!("Alan", "Field"),
                    };
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
        self.tokens.sweep(now_ms());
        let limit = self.cfg.vault.auto_lock_minutes.max(1) as u64 * 60_000;
        if crate::ui::idle_ms() >= limit {
            self.lock(t!("boşta", "idle"));
        }
    }
}
