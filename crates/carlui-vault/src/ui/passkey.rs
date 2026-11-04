//! Tarayıcıdan gelen passkey isteğinin onay kartı (bkz. `app::passkeys`).
//! Kasa açıkken her görünümün üstünde ortada durur ve alttaki tıklamaları
//! kapatır; kilitliyken kilit kartının üstünde kısa bir şerit "kilidi aç"
//! der. Kullanıcı onaylamadan anahtar oluşturulmaz ya da imza atılmaz.

use super::{para, primary, P};
use crate::app::{passkeys::AskKind, Act, App};
use carlui_core::t;
use carlui_ui::paint::{Align, R, TEXT};

pub const GLYPH: char = '\u{E8D7}';

/// Listede ve düzenleyicide kullanılan küçük "Passkey" etiketi; genişliğini döner.
pub fn badge(p: &P, x: f32, y: f32) -> f32 {
    let w = p.fonts.width("Passkey", 11.0, true) + 30.0;
    let r = R::new(x, y, w, 20.0);
    p.round(r, 10.0, p.accent, 0.16);
    p.glyph(GLYPH, R::new(r.x + 6.0, r.y, 14.0, r.h), 10.0, p.accent, 1.0);
    p.text("Passkey", R::new(r.x + 22.0, r.y, w - 24.0, r.h), 11.0, true, p.accent, 1.0, Align::Left);
    w
}

pub fn paint(app: &App, p: &mut P, w: f32, h: f32) {
    let Some(ask) = &app.passkey else { return };
    let create = matches!(ask.kind, AskKind::Create { .. });
    if !app.is_open() {
        // Kilit ekranının üstünde şerit; tıklamalar kilit kartına gider.
        let r = R::new(16.0, 14.0, w - 32.0, 44.0);
        p.round(r, 10.0, p.accent, 0.16);
        p.outline(r, 10.0, p.accent, 0.45);
        p.glyph(GLYPH, R::new(r.x + 12.0, r.y, 22.0, r.h), 14.0, p.accent, 1.0);
        let s = if create {
            t!(format!("{} passkey kaydetmek istiyor · kilidi açınca onaylayacaksın", ask.site()), format!("{} wants to save a passkey · confirm after unlocking", ask.site()))
        } else {
            t!(format!("{} passkey ile giriş istiyor · kilidi açınca onaylayacaksın", ask.site()), format!("{} wants to sign in with a passkey · confirm after unlocking", ask.site()))
        };
        p.text(&s, R::new(r.x + 42.0, r.y, r.w - 200.0, r.h), 13.0, false, TEXT, 0.92, Align::Left);
        let b = R::new(r.right() - 150.0, r.y + 7.0, 140.0, 30.0);
        p.button(b, None, t!("Tarayıcıya bırak", "Use browser"), Act::PasskeyDecline, true, false);
        return;
    }

    // Alttaki görünümün tıklamaları kapanır.
    p.hits.clear();
    p.fill(R::new(0.0, 0.0, w, h), 0x000000, 0.55);
    let rows = if create { 0 } else { ask.choices.len().min(5) };
    let height = 260.0 + rows as f32 * 52.0;
    let cwid = 480.0_f32.min(w - 32.0);
    let r = R::new((w - cwid) / 2.0, ((h - height) / 2.0).max(16.0), cwid, height);
    p.round(r, 12.0, 0x1A1A20, 0.97);
    p.outline(r, 12.0, 0xFFFFFF, 0.12);
    let x = r.x + 28.0;
    let cw = r.w - 56.0;
    p.round(R::new(x, r.y + 26.0, 44.0, 44.0), 12.0, p.accent, 0.18);
    p.glyph(GLYPH, R::new(x, r.y + 26.0, 44.0, 44.0), 20.0, p.accent, 1.0);
    let title = if create { t!("Passkey kaydedilsin mi?", "Save a passkey?") } else { t!("Passkey ile giriş", "Sign in with a passkey") };
    p.text(title, R::new(x + 58.0, r.y + 24.0, cw - 58.0, 26.0), 18.0, true, TEXT, 0.95, Align::Left);
    p.text(ask.site(), R::new(x + 58.0, r.y + 50.0, cw - 58.0, 20.0), 13.0, false, p.accent, 1.0, Align::Left);
    let mut y = r.y + 90.0;

    match &ask.kind {
        AskKind::Create { user_name, user_display, .. } => {
            let who = if user_display.trim().is_empty() || user_display == user_name { user_name.clone() } else { format!("{user_display} ({user_name})") };
            p.text(t!("Hesap", "Account"), R::new(x, y, cw, 18.0), 12.0, true, TEXT, 0.6, Align::Left);
            p.text(&who, R::new(x, y + 18.0, cw, 24.0), 14.0, false, TEXT, 0.95, Align::Left);
            y += 52.0;
            let s = match ask.choices.first() {
                Some(c) => t!(format!("\"{}\" kaydına eklenir. Özel anahtar kasada şifreli saklanır.", c.name), format!("Added to \"{}\". The private key is stored encrypted in the vault.", c.name)),
                None => t!("Yeni bir giriş kaydı oluşturulur. Özel anahtar kasada şifreli saklanır.", "A new login item is created. The private key is stored encrypted in the vault.").to_string(),
            };
            para(p, &s, x, y, cw, 12.5, TEXT, 0.6);
            let by = r.bottom() - 58.0;
            primary(p, R::new(r.right() - 28.0 - 150.0, by, 150.0, 36.0), t!("Passkey kaydet", "Save passkey"), Act::PasskeyApprove(0), true);
            p.button(R::new(x, by, 150.0, 36.0), None, t!("Tarayıcıya bırak", "Use browser"), Act::PasskeyDecline, true, false);
        }
        AskKind::Get { .. } => {
            p.text(t!("Hangi hesapla?", "Which account?"), R::new(x, y, cw, 18.0), 12.0, true, TEXT, 0.6, Align::Left);
            y += 24.0;
            for (i, c) in ask.choices.iter().take(rows).enumerate() {
                let row = R::new(x, y, cw, 46.0);
                let hot = p.hover(row);
                p.round(row, 8.0, 0xFFFFFF, if hot { 0.10 } else { 0.05 });
                p.text(&c.user, R::new(row.x + 14.0, row.y + 5.0, row.w - 140.0, 20.0), 13.5, true, TEXT, 0.95, Align::Left);
                p.text(&c.name, R::new(row.x + 14.0, row.y + 24.0, row.w - 140.0, 18.0), 12.0, false, TEXT, 0.55, Align::Left);
                let b = R::new(row.right() - 116.0, row.y + 7.0, 106.0, 32.0);
                primary(p, b, t!("Giriş yap", "Sign in"), Act::PasskeyApprove(i), true);
                p.hit(row, Act::PasskeyApprove(i));
                y += 52.0;
            }
            p.button(R::new(x, r.bottom() - 58.0, 150.0, 36.0), None, t!("Tarayıcıya bırak", "Use browser"), Act::PasskeyDecline, true, false);
        }
    }
    let hint = t!("Enter: onayla · Esc: tarayıcıya bırak", "Enter: confirm · Esc: use browser");
    p.text(hint, R::new(r.x, r.bottom() + 8.0, r.w, 18.0), 11.5, false, TEXT, 0.45, Align::Center);
}
