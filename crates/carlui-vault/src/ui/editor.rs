//! Düzenleyici: yalnızca bu kaydın alanları çözülmüş halde burada durur.
//! Gizli alanlar varsayılan olarak maskelidir; göz simgesi yalnızca o alanı
//! açar. Her alan ayrı kopyalanır. Kaydet/Vazgeç/kilit düzenleyiciyi kapatır
//! ve çözülen değerler bellekten silinir.

use super::{clipped, icon_button, input, list::column, para, primary, P};
use crate::app::{Act, App, Editor, Focus};
use carlui_core::t;
use carlui_ui::paint::{Align, ORANGE, R, TEXT};
use carlui_vault::{
    card::{self, Part},
    entry::{now, Category, FieldKind, Format},
    totp::{grouped, Totp},
};

/// 2FA alanının altı: o anki kod, kalan süre çubuğu ya da anahtar hatası.
/// Kopyala düğmesi bu alanda anahtarı değil kodu kopyalar.
fn totp_row(p: &mut P, value: &str, x: f32, y: f32, cw: f32) {
    match Totp::parse(value) {
        Ok(t) => {
            let at = now();
            let left = t.remaining(at);
            p.text(&grouped(&t.code(at)), R::new(x + 2.0, y, 150.0, 28.0), 21.0, true, p.accent, 1.0, Align::Left);
            let s = t!(format!("{left} sn · kopyala düğmesi kodu kopyalar"), format!("{left} s · the copy button copies the code"));
            p.text(&s, R::new(x + 158.0, y, cw - 158.0, 28.0), 12.0, false, TEXT, 0.55, Align::Left);
            let bar = R::new(x + 2.0, y + 30.0, 140.0, 3.0);
            p.round(bar, 1.5, 0xFFFFFF, 0.08);
            let col = if left <= 5 { ORANGE } else { p.accent };
            p.round(R::new(bar.x, bar.y, bar.w * left as f32 / t.period as f32, bar.h), 1.5, col, 0.9);
        }
        Err(e) => p.text(&e.to_string(), R::new(x + 2.0, y, cw, 28.0), 12.5, false, ORANGE, 0.95, Align::Left),
    }
}

/// Kart alanının altındaki kısa bilgi: kart türü, numara hatası, süresi geçmiş.
fn card_hint(fmt: Format, stored: &str) -> Option<(String, u32)> {
    match fmt {
        Format::CardNumber if !stored.is_empty() => {
            let brand = card::brand(stored);
            let full = stored.len() >= card::max_digits(fmt, stored).min(16);
            match (brand, full, card::luhn(stored)) {
                (_, true, false) => Some((t!("Numara geçersiz görünüyor; bir rakam yanlış olabilir.", "The number looks invalid; a digit may be wrong.").into(), ORANGE)),
                (Some(b), _, _) => Some((b.to_string(), TEXT)),
                _ => None,
            }
        }
        Format::Expiry if card::expired(stored, now()) => Some((t!("Bu kartın süresi geçmiş.", "This card has expired.").into(), ORANGE)),
        _ => None,
    }
}

/// "Hızlı kopyala": sitelerin istediği farklı dizilimler (ayrı ay/yıl, AA/YY,
/// AA/YYYY, boşluklu numara…) tek tıkla panoya. Kullanılan yüksekliği ekleyip yeni y döner.
fn quick_copy(p: &mut P, x: f32, mut y: f32, cw: f32) -> f32 {
    y += 4.0;
    p.text(t!("Hızlı kopyala", "Quick copy"), R::new(x + 2.0, y, cw, 20.0), 12.0, true, TEXT, 0.6, Align::Left);
    y += 24.0;
    let mut cx = x;
    for part in Part::ALL {
        let label = part.title();
        let bw = p.fonts.width(label, 12.5, false) + 26.0;
        if cx + bw > x + cw {
            cx = x;
            y += 38.0;
        }
        let r = R::new(cx, y, bw, 30.0);
        p.round(r, 15.0, 0xFFFFFF, if p.hover(r) { 0.12 } else { 0.06 });
        p.text(label, r, 12.5, false, TEXT, 0.88, Align::Center);
        p.hit(r, Act::CopyPart(part));
        cx += bw + 6.0;
    }
    y + 44.0
}

fn size_text(n: u64) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        format!("{:.0} KB", n as f64 / 1024.0)
    } else {
        format!("{:.1} MB", n as f64 / 1048576.0)
    }
}

