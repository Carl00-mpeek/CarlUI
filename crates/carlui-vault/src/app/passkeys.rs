//! Tarayıcıdaki passkey istekleri (eklenti → ada köprüsü → `Request::Passkey*`).
//!
//! Akış: istek gelince kasa hemen bir bilet (`Reply::pending`) döner, öne gelir
//! ve onay kartını gösterir (kilitliyse önce kilit açılır). Ada köprüsü
//! sonucu `PasskeyWait` ile sorar. Kullanıcı "Kaydet"/"Giriş yap"a basmadan
//! hiçbir anahtar oluşturulmaz ya da imza atılmaz. "Tarayıcıya bırak", süre
//! dolması, kilitleme ya da kasada uygun passkey olmaması eklentiye `fallback`
//! döner; tarayıcının kendi passkey akışı sürer.
//!
//! Köken (`origin`) eklentinin arka planının bildirdiği sekme adresidir;
//! rpId ona göre `passkey::rp_allowed` ile denetlenir, clientDataJSON da
//! burada o kökenden üretilir.

use super::*;
use carlui_core::vaultlink::PasskeyCred;
use carlui_vault::{
    autofill,
    passkey::{self, Passkey, ES256},
};
use std::time::Duration;

/// Onay kartı bu kadar açık kalır (köprü 120 sn bekler).
const ASK_TTL: Duration = Duration::from_secs(110);
/// Liste başına en çok kimlik (exclude/allow).
const MAX_IDS: usize = 64;

pub enum AskKind {
    Create { rp_name: String, user_name: String, user_display: String, user_id: String, exclude: Vec<String> },
    Get { allow: Vec<String> },
}

/// Kasadaki bir passkey (girişte seçenek) ya da passkey'in ekleneceği giriş kaydı.
pub struct Choice {
    pub entry: String,
    pub name: String,
    pub user: String,
    pub credential: String,
}

pub struct PasskeyAsk {
    pub ticket: String,
    pub kind: AskKind,
    pub origin: String,
    pub rp_id: String,
    challenge: String,
    /// Girişte: uyan passkey'ler. Oluşturmada: passkey'in ekleneceği mevcut kayıt (0 ya da 1).
    pub choices: Vec<Choice>,
    /// Kasa açıkken seçenekler hazırlandı mı (kilitliyken gelen istek açılınca hazırlanır).
    prepared: bool,
    at: Instant,
    back: isize,
}

impl PasskeyAsk {
    pub fn site(&self) -> &str {
        self.origin.split("://").nth(1).unwrap_or(&self.origin)
    }
}

fn ids(list: Vec<String>) -> Vec<String> {
    list.into_iter().take(MAX_IDS).filter_map(|s| passkey::unb64(&s).map(|b| passkey::b64(&b))).collect()
}

fn ticket() -> String {
    let mut b = [0u8; 16];
    let _ = getrandom::getrandom(&mut b);
    passkey::b64(&b)
}

