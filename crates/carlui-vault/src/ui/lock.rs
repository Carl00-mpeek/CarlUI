//! Kasa kapalıyken görünen ekranlar: ilk kurulum, kurtarma anahtarını
//! gösterme, kilit, kurtarma anahtarıyla açma ve yeni parola. Hepsi ortada
//! tek bir kart. Uyarılar açıkça yazılır: parola ve kurtarma anahtarı
//! kaybedilirse veri kurtarılamaz.

use super::{input, link, para, primary, P};
use crate::app::{Act, App, Focus};
use carlui_core::t;
use carlui_ui::paint::{Align, ORANGE, R, TEXT};

const CARD_W: f32 = 460.0;

/// Ortalanmış kart; içeriğin başladığı (x, y, w) döner.
fn card(p: &mut P, w: f32, h: f32, height: f32, glyph: char, title: &str) -> (f32, f32, f32) {
    let r = R::new(((w - CARD_W) / 2.0).max(16.0), ((h - height) / 2.0).max(16.0), CARD_W.min(w - 32.0), height);
    p.round(r, 12.0, 0x1A1A20, 0.55);
    p.outline(r, 12.0, 0xFFFFFF, 0.10);
    let x = r.x + 32.0;
    let cw = r.w - 64.0;
    p.round(R::new(x, r.y + 30.0, 44.0, 44.0), 12.0, p.accent, 0.18);
    p.glyph(glyph, R::new(x, r.y + 30.0, 44.0, 44.0), 20.0, p.accent, 1.0);
    p.text(title, R::new(x + 58.0, r.y + 30.0, cw - 58.0, 44.0), 20.0, true, TEXT, 0.95, Align::Left);
    (x, r.y + 92.0, cw)
}

fn error_line(app: &App, p: &P, x: f32, y: f32, w: f32) -> f32 {
    let msg = match app.wait_left() {
        Some(s) => Some(t!(format!("Çok fazla yanlış deneme. {s} sn bekleyin."), format!("Too many wrong attempts. Wait {s} s."))),
        None => app.error.clone(),
    };
    match msg {
        Some(m) => para(p, &m, x, y, w, 12.5, ORANGE, 0.95) + 6.0,
        None => 0.0,
    }
}

pub fn setup(app: &App, p: &mut P, w: f32, h: f32) -> f32 {
    let (x, mut y, cw) = card(p, w, h, 410.0, '\u{E72E}', t!("Carl Vault'u kur", "Set up Carl Vault"));
    y += para(p, t!("Ana parolayı yalnızca sen belirlersin. Bu bilgisayarda kalır; hiçbir yere gönderilmez ve saklanmaz.", "Only you choose the master password. It stays on this computer; it is never sent anywhere or stored."), x, y, cw, 13.0, TEXT, 0.7);
    y += para(p, t!("Unutursan CarlUI onu sıfırlayamaz.", "If you forget it, CarlUI can't reset it."), x, y, cw, 13.0, ORANGE, 0.9) + 14.0;
    input(p, app, R::new(x, y, cw, 38.0), Focus::Form(0), true, t!("Ana parola (en az 8 karakter)", "Master password (at least 8 characters)"));
    y += 46.0;
    input(p, app, R::new(x, y, cw, 38.0), Focus::Form(1), true, t!("Parolayı tekrar yaz", "Type the password again"));
    y += 52.0;
    p.toggle(x, y, t!("Kurtarma anahtarı oluştur (önerilir)", "Create a recovery key (recommended)"), app.want_recovery, Act::ToggleRecovery);
    y += 38.0;
    y += error_line(app, p, x, y, cw);
    primary(p, R::new(x, y, cw, 40.0), t!("Kasayı oluştur", "Create vault"), Act::Create, true);
    0.0
}