pub fn paint(app: &App, e: &Editor, p: &mut P, w: f32, h: f32) -> f32 {
    let (x, cw) = column(w);
    let title = if e.id.is_some() { t!("Kaydı düzenle", "Edit item") } else { t!("Yeni kayıt", "New item") };
    p.text(title, R::new(x, 18.0, cw - 360.0, 38.0), 22.0, true, TEXT, 0.95, Align::Left);
    let mut bx = x + cw - 110.0;
    primary(p, R::new(bx, 20.0, 110.0, 34.0), t!("Kaydet", "Save"), Act::Save, true);
    bx -= 106.0;
    p.button(R::new(bx, 20.0, 98.0, 34.0), None, t!("Vazgeç", "Cancel"), Act::Cancel, true, false);
    if e.id.is_some() {
        bx -= 118.0;
        let label = if e.confirm_delete { t!("Emin misin?", "Sure?") } else { t!("Sil", "Delete") };
        p.button(R::new(bx, 20.0, 110.0, 34.0), Some('\u{E74D}'), label, Act::Delete, true, true);
    }

    let clip = R::new(super::SIDEBAR, 70.0, w - super::SIDEBAR, h - 70.0);
    let mut y = clip.y + 6.0 - app.scroll;
    clipped(p, clip, |p| {
        if let Some(err) = &app.error {
            y += para(p, err, x, y, cw, 12.5, ORANGE, 0.95) + 6.0;
        }
        p.text(t!("Ad", "Name"), R::new(x + 2.0, y, cw, 20.0), 12.0, true, TEXT, 0.6, Align::Left);
        y += 22.0;
        input(p, app, R::new(x, y, cw, 38.0), Focus::Name, false, t!("Ör. GitHub, Ziraat kartı", "E.g. GitHub, bank card"));
        y += 50.0;

        // Kategori: yeni kayıtta seçilebilir, mevcut kayıtta yalnızca gösterilir.
        p.text(t!("Kategori", "Category"), R::new(x + 2.0, y, cw, 20.0), 12.0, true, TEXT, 0.6, Align::Left);
        y += 22.0;
        if e.id.is_none() {
            let per = 4;
            let chip_w = (cw - 8.0 * (per as f32 - 1.0)) / per as f32;
            for (i, c) in Category::ALL.iter().enumerate() {
                let r = R::new(x + (i % per) as f32 * (chip_w + 8.0), y + (i / per) as f32 * 42.0, chip_w, 34.0);
                let sel = e.category == *c;
                p.round(r, 7.0, if sel { p.accent } else { 0xFFFFFF }, if sel { 0.85 } else if p.hover(r) { 0.10 } else { 0.05 });
                let col = if sel { 0x000000 } else { TEXT };
                p.glyph(c.glyph(), R::new(r.x + 8.0, r.y, 22.0, r.h), 13.0, col, 0.9);
                p.text(c.title(), R::new(r.x + 34.0, r.y, r.w - 40.0, r.h), 12.5, sel, col, 0.9, Align::Left);
                p.hit(r, Act::SetCategory(*c));
            }
            y += 92.0;
        } else {
            p.glyph(e.category.glyph(), R::new(x, y, 22.0, 26.0), 13.0, p.accent, 1.0);
            p.text(e.category.title(), R::new(x + 28.0, y, cw, 26.0), 13.0, false, TEXT, 0.85, Align::Left);
            y += 38.0;
        }

        p.text(t!("Alanlar", "Fields"), R::new(x + 2.0, y, cw, 20.0), 12.0, true, TEXT, 0.6, Align::Left);
        y += 24.0;
        for (i, f) in e.fields.iter().enumerate() {
            // Alan adı: küçük, tıklayınca düzenlenir.
            let lr = R::new(x, y, (cw * 0.5).min(320.0), 26.0);
            if app.focus == Some(Focus::Label(i)) {
                input(p, app, lr, Focus::Label(i), false, t!("Alan adı", "Field name"));
            } else {
                p.text(f.label.as_str(), R::new(lr.x + 2.0, lr.y, lr.w, lr.h), 12.5, false, TEXT, 0.7, Align::Left);
                p.hit(lr, Act::Focus(Focus::Label(i)));
            }
            y += 28.0;
            let bw = 34.0;
            let vh = if f.kind == FieldKind::Multiline { f.value.height() } else { 38.0 };
            let vr = R::new(x, y, cw - 3.0 * (bw + 4.0) - 4.0, vh);
            let masked = f.kind.masked() && !f.shown;
            let hint = match f.kind {
                FieldKind::Url => "https://…",
                FieldKind::Multiline => t!("Ctrl+Enter: kaydet", "Ctrl+Enter: save"),
                FieldKind::Totp => t!("Base32 anahtar ya da otpauth://…", "Base32 key or otpauth://…"),
                _ => "",
            };
            input(p, app, vr, Focus::Value(i), masked, hint);
            let mut b = R::new(vr.right() + 8.0, y + 2.0, bw, bw);
            if f.kind.masked() {
                icon_button(p, b, if f.shown { '\u{ED1A}' } else { '\u{E7B3}' }, Act::Reveal(i), 0.8);
            }
            b.x += bw + 4.0;
            icon_button(p, b, '\u{E8C8}', Act::Copy(i), 0.8);
            b.x += bw + 4.0;
            icon_button(p, b, '\u{E711}', Act::RemoveField(i), 0.55);
            if f.kind == FieldKind::Totp && !f.value.text.is_empty() {
                y += vh + 6.0;
                totp_row(p, f.value.as_str(), x, y, cw);
                y += 40.0;
            } else if let Some((msg, col)) = card_hint(f.value.fmt, &f.value.value()) {
                y += vh + 4.0;
                p.text(&msg, R::new(x + 2.0, y, cw, 18.0), 12.0, false, col, 0.8, Align::Left);
                y += 26.0;
            } else {
                y += vh + 14.0;
            }
        }
        if e.category == Category::Card {
            y = quick_copy(p, x, y, cw);
        }
        p.button(R::new(x, y, 130.0, 32.0), Some('\u{E710}'), t!("Alan", "Field"), Act::AddField(FieldKind::Text), true, false);
        p.button(R::new(x + 138.0, y, 150.0, 32.0), Some('\u{E710}'), t!("Gizli alan", "Secret field"), Act::AddField(FieldKind::Secret), true, false);
        p.button(R::new(x + 296.0, y, 130.0, 32.0), Some('\u{E710}'), t!("2FA kodu", "2FA code"), Act::AddField(FieldKind::Totp), true, false);
        y += 50.0;

        if e.category == Category::File || e.file.is_some() || e.new_file.is_some() {
            p.text(t!("Dosya", "File"), R::new(x + 2.0, y, cw, 20.0), 12.0, true, TEXT, 0.6, Align::Left);
            y += 24.0;
            let r = R::new(x, y, cw, 54.0);
            p.card(r);
            let current = match (&e.new_file, &e.file) {
                (Some((n, d)), _) => Some((n.as_str(), d.len() as u64, true)),
                (None, Some(f)) if !e.drop_file => Some((f.name.as_str(), f.size, false)),
                _ => None,
            };
            p.glyph('\u{E8A5}', R::new(r.x + 12.0, r.y, 26.0, r.h), 15.0, p.accent, 1.0);
            match current {
                Some((name, size, pending)) => {
                    let s = if pending { t!(format!("{} · kaydedince şifrelenir", size_text(size)), format!("{} · encrypted on save", size_text(size))) } else { format!("{} · {}", size_text(size), t!("şifreli", "encrypted")) };
                    p.text(name, R::new(r.x + 46.0, r.y + 8.0, r.w - 360.0, 20.0), 13.5, true, TEXT, 0.92, Align::Left);
                    p.text(&s, R::new(r.x + 46.0, r.y + 28.0, r.w - 360.0, 18.0), 12.0, false, TEXT, 0.5, Align::Left);
                    p.button(R::new(r.right() - 300.0, r.y + 11.0, 96.0, 32.0), None, t!("Değiştir", "Replace"), Act::PickFile, true, false);
                    p.button(R::new(r.right() - 198.0, r.y + 11.0, 104.0, 32.0), Some('\u{EDE1}'), t!("Dışa aktar", "Export"), Act::ExportFile, true, false);
                    p.button(R::new(r.right() - 88.0, r.y + 11.0, 76.0, 32.0), None, t!("Kaldır", "Remove"), Act::RemoveFile, true, true);
                }
                None => {
                    p.text(t!("Dosya yok (en çok 25 MB)", "No file (up to 25 MB)"), R::new(r.x + 46.0, r.y, r.w - 200.0, r.h), 13.0, false, TEXT, 0.6, Align::Left);
                    p.button(R::new(r.right() - 130.0, r.y + 11.0, 118.0, 32.0), Some('\u{E8E5}'), t!("Dosya seç", "Choose file"), Act::PickFile, true, false);
                }
            }
            y += 64.0;
            y += para(p, t!("Dışa aktarılan dosya şifresiz yazılır.", "An exported file is written unencrypted."), x, y, cw, 12.0, TEXT, 0.45);
        }
        y += 8.0;
        para(p, t!("Kaydı kapatınca çözülen bilgiler bellekten silinir.", "When you close the item, its decrypted data is wiped from memory."), x, y, cw, 12.0, TEXT, 0.45);
        y += 24.0;
    });
    (y + app.scroll - clip.y + 20.0 - clip.h).max(0.0)
}
