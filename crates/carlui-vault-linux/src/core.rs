//! Pencere ile tarayıcı köprüsünün paylaştığı durum: açık kasa, ayarlar ve
//! kullanıcıdan beklenen onaylar (eklenti eşleştirmesi, passkey).
//!
//! Kasa işlemleri Windows'taki `carlui-vault` penceresinin (`app.rs`,
//! `app/capture.rs`, `app/passkeys.rs`) aynısıdır; yalnızca boru yerine aynı
//! süreçte, kilitli bir `Mutex` üzerinden çağrılır.

use crate::reply::{Login, PasskeyCred, Reply};
use crate::settings::Settings;
use carlui_vault::{
    autofill::{self, Capture},
    entry::{now, Body, Category, Meta},
    passkey::{self, Passkey, ES256},
    t,
    throttle::Throttle,
    vault::Vault,
};
use std::{
    path::PathBuf,
    sync::{mpsc, Arc, Mutex, MutexGuard, OnceLock},
    time::{Duration, Instant, SystemTime},
};

/// Formdan gelen alanların üst sınırı.
const MAX_LEN: usize = 1024;
/// Kayıt adının üst sınırı (karakter).
const MAX_NAME: usize = 120;
/// Liste başına en çok kimlik (exclude/allow).
const MAX_IDS: usize = 64;
/// Onay kartı bu kadar açık kalır (eklenti 130 sn bekler).
pub const ASK_TTL: Duration = Duration::from_secs(110);

pub struct Core {
    pub dir: PathBuf,
    /// Kasa dosyası yoksa `None` (ilk kurulum).
    pub vault: Option<Vault>,
    pub throttle: Throttle,
    pub settings: Settings,
    /// Son işlem (pencere girdisi ya da köprü isteği): otomatik kilit için.
    pub activity: Instant,
    /// Kayıtlar değişince artar; pencere listesini yeniler.
    pub generation: u64,
    /// Pencerenin altında kısa süre görünen bildirim.
    pub toast: Option<(String, Instant)>,
}

impl Core {
    pub fn is_open(&self) -> bool {
        self.vault.as_ref().is_some_and(|v| v.is_open())
    }

    pub fn touch(&mut self) {
        self.activity = Instant::now();
    }