pub fn show_recovery(_app: &App, p: &mut P, w: f32, h: f32, key: &str) -> f32 {
    let (x, mut y, cw) = card(p, w, h, 370.0, '\u{E8D7}', t!("Kurtarma anahtarın", "Your recovery key"));
    y += para(p, t!("Parolanı unutursan kasayı bu anahtarla açabilirsin. Yazıp güvenli bir yerde sakla; bir daha gösterilmez.", "If you forget your password you can open the vault with this key. Write it down and keep it safe; it won't be shown again."), x, y, cw, 13.0, TEXT, 0.7) + 12.0;
    let box_ = R::new(x, y, cw, 76.0);
    p.round(box_, 8.0, 0x000000, 0.30);
    p.outline(box_, 8.0, p.accent, 0.5);
    let groups: Vec<&str> = key.split('-').collect();
    let half = groups.len().div_ceil(2);
    p.text(&groups[..half].join("-"), R::new(x, y + 8.0, cw, 30.0), 15.0, true, TEXT, 0.95, Align::Center);
    p.text(&groups[half..].join("-"), R::new(x, y + 38.0, cw, 30.0), 15.0, true, TEXT, 0.95, Align::Center);
    y += 90.0;
    y += para(
        p,
        t!("Hem parolanı hem bu anahtarı kaybedersen verilerin kurtarılamaz. CarlUI dahil kimse kasayı açamaz.", "If you lose both your password and this key, your data can't be recovered. Nobody, CarlUI included, can open the vault."),
        x,
        y,
        cw,
        13.0,
        ORANGE,
        0.9,
    ) + 14.0;
    p.button(R::new(x, y, (cw - 10.0) / 2.0, 40.0), Some('\u{E8C8}'), t!("Kopyala", "Copy"), Act::CopyRecovery, true, false);
    primary(p, R::new(x + (cw + 10.0) / 2.0, y, (cw - 10.0) / 2.0, 40.0), t!("Sakladım, devam et", "Saved it, continue"), Act::RecoveryDone, true);
    0.0
}

pub fn locked(app: &App, p: &mut P, w: f32, h: f32) -> f32 {
    let Some(v) = app.vault.as_ref() else {
        // Dosya okunamadı: üzerine yazmamak için hiçbir işlem önerilmez.
        let (x, mut y, cw) = card(p, w, h, 260.0, '\u{E7BA}', t!("Kasa açılamıyor", "Can't open the vault"));
        y += error_line(app, p, x, y, cw) + 6.0;
        para(p, &carlui_vault::vault::default_dir().join("vault.json").display().to_string(), x, y, cw, 12.0, TEXT, 0.55);
        return 0.0;
    };
    let (x, mut y, cw) = card(p, w, h, 330.0, '\u{E72E}', t!("Carl Vault kilitli", "Carl Vault is locked"));
    y += para(p, t!("Açmak için ana parolanı yaz.", "Enter your master password to open it."), x, y, cw, 13.0, TEXT, 0.7) + 12.0;
    input(p, app, R::new(x, y, cw, 38.0), Focus::Form(0), true, t!("Ana parola", "Master password"));
    y += 50.0;
    y += error_line(app, p, x, y, cw);
    primary(p, R::new(x, y, cw, 40.0), t!("Kilidi aç", "Unlock"), Act::Unlock, app.wait_left().is_none());
    y += 52.0;
    if v.has_recovery() {
        link(p, R::new(x, y, cw, 24.0), t!("Parolamı unuttum: kurtarma anahtarıyla aç", "Forgot password: use the recovery key"), Act::GoRecover);
    }
    0.0
}

pub fn recover(app: &App, p: &mut P, w: f32, h: f32) -> f32 {
    let (x, mut y, cw) = card(p, w, h, 330.0, '\u{E8D7}', t!("Kurtarma anahtarıyla aç", "Open with recovery key"));
    y += para(p, t!("Kurtarma anahtarını yaz. Açıldıktan sonra yeni bir ana parola belirlersin.", "Enter your recovery key. After it opens you'll set a new master password."), x, y, cw, 13.0, TEXT, 0.7) + 12.0;
    input(p, app, R::new(x, y, cw, 38.0), Focus::Form(0), false, "XXXX-XXXX-XXXX-…");
    y += 50.0;
    y += error_line(app, p, x, y, cw);
    primary(p, R::new(x, y, cw, 40.0), t!("Aç", "Open"), Act::Recover, app.wait_left().is_none());
    y += 52.0;
    link(p, R::new(x, y, cw, 24.0), t!("Geri", "Back"), Act::BackToLock);
    0.0
}

pub fn new_password(app: &App, p: &mut P, w: f32, h: f32) -> f32 {
    let (x, mut y, cw) = card(p, w, h, 350.0, '\u{E72E}', t!("Yeni ana parola", "New master password"));
    y += para(p, t!("Kasa kurtarma anahtarıyla açıldı. Şimdi yeni bir ana parola belirle.", "The vault was opened with the recovery key. Now set a new master password."), x, y, cw, 13.0, TEXT, 0.7) + 12.0;
    input(p, app, R::new(x, y, cw, 38.0), Focus::Form(0), true, t!("Yeni parola (en az 8 karakter)", "New password (at least 8 characters)"));
    y += 46.0;
    input(p, app, R::new(x, y, cw, 38.0), Focus::Form(1), true, t!("Parolayı tekrar yaz", "Type the password again"));
    y += 52.0;
    y += error_line(app, p, x, y, cw);
    primary(p, R::new(x, y, cw, 40.0), t!("Kaydet", "Save"), Act::SetPassword, true);
    0.0
}
