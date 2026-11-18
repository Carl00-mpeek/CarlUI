//! Güvenlik sayfası: ana parolayı değiştir, kurtarma anahtarı oluştur/kaldır,
//! içe ve dışa aktarma (şifreli `.carlvault` yedeği ya da şifresiz CSV/JSON),
//! otomatik kilit bilgisi (süre Ayarlar > Vault'tan değişir).

use super::{clipped, input, list::column, para, primary, P};
use crate::app::{Act, App, Focus};
use carlui_core::t;
use carlui_ui::paint::{Align, GREEN, ORANGE, R, TEXT};
use carlui_vault::export::Kind;

fn section(p: &P, title: &str, x: f32, y: &mut f32, w: f32) {
    *y += 10.0;
    p.text(title, R::new(x + 2.0, *y, w, 22.0), 14.0, true, TEXT, 0.85, Align::Left);
    *y += 30.0;
}

pub fn paint(app: &App, p: &mut P, w: f32, h: f32) -> f32 {
    let (x, cw) = column(w);
    p.text(t!("Güvenlik", "Security"), R::new(x, 18.0, cw, 38.0), 22.0, true, TEXT, 0.95, Align::Left);
    let Some(v) = app.vault.as_ref() else { return 0.0 };
    let clip = R::new(super::SIDEBAR, 70.0, w - super::SIDEBAR, h - 70.0);
    let mut y = clip.y + 6.0 - app.scroll;
    clipped(p, clip, |p| {
        section(p, t!("Ana parola", "Master password"), x, &mut y, cw);
        let fw = cw.min(420.0);
        input(p, app, R::new(x, y, fw, 38.0), Focus::Form(2), true, t!("Mevcut parola", "Current password"));
        y += 46.0;
        input(p, app, R::new(x, y, fw, 38.0), Focus::Form(0), true, t!("Yeni parola (en az 8 karakter)", "New password (at least 8 characters)"));
        y += 46.0;
        input(p, app, R::new(x, y, fw, 38.0), Focus::Form(1), true, t!("Yeni parolayı tekrar yaz", "Type the new password again"));
        y += 50.0;
        if let Some(s) = app.wait_left() {
            y += para(p, &t!(format!("Çok fazla yanlış deneme. {s} sn bekleyin."), format!("Too many wrong attempts. Wait {s} s.")), x, y, fw, 12.5, ORANGE, 0.95) + 4.0;
        } else if let Some(e) = &app.error {
            y += para(p, e, x, y, fw, 12.5, ORANGE, 0.95) + 4.0;
        }
        primary(p, R::new(x, y, 180.0, 36.0), t!("Parolayı değiştir", "Change password"), Act::ChangePassword, app.wait_left().is_none());
        y += 50.0;

        section(p, t!("Kurtarma anahtarı", "Recovery key"), x, &mut y, cw);
        let (msg, col) = if v.has_recovery() { (t!("Kurtarma anahtarı var.", "A recovery key is set."), GREEN) } else { (t!("Kurtarma anahtarı yok.", "No recovery key."), ORANGE) };
        p.text(msg, R::new(x + 2.0, y, cw, 22.0), 13.0, true, col, 0.95, Align::Left);
        y += 28.0;
        y += para(
            p,
            t!(
                "Parolanı unutursan kasayı yalnızca bu anahtar açar. Yeni anahtar oluşturunca eskisi geçersiz olur. Hem parolayı hem anahtarı kaybedersen verilerin kurtarılamaz.",
                "If you forget your password, only this key opens the vault. Creating a new key invalidates the old one. If you lose both the password and the key, your data can't be recovered."
            ),
            x,
            y,
            cw,
            12.5,
            TEXT,
            0.6,
        ) + 10.0;
        let label = if v.has_recovery() { t!("Yeni anahtar oluştur", "Create a new key") } else { t!("Anahtar oluştur", "Create key") };
        p.button(R::new(x, y, 190.0, 34.0), Some('\u{E8D7}'), label, Act::NewRecovery, true, false);
        if v.has_recovery() {
            let l = if app.confirm_remove_recovery { t!("Emin misin?", "Sure?") } else { t!("Anahtarı kaldır", "Remove key") };
            p.button(R::new(x + 200.0, y, 160.0, 34.0), None, l, Act::RemoveRecovery, true, true);
        }
        y += 50.0;

        section(p, t!("İçe aktar", "Import"), x, &mut y, cw);
        y += para(
            p,
            t!(
                "Bir Carl Vault yedeğini (.carlvault) ya da başka bir parola yöneticisinin veya 2FA uygulamasının dışa aktarım dosyasını seç. Önce bir önizleme görürsün; kasada zaten olan kayıtlar tekrar eklenmez.",
                "Pick a Carl Vault backup (.carlvault) or an export file from another password manager or 2FA app. You'll see a preview first; items already in the vault aren't added again."
            ),
            x,
            y,
            cw,
            12.5,
            TEXT,
            0.6,
        ) + 4.0;
        y += para(
            p,
            t!(
                "Parola: Bitwarden (JSON/CSV), Chrome, Edge, Brave, Firefox, 1Password, LastPass, KeePass, Proton Pass, Dashlane, NordPass (CSV). 2FA: Aegis, 2FAS, andOTP (şifresiz yedek), otpauth listesi; Google Authenticator için aktarım QR'ının ekran görüntüsünü al.",
                "Passwords: Bitwarden (JSON/CSV), Chrome, Edge, Brave, Firefox, 1Password, LastPass, KeePass, Proton Pass, Dashlane, NordPass (CSV). 2FA: Aegis, 2FAS, andOTP (unencrypted backup), otpauth lists; for Google Authenticator, take a screenshot of its export QR."
            ),
            x,
            y,
            cw,
            12.0,
            TEXT,
            0.45,
        ) + 10.0;
        if let Some(e) = &app.import_error {
            y += para(p, e, x, y, cw, 12.5, ORANGE, 0.95) + 6.0;
        }
        p.button(R::new(x, y, 210.0, 34.0), Some('\u{E8B5}'), t!("Dosyadan içe aktar…", "Import from file…"), Act::Import, true, false);
        y += 50.0;

        y = export(app, p, x, y, cw);

        section(p, t!("Otomatik kilit", "Auto-lock"), x, &mut y, cw);
        let c = &app.cfg.vault;
        let mins = c.auto_lock_minutes.max(1);
        let sleep = if c.lock_on_sleep { t!(" Uyku ve oturum kilidinde de kilitlenir.", " Also locks on sleep and when the session is locked.") } else { "" };
        y += para(p, &t!(format!("{mins} dakika boşta kalınca kilitlenir.{sleep}"), format!("Locks after {mins} minutes idle.{sleep}")), x, y, cw, 13.0, TEXT, 0.75) + 8.0;
        p.button(R::new(x, y, 190.0, 34.0), Some('\u{E713}'), t!("Ayarlarda değiştir", "Change in Settings"), Act::OpenSettings, true, false);
        y += 50.0;
        para(
            p,
            t!(
                "Kasa yalnızca bu bilgisayarda, %APPDATA%\\CarlUI\\vault içinde şifreli durur. Hiçbir veri sunucuya gönderilmez. Hiçbir yazılım mutlak güvenlik sağlayamaz; bilgisayarını güncel ve temiz tut.",
                "The vault is stored encrypted only on this computer, in %APPDATA%\\CarlUI\\vault. No data is sent to a server. No software can promise absolute security; keep your computer updated and clean."
            ),
            x,
            y,
            cw,
            12.0,
            TEXT,
            0.45,
        );
        y += 60.0;
    });
    (y + app.scroll - clip.y + 20.0 - clip.h).max(0.0)
}