    pub fn changed(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    fn open_vault(&self) -> Option<&Vault> {
        self.vault.as_ref().filter(|v| v.is_open())
    }

    fn locked_reply() -> Reply {
        Reply { locked: true, ..Reply::no(t!("Carl Vault kilitli.", "Carl Vault is locked.")) }
    }

    /// Bu sitenin kayıtları (ad ve kullanıcı adı; parola verilmez).
    pub fn find_logins(&mut self, url: &str) -> Reply {
        let Some(v) = self.open_vault() else { return Self::locked_reply() };
        let mut logins = Vec::new();
        for it in v.items() {
            let Ok(body) = v.decrypt(&it.id) else { continue };
            if autofill::body_matches(&body, url) {
                let (username, _) = autofill::credentials(&body);
                logins.push(Login { id: it.id.clone(), name: it.meta.name.clone(), username });
            }
        }
        let message = if logins.is_empty() { t!("Bu site için kayıt yok.", "No item for this site.").to_string() } else { String::new() };
        Reply { ok: true, message, logins, ..Default::default() }
    }

    /// Seçilen kaydın giriş bilgileri; kayıt adresi sayfaya uymazsa verilmez.
    pub fn fill_login(&mut self, url: &str, id: &str, browser: &str) -> Reply {
        let Some(v) = self.open_vault() else { return Self::locked_reply() };
        let Ok(body) = v.decrypt(id) else { return Reply::no(t!("Kayıt bulunamadı.", "Item not found.")) };
        if !autofill::body_matches(&body, url) {
            eprintln!("kasa: doldurma reddedildi, kayıt adresi sayfaya uymuyor");
            return Reply::no(t!("Bu kayıt bu siteye ait değil.", "This item doesn't belong to this site."));
        }
        let (user, pass) = autofill::credentials(&body);
        if pass.is_empty() && user.is_empty() {
            return Reply::no(t!("Kayıtta kullanıcı adı ya da parola yok.", "The item has no username or password."));
        }
        let name = v.items().iter().find(|i| i.id == id).map(|i| i.meta.name.clone()).unwrap_or_default();
        let host = url.split("://").nth(1).and_then(|r| r.split(['/', '?', '#']).next()).unwrap_or_default().to_string();
        eprintln!("kasa: {browser} içinde {host} için giriş bilgisi verildi");
        self.toast(format!("{name} → {host}"));
        Reply { ok: true, fill: Some((user, pass)), ..Default::default() }
    }

    /// Sayfada gönderilen giriş formu: önce durum sorulur, `commit` ile yazılır.
    pub fn save_login(&mut self, url: &str, user: &str, password: &str, commit: bool, name: &str) -> Reply {
        let user = user.trim();
        if password.is_empty() || password.len() > MAX_LEN || user.len() > MAX_LEN || autofill::origin(url).is_none() {
            return Reply::no(t!("Kaydedilecek giriş bilgisi yok.", "No login to save."));
        }
        let Some(v) = self.open_vault() else { return Self::locked_reply() };
        let bodies: Vec<(String, Body)> = v.items().iter().filter(|i| matches!(i.meta.category, Category::Password | Category::Email)).filter_map(|i| Some((i.id.clone(), v.decrypt(&i.id).ok()?))).collect();
        let found = autofill::classify(bodies.iter().map(|(id, b)| (id.as_str(), b)), url, user, password);
        let status = match &found {
            Capture::New => "new",
            Capture::Changed(_) => "changed",
            Capture::Same => "same",
        };
        let shown = match &found {
            Capture::Changed(id) => v.items().iter().find(|i| &i.id == id).map(|i| i.meta.name.clone()).unwrap_or_default(),
            _ => autofill::site_name(url),
        };
        if !commit || matches!(found, Capture::Same) {
            return Reply { ok: true, status: status.into(), name: shown, ..Default::default() };
        }
        let Some(v) = self.vault.as_mut() else { return Self::locked_reply() };
        let result = match &found {
            Capture::Changed(id) => {
                let Some((_, mut body)) = bodies.into_iter().find(|(i, _)| i == id) else { return Reply::no(t!("Kayıt bulunamadı.", "Item not found.")) };
                autofill::set_password(&mut body, password);
                let (name, category) = v.items().iter().find(|i| &i.id == id).map(|i| (i.meta.name.clone(), i.meta.category)).unwrap_or((String::new(), Category::Password));
                v.update(id, Meta { name: name.clone(), category, updated: now() }, &body).map(|_| name)
            }
            _ => {
                let (mut meta, body) = autofill::new_login(url, user, password);
                let given: String = name.chars().filter(|c| !c.is_control()).take(MAX_NAME).collect();
                if !given.trim().is_empty() {
                    meta.name = given.trim().to_string();
                }
                let name = meta.name.clone();
                v.add(meta, &body).map(|_| name)
            }
        };
        match result {
            Ok(name) => {
                let (tr, en) = if status == "changed" { ("parolası güncellendi", "password updated") } else { ("kaydedildi", "saved") };
                let message = t!(format!("{name} {tr}."), format!("{name} {en}."));
                self.toast(message.clone());
                self.changed();
                Reply { ok: true, status: status.into(), message, ..Default::default() }
            }
            Err(e) => Reply::no(e.to_string()),
        }
    }
}

// ---------------------------------------------------------------- passkey

pub enum PasskeyKind {
    Create { rp_name: String, user_name: String, user_display: String, user_id: String, exclude: Vec<String> },
    Get { allow: Vec<String> },
}

/// Kasadaki bir passkey (girişte seçenek) ya da passkey'in ekleneceği giriş kaydı.
#[derive(Clone)]
pub struct Choice {
    pub entry: String,
    pub name: String,
    pub user: String,
    pub credential: String,
}

pub struct PasskeyAsk {
    pub kind: PasskeyKind,
    pub origin: String,
    pub rp_id: String,
    pub challenge: String,
    pub choices: Vec<Choice>,
}

impl PasskeyAsk {
    pub fn site(&self) -> &str {
        self.origin.split("://").nth(1).unwrap_or(&self.origin)
    }

