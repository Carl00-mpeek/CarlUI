//! Tarayıcıda gönderilen giriş/kayıt formu (eklenti, `Request::Save`). Eklenti
//! önce yalnızca durumu sorar (`commit` false): bu site ve kullanıcı adıyla
//! kayıt yoksa "new", parola farklıysa "changed", aynısı varsa "same". Sayfada
//! "Kaydet"/"Güncelle"ye basılınca aynı bilgi `commit` true ile gelir; ancak o
//! zaman kasaya yazılır; yeni kayıt şeritte kullanıcının verdiği adla (önerilen:
//! site adı) eklenir. Kullanıcı onaylamadan hiçbir şey kaydedilmez.

use super::*;
use carlui_vault::autofill::{self, Capture};

/// Formdan gelen alanların üst sınırı (boru satırı 8 KB).
const MAX_LEN: usize = 1024;
/// Kayıt adının üst sınırı (karakter).
const MAX_NAME: usize = 120;

impl App {
    pub(super) fn save_login(&mut self, url: &str, user: Zeroizing<String>, password: Zeroizing<String>, commit: bool, name: &str) -> Reply {
        let browser = match self.browser_gate() {
            Ok(b) => b,
            Err(r) => return r,
        };
        let user = user.trim();
        if password.is_empty() || password.len() > MAX_LEN || user.len() > MAX_LEN || autofill::origin(url).is_none() {
            return Reply::no(t!("Kaydedilecek giriş bilgisi yok.", "No login to save."));
        }
        let Some(v) = self.vault.as_ref() else { return Reply::no("") };
        // Kayıtlar geçici olarak çözülür; `Body` düşerken bellekten silinir.
        let bodies: Vec<(String, Body)> = v.items().iter().filter(|i| matches!(i.meta.category, Category::Password | Category::Email)).filter_map(|i| Some((i.id.clone(), v.decrypt(&i.id).ok()?))).collect();
        let found = autofill::classify(bodies.iter().map(|(id, b)| (id.as_str(), b)), url, user, &password);
        let status = match &found {
            Capture::New => "new",
            Capture::Changed(_) => "changed",
            Capture::Same => "same",
        };
        // Şeritte görünecek ad: yeni kayıtta önerilen, değişmişte mevcut kaydın adı.
        let shown = match &found {
            Capture::Changed(id) => v.items().iter().find(|i| &i.id == id).map(|i| i.meta.name.clone()).unwrap_or_default(),
            _ => autofill::site_name(url),
        };
        if !commit || found == Capture::Same {
            return Reply { ok: true, status: status.into(), name: shown, ..Default::default() };
        }
        let host = autofill::origin(url).unwrap_or_default();
        let host = host.split("://").nth(1).unwrap_or_default().to_string();
        let v = self.vault.as_mut().expect("kasa açık");
        let result = match &found {
            Capture::Changed(id) => {
                let Some((_, mut body)) = bodies.into_iter().find(|(i, _)| i == id) else { return Reply::no(t!("Kayıt bulunamadı.", "Item not found.")) };
                autofill::set_password(&mut body, &password);
                let (name, category) = v.items().iter().find(|i| &i.id == id).map(|i| (i.meta.name.clone(), i.meta.category)).unwrap_or((String::new(), Category::Password));
                v.update(id, Meta { name: name.clone(), category, updated: now() }, &body).map(|_| name)
            }
            _ => {
                let (mut meta, body) = autofill::new_login(url, user, &password);
                // Kullanıcının yazdığı ad (satır sonu ve aşırı uzunluk atılır).
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
                info!("kasa: {browser} içinde {host} için giriş bilgisi kaydedildi ({status})");
                let (tr, en) = if status == "changed" { ("parolası güncellendi", "password updated") } else { ("kaydedildi", "saved") };
                let message = t!(format!("{name} {tr}."), format!("{name} {en}."));
                self.toast(message.clone());
                Reply { ok: true, status: status.into(), message, ..Default::default() }
            }
            Err(e) => {
                warn!("kasa: tarayıcıdan kayıt yazılamadı: {e}");
                Reply::no(e.to_string())
            }
        }
    }
}
