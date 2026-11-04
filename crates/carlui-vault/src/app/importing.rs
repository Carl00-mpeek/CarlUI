//! İçe aktarma akışı (pencere tarafı). Dosya seçilir, `carlui_vault::import`
//! biçimi tanıyıp kayıtları çıkarır; önizlemede kaç kayıt geleceği ve kaçının
//! kasada zaten olduğu görünür. "İçe aktar" ile yeni olanlar tek seferde
//! şifrelenip yazılır. Kasada aynısı olan kayıt (aynı ad ve aynı değerler)
//! ikinci kez eklenmez. Google Authenticator'ın aktarım QR'ı ekran görüntüsü
//! aracından gelince de aynı önizleme açılır.

use super::*;
use carlui_vault::import::{self, Batch, ImportError};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

/// Karşılaştırma anahtarının özeti (düz değerler kümede tutulmasın).
fn digest(k: &str) -> [u8; 32] {
    Sha256::digest(k.as_bytes()).into()
}

pub struct Preview {
    pub batch: Batch,
    /// Her kayıt için: kasada yoksa true (eklenecek).
    pub fresh: Vec<bool>,
    pub dups: usize,
    /// Seçilen dosyanın adı (QR'dan geldiyse boş).
    pub file: String,
}

impl Preview {
    pub fn count(&self) -> usize {
        self.fresh.iter().filter(|f| **f).count()
    }

    /// Kategori başına eklenecek kayıt sayısı.
    pub fn by_category(&self) -> Vec<(Category, usize)> {
        Category::ALL
            .iter()
            .map(|c| (*c, self.batch.items.iter().zip(&self.fresh).filter(|(i, f)| **f && i.meta.category == *c).count()))
            .filter(|(_, n)| *n > 0)
            .collect()
    }
}

impl App {
    /// Toplu aktarımın önizlemesi: kasadaki kayıtlarla karşılaştırılır.
    pub(super) fn preview(&self, batch: Batch, file: String) -> Preview {
        let mut have: HashSet<[u8; 32]> = HashSet::new();
        if let Some(v) = self.vault.as_ref() {
            for it in v.items() {
                if let Ok(body) = v.decrypt(&it.id) {
                    have.insert(digest(&import::key(it.meta.category, &it.meta.name, &body)));
                }
            }
        }
        let mut fresh = Vec::with_capacity(batch.items.len());
        for it in &batch.items {
            // Aynı dosyada iki kez geçen kayıt da bir kez eklenir.
            fresh.push(have.insert(digest(&import::key(it.meta.category, &it.meta.name, &it.body))));
        }
        let dups = fresh.iter().filter(|f| !**f).count();
        Preview { batch, fresh, dups, file }
    }

    /// "Dosyadan içe aktar…": dosyayı seç, oku, önizle.
    pub(super) fn pick_import(&mut self) {
        let Some(path) = dialog::open(self.hwnd) else { return };
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if size as usize > import::MAX_FILE {
            self.import_error = Some(ImportError::TooBig.to_string());
            return;
        }
        let data = match std::fs::read(&path) {
            Ok(d) => Zeroizing::new(d),
            Err(e) => {
                self.import_error = Some(e.to_string());
                return;
            }
        };
        match import::parse(&data) {
            Ok(batch) => {
                let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                info!("içe aktarma önizlemesi: {} ({} kayıt, {} atlandı)", batch.source, batch.items.len(), batch.skipped);
                let p = self.preview(batch, file);
                self.go(View::Import(Box::new(p)));
            }
            Err(e) => {
                warn!("içe aktarma: {e:?}");
                self.import_error = Some(e.to_string());
            }
        }
    }