    pub fn is_create(&self) -> bool {
        matches!(self.kind, PasskeyKind::Create { .. })
    }
}

fn ids(list: Vec<String>) -> Vec<String> {
    list.into_iter().take(MAX_IDS).filter_map(|s| passkey::unb64(&s).map(|b| passkey::b64(&b))).collect()
}

/// Eklentinin isteğini doğrular. Hata: hemen dönecek yanıt.
#[allow(clippy::too_many_arguments)]
pub fn passkey_create(origin: &str, rp_id: &str, rp_name: String, user_name: String, user_display: String, user_id: &str, challenge: String, algs: &[i64], exclude: Vec<String>) -> Result<PasskeyAsk, Reply> {
    if !algs.contains(&ES256) {
        return Err(Reply::fallback(t!("Site desteklenmeyen bir passkey algoritması istedi.", "The site asked for an unsupported passkey algorithm.")));
    }
    let uid = passkey::unb64(user_id).unwrap_or_default();
    if uid.is_empty() || uid.len() > 64 || user_name.len() > 256 || rp_name.len() > 256 || user_display.len() > 256 {
        return Err(Reply::fallback(passkey::Error::BadInput.to_string()));
    }
    let kind = PasskeyKind::Create { rp_name, user_name, user_display, user_id: passkey::b64(&uid), exclude: ids(exclude) };
    passkey_common(origin, rp_id, challenge, kind)
}

pub fn passkey_get(origin: &str, rp_id: &str, challenge: String, allow: Vec<String>) -> Result<PasskeyAsk, Reply> {
    passkey_common(origin, rp_id, challenge, PasskeyKind::Get { allow: ids(allow) })
}

fn passkey_common(origin: &str, rp_id: &str, challenge: String, kind: PasskeyKind) -> Result<PasskeyAsk, Reply> {
    let rp_id = rp_id.trim().to_ascii_lowercase();
    let Some((origin, _)) = passkey::origin_of(origin).filter(|_| passkey::rp_allowed(&rp_id, origin)) else {
        eprintln!("kasa: passkey isteği reddedildi, rpId sekme adresine uymuyor");
        return Err(Reply::fallback(t!("Bu passkey isteği bu siteye ait değil.", "This passkey request doesn't belong to this site.")));
    };
    if passkey::client_data("webauthn.get", &challenge, &origin).is_err() {
        return Err(Reply::fallback(passkey::Error::BadInput.to_string()));
    }
    Ok(PasskeyAsk { kind, origin, rp_id, challenge, choices: Vec::new() })
}

fn not_found() -> String {
    t!("Kayıt bulunamadı.", "Item not found.").to_string()
}

impl Core {
    /// Seçenekleri hazırlar. Hemen verilecek yanıt varsa (uygun passkey yok,
    /// kimlik zaten kayıtlı) onu döner; yoksa onay kartı gösterilir.
    pub fn passkey_prepare(&self, ask: &mut PasskeyAsk) -> Option<Reply> {
        let Some(v) = self.open_vault() else { return Some(Reply::fallback("")) };
        ask.choices.clear();
        for it in v.items() {
            let Ok(body) = v.decrypt(&it.id) else { continue };
            match &ask.kind {
                PasskeyKind::Get { allow } => {
                    for k in body.passkeys.iter().filter(|k| k.rp_id == ask.rp_id && (allow.is_empty() || allow.contains(&k.credential_id))) {
                        ask.choices.push(Choice { entry: it.id.clone(), name: it.meta.name.clone(), user: k.user_name.clone(), credential: k.credential_id.clone() });
                    }
                }
                PasskeyKind::Create { user_name, exclude, .. } => {
                    if body.passkeys.iter().any(|k| k.rp_id == ask.rp_id && exclude.contains(&k.credential_id)) {
                        return Some(Reply { error: "InvalidStateError".into(), ..Reply::no(t!("Bu site için passkey zaten Carl Vault'ta.", "Carl Vault already has a passkey for this site.")) });
                    }
                    let login = matches!(it.meta.category, Category::Password | Category::Email) && autofill::body_matches(&body, &ask.origin);
                    if login && ask.choices.is_empty() && autofill::credentials(&body).0.eq_ignore_ascii_case(user_name.trim()) {
                        ask.choices.push(Choice { entry: it.id.clone(), name: it.meta.name.clone(), user: user_name.clone(), credential: String::new() });
                    }
                }
            }
        }
        if !ask.is_create() && ask.choices.is_empty() {
            return Some(Reply::fallback(t!("Carl Vault'ta bu site için passkey yok.", "Carl Vault has no passkey for this site.")));
        }
        None
    }

    /// Kullanıcı onayladı: oluşturmada `i` yok sayılır, girişte seçilen passkey.
    pub fn passkey_approve(&mut self, ask: &PasskeyAsk, i: usize) -> Reply {
        if !self.is_open() {
            return Reply::fallback("");
        }
        let result = if ask.is_create() { self.passkey_new(ask) } else { self.passkey_sign(ask, i) };
        match result {
            Ok((cred, msg)) => {
                self.toast(msg);
                self.changed();
                Reply { ok: true, passkey: Some(cred), ..Default::default() }
            }
            Err(e) => {
                eprintln!("kasa: passkey işlemi başarısız: {e}");
                self.toast(e.clone());
                Reply::fallback(e)
            }
        }
    }