/// "Dışa aktar" bölümü: biçim, parola seçimi, ana parola ve düğme.
fn export(app: &App, p: &mut P, x: f32, mut y: f32, cw: f32) -> f32 {
    section(p, t!("Dışa aktar", "Export"), x, &mut y, cw);
    let kind = app.export_kind;
    let items: Vec<(&str, Act)> = Kind::ALL.iter().map(|k| (k.title(), Act::ExportKind(*k))).collect();
    let sel = Kind::ALL.iter().position(|k| *k == kind).unwrap_or(0);
    p.segments(R::new(x, y, cw.min(420.0), 34.0), &items, sel);
    y += 46.0;
    if kind.encrypted() {
        y += para(
            p,
            t!(
                "Tüm kayıtlar ve ek dosyalar tek bir şifreli dosyaya yazılır. Geri yüklemek için bu sayfadaki \"Dosyadan içe aktar…\" ile dosyayı seç.",
                "All items and attachments are written to a single encrypted file. To restore, pick the file with \"Import from file…\" on this page."
            ),
            x,
            y,
            cw,
            12.5,
            TEXT,
            0.6,
        ) + 8.0;
        p.toggle(
            x,
            y,
            t!("Yedek için güçlü bir parola üret (ana parola yerine)", "Generate a strong password for the backup (instead of the master password)"),
            app.export_generated,
            Act::ToggleExportGenerated,
        );
        y += 32.0;
        let note = if app.export_generated {
            t!("Üretilen parola yalnızca bir kez gösterilir. Onu kaybedersen yedek açılamaz.", "The generated password is shown only once. If you lose it, the backup can't be opened.")
        } else {
            t!(
                "Yedek, şu anki ana parolanla şifrelenir. Parolanı sonra değiştirsen de yedeği eski parolayla açarsın.",
                "The backup is encrypted with your current master password. If you change it later, open the backup with the old one."
            )
        };
        y += para(p, note, x, y, cw, 12.0, TEXT, 0.5) + 10.0;
    } else {
        y += para(
            p,
            &t!(
                format!("Dikkat: {} dosyası ŞİFRELENMEZ. Parolalar, 2FA anahtarları ve notlar düz metin olarak yazılır; dosyayı açan herkes okuyabilir. Ek dosyalar ve passkey'ler dahil edilmez. Başka bir uygulamaya aktardıktan sonra dosyayı hemen sil.", kind.title()),
                format!("Warning: the {} file is NOT encrypted. Passwords, 2FA secrets and notes are written as plain text; anyone who opens the file can read them. Attachments and passkeys aren't included. Delete the file as soon as you've imported it elsewhere.", kind.title())
            ),
            x,
            y,
            cw,
            12.5,
            ORANGE,
            0.95,
        ) + 10.0;
    }
    let fw = cw.min(420.0);
    input(p, app, R::new(x, y, fw, 38.0), Focus::Form(3), true, t!("Onay için ana parola", "Master password to confirm"));
    y += 50.0;
    if let Some(s) = app.wait_left() {
        y += para(p, &t!(format!("Çok fazla yanlış deneme. {s} sn bekleyin."), format!("Too many wrong attempts. Wait {s} s.")), x, y, fw, 12.5, ORANGE, 0.95) + 4.0;
    } else if let Some(e) = &app.export_error {
        y += para(p, e, x, y, fw, 12.5, ORANGE, 0.95) + 4.0;
    }
    let label = if kind.encrypted() { t!("Yedeği dışa aktar…", "Export backup…") } else { t!("Şifresiz dışa aktar…", "Export unencrypted…") };
    primary(p, R::new(x, y, 210.0, 36.0), label, Act::Export, app.wait_left().is_none());
    y + 50.0
}