    /// Önizlemedeki yeni kayıtları kasaya yazar.
    pub(super) fn confirm_import(&mut self) {
        let View::Import(p) = std::mem::replace(&mut self.view, View::List) else { return };
        let p = *p;
        let items: Vec<(Meta, Body)> = p
            .batch
            .items
            .into_iter()
            .zip(p.fresh)
            .filter(|(_, f)| *f)
            .map(|(i, _)| {
                let mut body = i.body.clone();
                carlui_vault::card::upgrade(i.meta.category, &mut body);
                (i.meta.clone(), body)
            })
            .collect();
        let Some(v) = self.vault.as_mut() else { return };
        match v.add_many(items) {
            Ok(n) => {
                info!("içe aktarıldı: {n} kayıt ({})", p.batch.source);
                self.go(View::List);
                let tip = if p.file.is_empty() { String::new() } else { t!(" Şifresiz dışa aktarım dosyasını silmeyi unutma.", " Don't forget to delete the unencrypted export file.").to_string() };
                self.toast(t!(format!("{n} kayıt içe aktarıldı.{tip}"), format!("{n} items imported.{tip}")));
            }
            Err(e) => {
                self.go(View::Security);
                self.import_error = Some(e.to_string());
            }
        }
    }

    /// Bir kez (kasa açılınca): önceki sürümün "Parolalar"a aktardığı e-posta
    /// hesaplarını (Gmail, Outlook…) "E-posta" kategorisine taşır. Kasa
    /// klasöründeki işaret dosyası tekrar yapılmasını önler; kullanıcı sonra
    /// bir kaydı elle geri taşırsa dokunulmaz.
    pub(super) fn move_mail_logins(&mut self) {
        let mark = self.dir.join("mail-category.done");
        if mark.exists() {
            return;
        }
        let Some(v) = self.vault.as_mut() else { return };
        let ids: Vec<String> = v
            .items()
            .iter()
            .filter(|i| i.meta.category == Category::Password)
            .filter(|i| v.decrypt(&i.id).is_ok_and(|b| carlui_vault::autofill::is_mail_login(&b)))
            .map(|i| i.id.clone())
            .collect();
        match v.set_categories(&ids, Category::Email) {
            Ok(n) => {
                let _ = std::fs::write(&mark, b"1");
                if n > 0 {
                    info!("kasa: {n} e-posta hesabı E-posta kategorisine taşındı");
                    self.toast(t!(format!("{n} e-posta hesabı E-posta kategorisine taşındı."), format!("{n} email accounts moved to the Email category.")));
                }
            }
            Err(e) => warn!("kasa: e-posta hesapları taşınamadı: {e}"),
        }
    }

    /// Ekran görüntüsündeki Google Authenticator aktarım QR'ı: kasa kilitliyse
    /// açılınca önizlenir. Hiçbir şey onaysız kaydedilmez.
    pub(super) fn import_migration(&mut self, uri: &str) -> Reply {
        let Some(batch) = import::gauth::parse(uri).filter(|b| !b.items.is_empty()) else {
            return Reply::no(t!("Google Authenticator aktarım kodu okunamadı.", "Couldn't read the Google Authenticator export code."));
        };
        let n = batch.items.len();
        self.batch = Some((Box::new(batch), Instant::now()));
        info!("kasa: ekran görüntüsünden Google Authenticator aktarımı geldi ({n} hesap)");
        let message = match &self.view {
            View::List => {
                self.go(View::List);
                t!(format!("Carl Vault'ta {n} hesabın önizlemesi açıldı."), format!("A preview of {n} accounts opened in Carl Vault."))
            }
            View::Edit(_) | View::Security | View::Import(_) => {
                self.toast(t!("Açık sayfayı kapatınca aktarım önizlemesi açılacak.", "The import preview opens when you close this page."));
                t!(format!("{n} hesap: Carl Vault'ta açık sayfa kapanınca önizlenecek."), format!("{n} accounts: previewed once the open page in Carl Vault is closed."))
            }
            _ => t!(format!("{n} hesap: Carl Vault'u açınca önizlenecek."), format!("{n} accounts: previewed once you unlock Carl Vault.")),
        };
        Reply { ok: true, message, ..Default::default() }
    }
}