    fn passkey_new(&mut self, ask: &PasskeyAsk) -> Result<(PasskeyCred, String), String> {
        let PasskeyKind::Create { rp_name, user_name, user_display, user_id, .. } = &ask.kind else { return Err(String::new()) };
        let pk = Passkey::generate(&ask.rp_id, rp_name, user_name, user_display, user_id, now()).map_err(|e| e.to_string())?;
        let reg = passkey::register(&pk, &ask.origin, &ask.challenge).map_err(|e| e.to_string())?;
        let v = self.vault.as_mut().ok_or_else(not_found)?;
        let name = match ask.choices.first() {
            Some(c) => {
                let mut body = v.decrypt(&c.entry).map_err(|e| e.to_string())?;
                body.passkeys.retain(|k| !(k.rp_id == pk.rp_id && k.user_handle == pk.user_handle));
                body.passkeys.push(pk);
                let mut meta = v.items().iter().find(|i| i.id == c.entry).map(|i| i.meta.clone()).ok_or_else(not_found)?;
                meta.updated = now();
                let name = meta.name.clone();
                v.update(&c.entry, meta, &body).map_err(|e| e.to_string())?;
                name
            }
            None => {
                let site = if rp_name.trim().is_empty() { autofill::site_name(&ask.origin) } else { rp_name.trim().chars().take(MAX_NAME).collect() };
                let url = format!("https://{}", ask.rp_id);
                let mut i = carlui_vault::import::login(carlui_vault::import::Login { name: &site, url: &url, user: user_name, password: "", totp: "", notes: "" });
                i.body.passkeys.push(pk);
                let name = i.meta.name.clone();
                v.add(i.meta.clone(), &i.body).map_err(|e| e.to_string())?;
                name
            }
        };
        eprintln!("kasa: {} için passkey oluşturuldu", ask.site());
        let cred = PasskeyCred { id: reg.credential_id, client_data: reg.client_data, authenticator_data: reg.authenticator_data, attestation_object: reg.attestation_object, public_key: reg.public_key, ..Default::default() };
        Ok((cred, t!(format!("Passkey kaydedildi · {name}"), format!("Passkey saved · {name}"))))
    }

    fn passkey_sign(&mut self, ask: &PasskeyAsk, i: usize) -> Result<(PasskeyCred, String), String> {
        let c = ask.choices.get(i).ok_or_else(not_found)?;
        let v = self.vault.as_mut().ok_or_else(not_found)?;
        let mut body = v.decrypt(&c.entry).map_err(|e| e.to_string())?;
        let pk = body.passkeys.iter_mut().find(|k| k.credential_id == c.credential && k.rp_id == ask.rp_id).ok_or_else(|| t!("Passkey bulunamadı.", "Passkey not found.").to_string())?;
        let a = passkey::assert(pk, &ask.origin, &ask.challenge).map_err(|e| e.to_string())?;
        // Sayaç kalıcı olsun; kaydın tarihi değişmez.
        let meta = v.items().iter().find(|it| it.id == c.entry).map(|it| it.meta.clone()).ok_or_else(not_found)?;
        v.update(&c.entry, meta, &body).map_err(|e| e.to_string())?;
        eprintln!("kasa: {} için passkey ile giriş imzalandı", ask.site());
        let cred = PasskeyCred { id: a.credential_id, client_data: a.client_data, authenticator_data: a.authenticator_data, signature: a.signature, user_handle: a.user_handle, ..Default::default() };
        Ok((cred, t!(format!("Passkey ile giriş · {}", c.name), format!("Signed in with passkey · {}", c.name))))
    }
}

// ---------------------------------------------------------------- onaylar

pub enum AskKind {
    /// Eklenti eşleştirmesi (köken).
    Pair(String),
    Passkey(PasskeyAsk),
}

/// Pencerede onay kartı. Yanıt: `Some(i)` onay (passkey seçimi), `None` ret.
/// Kart düşerse (kilit, süre) gönderici de düşer; köprü bunu ret sayar.
pub struct Ask {
    pub id: u64,
    pub kind: AskKind,
    pub at: Instant,
    /// Pencere yanıtladı; kart köprü onu alana dek gizlenir.
    pub answered: bool,
    pub tx: mpsc::Sender<Option<usize>>,
}

#[derive(Default)]
pub struct Asks {
    pub list: Vec<Ask>,
    /// Kilitliyken gelen passkey isteği: kasa açılınca kart gösterilir.
    pub waiting_unlock: Option<String>,
    next: u64,
}

pub struct Shared {
    pub core: Mutex<Core>,
    pub asks: Mutex<Asks>,
    pub ctx: OnceLock<eframe::egui::Context>,
}

impl Shared {
    pub fn new(dir: PathBuf, settings: Settings) -> Arc<Self> {
        let vault = if Vault::exists(&dir) {
            match Vault::load(&dir) {
                Ok(v) => Some(v),
                Err(e) => {
                    eprintln!("kasa dosyası okunamadı: {e}");
                    None
                }
            }
        } else {
            None
        };
        let throttle = Throttle::load(&dir);
        Arc::new(Self {
            core: Mutex::new(Core { dir, vault, throttle, settings, activity: Instant::now(), generation: 0, toast: None }),
            asks: Mutex::new(Asks::default()),
            ctx: OnceLock::new(),
        })
    }

