//! Masaüstü uygulamalarının giriş ekranı (adadaki "Vault" düğmesi →
//! `Request::AppFill`). Akış: kasa istekteki pencereyi doğrular, uygulama
//! anahtarını pencerenin exe'sinden kendisi çıkarır (`steamwebhelper.exe` →
//! `steam`). Kasa açıksa ve bu uygulamaya bağlı tek kayıt varsa hemen yazar;
//! yoksa kasa öne gelir (kilitliyse önce kilit açılır) ve listede kayıt
//! seçilir. Seçilen kayda ilk kullanımda `app://<anahtar>` bağı eklenir.
//! Yazmadan hemen önce pencerenin hâlâ aynı uygulama olduğuna bakılır.
//!
//! Bağlı uygulamaların listesi (yalnızca anahtarlar) `vaultlink::apps_file`'a
//! yazılır; ada kasa kilitliyken de düğmeyi bunlarda gösterir.

use super::*;
use carlui_vault::autofill;
use std::time::Duration;
use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GetWindowThreadProcessId, IsWindow, IsWindowVisible, GA_ROOT};

/// Seçim bu kadar bekler.
const ASK_TTL: Duration = Duration::from_secs(120);

pub struct AppAsk {
    pub hwnd: isize,
    pub key: String,
    at: Instant,
}

impl AppAsk {
    pub fn name(&self) -> String {
        carlui_core::win::app_display(&self.key)
    }
}

