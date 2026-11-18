//! Dışa aktarma ve yedekten geri yükleme (pencere tarafı; biçimler
//! `carlui_vault::export`'ta). Dışa aktarmadan önce ana parola sorulur (yanlış
//! denemeler kilit ekranıyla aynı beklemeye sayılır). Şifreli yedek ana
//! parolayla ya da istenirse burada üretilen güçlü parolayla mühürlenir;
//! üretilen parola yalnızca bir kez gösterilir. `.carlvault` dosyası içe
//! aktarmada seçilince yedek parolası sorulur, sonra olağan önizleme açılır.

use super::*;
use carlui_vault::{
    export::{self, Kind},
    gen::{self, Options},
    import::{self, ImportError},
};

impl App {
    /// "Dışa aktar…": ana parolayı doğrula, dosya seç, yaz.
    pub(super) fn export(&mut self) {
        self.export_error = None;
        let Some(v) = self.vault.as_ref() else { return };
        if self.wait_left().is_some() {
            return;
        }
        if self.form[3].as_str().is_empty() {
            self.export_error = Some(t!("Dışa aktarmak için ana parolanı yaz.", "Enter your master password to export.").into());
            self.focus = Some(Focus::Form(3));
            return;
        }
        if !v.verify_password(self.form[3].as_str()) {
            self.form[3].clear();
            self.fail(Error::WrongKey);
            self.error = None;
            self.export_error = Some(t!("Ana parola yanlış.", "Master password is wrong.").into());
            return;
        }
        self.throttle.reset(&self.dir);
        let kind = self.export_kind;
        let generated = kind.encrypted() && self.export_generated;
        let password = if generated { gen::generate(Options { len: 24, symbols: true }) } else { Zeroizing::new(self.form[3].as_str().to_string()) };
        self.form[3].clear();

        let stamp = crate::ui::date(now()).split('.').rev().collect::<Vec<_>>().join("-");
        let name = format!("CarlVault-{stamp}.{}", kind.ext());
        let Some(mut path) = dialog::save_as(self.hwnd, &name, kind.ext()) else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension(kind.ext());
        }
        let Some(v) = self.vault.as_ref() else { return };
        let collected = match export::collect(v, kind.encrypted()) {
            Ok(c) => c,
            Err(e) => {
                self.export_error = Some(e.to_string());
                return;
            }
        };
        let data: Result<Zeroizing<Vec<u8>>, Error> = match kind {
            Kind::Encrypted => export::encrypt(&collected.entries, &password).map(Zeroizing::new),
            Kind::Csv => Ok(Zeroizing::new(export::to_csv(&collected.entries).as_bytes().to_vec())),
            Kind::Json => export::to_json(&collected.entries).map(|s| Zeroizing::new(s.as_bytes().to_vec())),
        };
        let written = data.and_then(|d| std::fs::write(&path, &d[..]).map_err(|e| Error::Io(e.to_string())));
        if let Err(e) = written {
            warn!("dışa aktarma: {e}");
            self.export_error = Some(e.to_string());
            return;
        }
        let n = collected.entries.len();
        info!("dışa aktarıldı: {n} kayıt ({kind:?}, {} ek dosya okunamadı)", collected.missing_files);
        let missing = match collected.missing_files {
            0 => String::new(),
            m => t!(format!(" {m} ek dosya okunamadığı için yedeğe girmedi."), format!(" {m} attachments couldn't be read and weren't included.")),
        };
        let msg = if kind.encrypted() {
            t!(format!("{n} kayıt şifreli olarak yedeklendi.{missing}"), format!("{n} items backed up, encrypted.{missing}"))
        } else {
            t!(format!("{n} kayıt ŞİFRESİZ dışa aktarıldı. İşin bitince dosyayı sil."), format!("{n} items exported UNENCRYPTED. Delete the file when you're done."))
        };
        if generated {
            self.go(View::ShowBackupPassword { password });
        }
        self.toast(msg);
    }

    /// İçe aktarmada seçilen dosya bir Carl Vault yedeğiyse parola sorulur.
    /// Okunmuş içerik parola girilene ya da vazgeçilene kadar bellekte durur.
    pub(super) fn ask_restore(&mut self, data: Zeroizing<Vec<u8>>, file: String) {
        self.restore = Some((data, file));
        self.go(View::Restore);
    }

    /// Yedek parolasıyla `.carlvault` dosyasını açıp önizler.
    pub(super) fn restore(&mut self) {
        let Some((data, file)) = self.restore.as_ref() else {
            return;
        };
        if self.form[3].as_str().is_empty() {
            return;
        }
        let r = export::open(data, self.form[3].as_str());
        self.form[3].clear();
        match r {
            Ok(r) if r.batch.items.is_empty() => self.error = Some(ImportError::Empty.to_string()),
            Ok(r) => {
                let file = file.clone();
                self.restore = None;
                info!("yedek açıldı: {} kayıt", r.batch.items.len());
                let mut p = self.preview(r.batch, file);
                p.files = r.files;
                p.backup = true;
                self.go(View::Import(Box::new(p)));
            }
            Err(Error::WrongKey) => self.error = Some(t!("Yedek parolası yanlış ya da dosya değiştirilmiş.", "The backup password is wrong or the file was modified.").into()),
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    /// Okunan dosya çok büyük mü (yedek için sınır daha geniş).
    pub(super) fn too_big(size: u64, backup: bool) -> bool {
        size > if backup { export::MAX_BACKUP } else { import::MAX_FILE as u64 }
    }
}