    pub fn core(&self) -> MutexGuard<'_, Core> {
        self.core.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn asks(&self) -> MutexGuard<'_, Asks> {
        self.asks.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn repaint(&self) {
        if let Some(c) = self.ctx.get() {
            c.request_repaint();
        }
    }

    /// Pencereyi öne getirir. Wayland'de odak alma derleyiciye bağlıdır; KDE
    /// en azından görev çubuğunda dikkat ister.
    pub fn raise(&self) {
        use eframe::egui::{UserAttentionType, ViewportCommand};
        if let Some(c) = self.ctx.get() {
            c.send_viewport_cmd(ViewportCommand::Visible(true));
            c.send_viewport_cmd(ViewportCommand::Minimized(false));
            c.send_viewport_cmd(ViewportCommand::Focus);
            c.send_viewport_cmd(ViewportCommand::RequestUserAttention(UserAttentionType::Informational));
            c.request_repaint();
        }
    }

    /// Kartı ekler ve kullanıcının yanıtını bekler (süre dolarsa `None`).
    pub fn ask(&self, kind: AskKind, wait: Duration) -> (Option<usize>, AskKind) {
        let (tx, rx) = mpsc::channel();
        let id = {
            let mut a = self.asks();
            a.next += 1;
            let id = a.next;
            a.list.push(Ask { id, kind, at: Instant::now(), answered: false, tx });
            id
        };
        self.raise();
        let answer = rx.recv_timeout(wait).ok().flatten();
        let kind = {
            let mut a = self.asks();
            let pos = a.list.iter().position(|x| x.id == id);
            pos.map(|p| a.list.remove(p).kind)
        };
        self.repaint();
        // Yanıtlanan kartı pencere yalnızca gizler (türü burada geri alınır);
        // kilitlenince düşürülen kartın türü yoktur.
        match kind {
            Some(k) => (answer, k),
            None => (None, AskKind::Pair(String::new())),
        }
    }

    /// Kasayı kilitler; bekleyen passkey kartları tarayıcıya bırakılır.
    pub fn lock(&self) {
        {
            let mut c = self.core();
            if let Some(v) = c.vault.as_mut() {
                v.lock();
            }
            c.changed();
        }
        self.asks().list.retain(|a| !matches!(a.kind, AskKind::Passkey(_)));
        self.repaint();
    }

    /// Otomatik kilit ve uyku denetimi (arka plan iş parçacığı).
    pub fn ticker(self: Arc<Self>) {
        let mut last = (Instant::now(), SystemTime::now());
        loop {
            std::thread::sleep(Duration::from_secs(5));
            let (mono, wall) = (Instant::now(), SystemTime::now());
            // Uyku: duvar saati tekdüze saatten çok ileri gittiyse makine uyumuştur.
            let slept = wall.duration_since(last.1).unwrap_or_default().saturating_sub(mono.duration_since(last.0)) > Duration::from_secs(30);
            last = (mono, wall);
            let idle = {
                let c = self.core();
                let limit = c.settings.auto_lock;
                c.is_open() && (slept || (limit > 0 && c.activity.elapsed() > Duration::from_secs(limit as u64 * 60)))
            };
            if idle {
                eprintln!("kasa: {}", if slept { "uykudan dönüldü, kilitlendi" } else { "işlem yok, kilitlendi" });
                self.lock();
            }
            // Süresi dolan kartlar.
            let expired = self.asks().list.iter().any(|a| a.at.elapsed() > ASK_TTL);
            if expired {
                self.asks().list.retain(|a| a.at.elapsed() <= ASK_TTL);
                self.repaint();
            }
        }
    }
}