/// Uygulama penceresi kabul edilir mi; öyleyse anahtarı.
fn app_target(hwnd: HWND, client_pid: u32) -> Result<String, String> {
    unsafe {
        if hwnd.is_invalid() || !IsWindow(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool() || GetAncestor(hwnd, GA_ROOT) != hwnd {
            return Err(t!("Uygulama penceresi geçersiz.", "The app window isn't valid.").into());
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 || pid == windows::Win32::System::Threading::GetCurrentProcessId() || pid == client_pid {
            return Err(t!("Uygulama penceresi kabul edilmedi.", "The app window was refused.").into());
        }
    }
    let exe = carlui_core::win::window_exe(hwnd);
    if exe.is_empty() || carlui_core::win::is_browser(&exe) {
        return Err(t!("Tarayıcılarda eklentinin düğmesini kullan.", "In browsers, use the extension's button.").into());
    }
    let key = carlui_core::win::app_key_of(&exe);
    if key.is_empty() || key.starts_with("carlui") {
        return Err(t!("Uygulama penceresi kabul edilmedi.", "The app window was refused.").into());
    }
    Ok(key)
}

impl App {
    /// `Request::AppFill`.
    pub(super) fn app_request(&mut self, hwnd: isize, client_pid: u32) -> Reply {
        if !self.cfg.vault.app_fill {
            return Reply::no(t!("Uygulamalarda doldurma Ayarlar'da kapalı.", "App filling is turned off in Settings."));
        }
        let key = match app_target(HWND(hwnd as _), client_pid) {
            Ok(k) => k,
            Err(m) => {
                warn!("kasa: uygulama isteği reddedildi: {m}");
                return Reply::no(m);
            }
        };
        info!("kasa: {key} uygulaması için giriş istendi");
        self.app = Some(AppAsk { hwnd, key, at: Instant::now() });
        let open = self.is_open();
        if open && self.app_ready() {
            return Reply { ok: true, ..Default::default() };
        }
        self.restore_window();
        carlui_core::win::force_foreground(self.hwnd);
        Reply { ok: true, locked: !open, ..Default::default() }
    }

    /// Bu uygulamaya bağlı kayıtlar.
    fn app_linked(&self, key: &str) -> Vec<String> {
        let Some(v) = self.vault.as_ref() else { return Vec::new() };
        v.items().iter().filter(|it| v.decrypt(&it.id).is_ok_and(|b| autofill::body_app_matches(&b, key))).map(|it| it.id.clone()).collect()
    }

    /// Bağ yokken uygulamaya ait görünen, parolalı kayıtlar: adı anahtarı
    /// içeren ya da web adresi uygulamanın sitesi olan ("steampowered.com").
    fn app_guess(&self, key: &str) -> Vec<String> {
        let Some(v) = self.vault.as_ref() else { return Vec::new() };
        v.items()
            .iter()
            .filter(|it| {
                v.decrypt(&it.id).is_ok_and(|b| {
                    let named = it.meta.name.to_lowercase().contains(key);
                    !autofill::credentials(&b).1.is_empty() && (named || autofill::body_site_matches(&b, key))
                })
            })
            .map(|it| it.id.clone())
            .collect()
    }

    /// Kasa açık: tek bağlı kayıt varsa hemen yazar (true); bağ yoksa uygulamaya
    /// ait görünen tek kayıt varsa onu kullanır. Yoksa listede seçim beklenir
    /// (arama kutusu uygulama adına uyan kayıtlarla doldurulur).
    fn app_ready(&mut self) -> bool {
        let Some(key) = self.app.as_ref().map(|a| a.key.clone()) else { return false };
        let mut linked = self.app_linked(&key);
        let bound = !linked.is_empty();
        if linked.is_empty() {
            linked = self.app_guess(&key);
            info!("kasa: {key} için bağlı kayıt yok; {} aday bulundu", linked.len());
        }
        if let [id] = linked.as_slice() {
            let id = id.clone();
            self.app_pick(&id);
            return true;
        }
        self.filter = None;
        self.search.clear();
        if !bound {
            let named = self.vault.as_ref().is_some_and(|v| v.items().iter().any(|i| i.meta.name.to_lowercase().contains(&key)));
            if named {
                self.search = TextBox::new(&carlui_core::win::app_display(&key), false);
            }
        }
        if !matches!(self.view, View::List) {
            self.go(View::List);
        }
        false
    }

    /// Kilit açıldı: bekleyen uygulama isteği varsa sürdür.
    pub(super) fn app_unlocked(&mut self) {
        if self.app.as_ref().is_some_and(|a| a.at.elapsed() > ASK_TTL) {
            self.app = None;
        }
        if self.app.is_some() {
            self.app_ready();
        }
    }

    /// Seçim süresi doldu mu (pencere her saniye bakar).
    pub(super) fn app_expired(&mut self) {
        if self.app.as_ref().is_some_and(|a| a.at.elapsed() > ASK_TTL) {
            self.app = None;
        }
    }

    pub(super) fn app_cancel(&mut self) {
        self.app = None;
        self.search.clear();
    }

    /// Listede seçilen kayıt: bağla (gerekirse) ve uygulamaya yaz.
    pub(super) fn app_pick(&mut self, id: &str) {
        let Some(ask) = self.app.take() else { return };
        self.search.clear();
        let hwnd = HWND(ask.hwnd as _);
        if ask.at.elapsed() > ASK_TTL {
            return self.toast(t!("Uygulama isteğinin süresi doldu; düğmeye yeniden bas.", "The app request expired; press the button again."));
        }
        if !unsafe { IsWindow(hwnd) }.as_bool() || carlui_core::win::app_key(hwnd) != ask.key {
            warn!("kasa: uygulama penceresi değişti; yazılmadı");
            return self.toast(t!("Uygulama penceresi kapanmış ya da değişmiş.", "The app window closed or changed."));
        }
        let Some(v) = self.vault.as_mut() else { return };
        let Some(meta) = v.items().iter().find(|i| i.id == id).map(|i| i.meta.clone()) else { return };
        let Ok(mut body) = v.decrypt(id) else { return self.toast(t!("Kayıt bulunamadı.", "Item not found.")) };
        let (user, pass) = autofill::credentials(&body);
        let (user, pass) = (Zeroizing::new(user), Zeroizing::new(pass));
        if pass.is_empty() {
            return self.toast(t!("Kayıtta parola yok.", "The item has no password."));
        }
        if autofill::link_app(&mut body, &ask.key) {
            match v.update(id, meta.clone(), &body) {
                Ok(()) => info!("kasa: \"{}\" kaydı {} uygulamasına bağlandı", meta.name, ask.key),
                Err(e) => warn!("kasa: uygulama bağı kaydedilemedi: {e}"),
            }
            self.write_apps();
        }
        let name = ask.name();
        self.toast(t!(format!("{} → {name}", meta.name), format!("{} → {name}", meta.name)));
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_MINIMIZE};
            let _ = ShowWindow(self.hwnd, SW_MINIMIZE);
        }
        carlui_core::win::force_foreground(hwnd);
        crate::apptype::fill(hwnd, ask.key, user, pass);
    }

    /// Bağlı uygulama anahtarlarını kasa klasörüne yazar (değiştiyse).
    pub(super) fn write_apps(&self) {
        let Some(v) = self.vault.as_ref() else { return };
        store_apps(v.items().iter().filter_map(|it| v.decrypt(&it.id).ok()).flat_map(|b| autofill::body_apps(&b).collect::<Vec<_>>()).collect());
    }
}

/// Uygulama anahtarlarını `apps_file`'a yazar (değiştiyse).
pub(super) fn store_apps(mut keys: Vec<String>) {
    {
        keys.sort();
        keys.dedup();
        if carlui_core::vaultlink::linked_apps() == keys {
            return;
        }
        let path = carlui_core::vaultlink::apps_file();
        let json = serde_json::to_vec(&keys).unwrap_or_default();
        if let Err(e) = std::fs::write(&path, json) {
            warn!("kasa: uygulama listesi yazılamadı: {e}");
        }
    }
}