impl App {
    /// `PasskeyCreate` / `PasskeyGet`.
    pub(super) fn passkey_request(&mut self, req: Request) -> Reply {
        if let Err(r) = self.browser_gate() {
            return Reply::fallback(r.message);
        }
        let (origin, rp_id, challenge, kind) = match req {
            Request::PasskeyCreate { origin, rp_id, rp_name, user_name, user_display, user_id, challenge, algs, exclude } => {
                if !algs.contains(&ES256) {
                    return Reply::fallback(t!("Site desteklenmeyen bir passkey algoritması istedi.", "The site asked for an unsupported passkey algorithm."));
                }
                let uid = passkey::unb64(&user_id).unwrap_or_default();
                if uid.is_empty() || uid.len() > 64 || user_name.len() > 256 || rp_name.len() > 256 || user_display.len() > 256 {
                    return Reply::fallback(passkey::Error::BadInput.to_string());
                }
                (origin, rp_id, challenge, AskKind::Create { rp_name, user_name, user_display, user_id: passkey::b64(&uid), exclude: ids(exclude) })
            }
            Request::PasskeyGet { origin, rp_id, challenge, allow } => (origin, rp_id, challenge, AskKind::Get { allow: ids(allow) }),
            _ => return Reply::no(""),
        };
        let rp_id = rp_id.trim().to_ascii_lowercase();
        let Some((origin, _)) = passkey::origin_of(&origin).filter(|_| passkey::rp_allowed(&rp_id, &origin)) else {
            warn!("kasa: passkey isteği reddedildi, rpId sekme adresine uymuyor");
            return Reply::fallback(t!("Bu passkey isteği bu siteye ait değil.", "This passkey request doesn't belong to this site."));
        };
        if passkey::client_data("webauthn.get", &challenge, &origin).is_err() {
            return Reply::fallback(passkey::Error::BadInput.to_string());
        }
        // Önceki onay hâlâ açıksa tarayıcıya bırakılır.
        if let Some(old) = self.passkey.take() {
            self.passkey_done = Some((old.ticket, Reply::fallback("")));
        }
        let back = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() }.0 as isize;
        let mut ask = PasskeyAsk { ticket: ticket(), kind, origin, rp_id, challenge, choices: Vec::new(), prepared: false, at: Instant::now(), back };
        let open = self.is_open();
        if open {
            if let Some(r) = self.prepare(&mut ask) {
                return r;
            }
        }
        let ticket = ask.ticket.clone();
        info!("kasa: {} için passkey isteği ({})", ask.site(), if matches!(ask.kind, AskKind::Create { .. }) { "oluşturma" } else { "giriş" });
        self.passkey = Some(ask);
        self.restore_window();
        carlui_core::win::force_foreground(self.hwnd);
        Reply { ok: true, pending: ticket, locked: !open, ..Default::default() }
    }

    /// Kasadaki kayıtlara bakar. Hemen verilecek bir yanıt varsa (uygun passkey
    /// yok, kimlik zaten kayıtlı) onu döner; yoksa onay kartı gösterilebilir.
    fn prepare(&self, ask: &mut PasskeyAsk) -> Option<Reply> {
        let v = self.vault.as_ref()?;
        ask.prepared = true;
        ask.choices.clear();
        for it in v.items() {
            let Ok(body) = v.decrypt(&it.id) else { continue };
            match &ask.kind {
                AskKind::Get { allow } => {
                    for k in body.passkeys.iter().filter(|k| k.rp_id == ask.rp_id && (allow.is_empty() || allow.contains(&k.credential_id))) {
                        ask.choices.push(Choice { entry: it.id.clone(), name: it.meta.name.clone(), user: k.user_name.clone(), credential: k.credential_id.clone() });
                    }
                }
                AskKind::Create { user_name, exclude, .. } => {
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
        if matches!(ask.kind, AskKind::Get { .. }) && ask.choices.is_empty() {
            return Some(Reply::fallback(t!("Carl Vault'ta bu site için passkey yok.", "Carl Vault has no passkey for this site.")));
        }
        None
    }

    /// Kasa açılınca kilitliyken gelen isteğin seçenekleri hazırlanır.
    pub(super) fn passkey_unlocked(&mut self) {
        let Some(mut ask) = self.passkey.take() else { return };
        if ask.prepared {
            self.passkey = Some(ask);
            return;
        }
        if ask.at.elapsed() > ASK_TTL {
            self.passkey_done = Some((ask.ticket, Reply::fallback("")));
            return;
        }
        match self.prepare(&mut ask) {
            Some(r) => {
                self.passkey_done = Some((ask.ticket.clone(), r));
                self.focus_back(ask.back);
            }
            None => self.passkey = Some(ask),
        }
    }

    /// `PasskeyWait`: sonuç hazırsa o, değilse yine bilet.
    pub(super) fn passkey_wait(&mut self, ticket: &str) -> Reply {
        if self.passkey_done.as_ref().is_some_and(|(t, _)| t == ticket) {
            return self.passkey_done.take().map(|(_, r)| r).unwrap_or_default();
        }
        match &self.passkey {
            Some(a) if a.ticket == ticket && a.at.elapsed() <= ASK_TTL => Reply { ok: true, pending: ticket.into(), locked: !self.is_open(), ..Default::default() },
            Some(a) if a.ticket == ticket => {
                self.passkey = None;
                Reply::fallback(t!("Süre doldu.", "Timed out."))
            }
            _ => Reply::fallback(""),
        }
    }

    /// Kilitlenince ya da kart kapanınca bekleyen istek tarayıcıya bırakılır.
    pub(super) fn passkey_drop(&mut self) {
        if let Some(a) = self.passkey.take() {
            self.passkey_done = Some((a.ticket, Reply::fallback("")));
        }
    }

    /// Kartın süresi doldu mu (pencere her saniye bakar).
    pub(super) fn passkey_expired(&mut self) -> bool {
        if self.passkey.as_ref().is_some_and(|a| a.at.elapsed() > ASK_TTL) {
            self.passkey_drop();
            return true;
        }
        false
    }

    /// "Tarayıcıya bırak": tarayıcının kendi passkey penceresi açılır.
    pub(super) fn passkey_decline(&mut self) {
        let back = self.passkey.as_ref().map(|a| a.back);
        self.passkey_drop();
        if let Some(b) = back {
            self.focus_back(b);
        }
    }

    /// Onay: oluşturmada `i` yok sayılır, girişte seçilen passkey.
    pub(super) fn passkey_approve(&mut self, i: usize) {
        let Some(ask) = self.passkey.take() else { return };
        if !self.is_open() {
            self.passkey = Some(ask);
            return;
        }
        let result = match &ask.kind {
            AskKind::Create { .. } => self.passkey_create(&ask),
            AskKind::Get { .. } => self.passkey_sign(&ask, i),
        };
        let reply = match result {
            Ok((cred, msg, entry)) => {
                self.toast(msg);
                self.sync_editor(&entry);
                Reply { ok: true, passkey: Some(cred), ..Default::default() }
            }
            Err(e) => {
                warn!("kasa: passkey işlemi başarısız: {e}");
                self.toast(e.clone());
                Reply::fallback(e)
            }
        };
        self.passkey_done = Some((ask.ticket.clone(), reply));
        // Listedeki "Passkey" işaretleri yenilensin.
        self.refresh_codes();
        self.focus_back(ask.back);
    }

    fn passkey_create(&mut self, ask: &PasskeyAsk) -> Result<(PasskeyCred, String, String), String> {
        let AskKind::Create { rp_name, user_name, user_display, user_id, .. } = &ask.kind else { return Err(String::new()) };
        let pk = Passkey::generate(&ask.rp_id, rp_name, user_name, user_display, user_id, now()).map_err(|e| e.to_string())?;
        let reg = passkey::register(&pk, &ask.origin, &ask.challenge).map_err(|e| e.to_string())?;
        let v = self.vault.as_mut().ok_or_default()?;
        let (name, entry) = match ask.choices.first() {
            Some(c) => {
                let mut body = v.decrypt(&c.entry).map_err(|e| e.to_string())?;
                // Aynı site ve kullanıcı için eski passkey'in yerine geçer.
                body.passkeys.retain(|k| !(k.rp_id == pk.rp_id && k.user_handle == pk.user_handle));
                body.passkeys.push(pk);
                let mut meta = v.items().iter().find(|i| i.id == c.entry).map(|i| i.meta.clone()).ok_or_default()?;
                meta.updated = now();
                let name = meta.name.clone();
                v.update(&c.entry, meta, &body).map_err(|e| e.to_string())?;
                (name, c.entry.clone())
            }
            None => {
                let site = if rp_name.trim().is_empty() { autofill::site_name(&ask.origin) } else { rp_name.trim().chars().take(120).collect() };
                let url = format!("https://{}", ask.rp_id);
                let mut i = carlui_vault::import::login(carlui_vault::import::Login { name: &site, url: &url, user: user_name, password: "", totp: "", notes: "" });
                i.body.passkeys.push(pk);
                let name = i.meta.name.clone();
                let id = v.add(i.meta.clone(), &i.body).map_err(|e| e.to_string())?;
                (name, id)
            }
        };
        info!("kasa: {} için passkey oluşturuldu", ask.site());
        let cred = PasskeyCred { id: reg.credential_id, client_data: reg.client_data, authenticator_data: reg.authenticator_data, attestation_object: reg.attestation_object, public_key: reg.public_key, ..Default::default() };
        Ok((cred, t!(format!("Passkey kaydedildi · {name}"), format!("Passkey saved · {name}")), entry))
    }

    fn passkey_sign(&mut self, ask: &PasskeyAsk, i: usize) -> Result<(PasskeyCred, String, String), String> {
        let c = ask.choices.get(i).ok_or_default()?;
        let v = self.vault.as_mut().ok_or_default()?;
        let mut body = v.decrypt(&c.entry).map_err(|e| e.to_string())?;
        let pk = body.passkeys.iter_mut().find(|k| k.credential_id == c.credential && k.rp_id == ask.rp_id).ok_or_else(|| t!("Passkey bulunamadı.", "Passkey not found.").to_string())?;
        let a = passkey::assert(pk, &ask.origin, &ask.challenge).map_err(|e| e.to_string())?;
        // Sayaç kalıcı olsun; kaydın tarihi değişmez.
        let meta = v.items().iter().find(|it| it.id == c.entry).map(|it| it.meta.clone()).ok_or_default()?;
        v.update(&c.entry, meta, &body).map_err(|e| e.to_string())?;
        info!("kasa: {} için passkey ile giriş imzalandı", ask.site());
        let cred = PasskeyCred { id: a.credential_id, client_data: a.client_data, authenticator_data: a.authenticator_data, signature: a.signature, user_handle: a.user_handle, ..Default::default() };
        Ok((cred, t!(format!("Passkey ile giriş · {}", c.name), format!("Signed in with passkey · {}", c.name)), c.entry.clone()))
    }

    /// Bu kayıt düzenleyicide açıksa passkey'leri kasadakiyle eşitlenir; yoksa
    /// düzenleyiciyi kaydetmek yeni passkey'i (ya da sayacı) geri alırdı.
    fn sync_editor(&mut self, entry: &str) {
        let Some(body) = self.vault.as_ref().and_then(|v| v.decrypt(entry).ok()) else { return };
        if let View::Edit(e) = &mut self.view {
            if e.id.as_deref() == Some(entry) {
                let clean = !e.dirty();
                e.passkeys = body.passkeys.clone();
                if clean {
                    e.snap = e.digest();
                }
            }
        }
    }

    /// Odağı isteği yapan tarayıcı penceresine geri ver, kasayı küçült.
    fn focus_back(&self, back: isize) {
        let hwnd = HWND(back as _);
        let exe = carlui_core::win::window_exe(hwnd);
        if !crate::inject::is_browser(exe.rsplit('\\').next().unwrap_or_default()) {
            return;
        }
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_MINIMIZE};
            let _ = ShowWindow(self.hwnd, SW_MINIMIZE);
        }
        carlui_core::win::force_foreground(hwnd);
    }
}

trait OkOrDefault<T> {
    fn ok_or_default(self) -> Result<T, String>;
}

impl<T> OkOrDefault<T> for Option<T> {
    fn ok_or_default(self) -> Result<T, String> {
        self.ok_or_else(|| t!("Kayıt bulunamadı.", "Item not found.").to_string())
    }
}
