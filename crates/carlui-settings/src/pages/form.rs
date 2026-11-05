//! Bu modül "sayfa içinde iki alanlı küçük bir form nasıl çizilir?" sorununu
//! çözüyor: arama motoru ekleme ve metin genişletici kısaltması ekleme aynı
//! formu kullanır. Form açılırken yüksekliği yaylanarak büyür, alanlar sonra
//! belirir; yazma `input.rs`'de işlenir.

use crate::{
    input::Form,
    ui::{Flow, P},
    Act, App,
};
use carlui_ui::{
    paint::{Align, R, TEXT},
    tween,
};

/// `kind` türündeki form açıksa (ya da kapanıyorsa) çizer.
pub fn draw(app: &App, p: &mut P, f: &mut Flow, kind: Form, fields: &[(&str, &str)]) {
    let open = app.input.as_ref().is_some_and(|i| i.kind == kind);
    let t = tween::to(tween::key(("input-form", kind as u8)), if open { 1.0 } else { 0.0 }, 14.0);
    if t <= 0.01 {
        return;
    }
    f.flush(p);
    let full = 66.0 + 62.0 * fields.len() as f32 + if app.input.as_ref().is_some_and(|i| i.error.is_some()) { 22.0 } else { 0.0 };
    let card = R::new(f.x, f.y, f.w, full * t);
    p.round(card, 8.0, 0xFFFFFF, 0.05 * t);
    p.outline(card, 8.0, p.accent, 0.35 * t);
    f.y += full * t + 6.0;
    let Some(inp) = app.input.as_ref().filter(|i| i.kind == kind) else { return };
    if t < 0.6 {
        return;
    }
    let a = ((t - 0.6) / 0.4).clamp(0.0, 1.0);
    let blink = (inp.since.elapsed().as_millis() / 530) % 2 == 0;
    let x = card.x + 16.0;
    let fw = card.w - 32.0;
    let mut y = card.y + 14.0;
    for (k, ((label, hint), val)) in fields.iter().zip([&inp.name, &inp.url]).enumerate() {
        p.text(label, R::new(x, y, fw, 18.0), 12.0, false, TEXT, 0.6 * a, Align::Left);
        let field = R::new(x, y + 20.0, fw, 34.0);
        let focus = inp.field as usize == k;
        let ft = tween::to(tween::key(("field", k)), if focus { 1.0 } else { 0.0 }, 18.0);
        p.round(field, 6.0, 0x000000, (0.18 + 0.08 * ft) * a);
        p.outline(field, 6.0, carlui_ui::paint::mix(0xFFFFFF, p.accent, ft), (0.12 + 0.5 * ft) * a);
        let tr = R::new(field.x + 10.0, field.y, field.w - 20.0, field.h);
        // Çok satırlı metin tek satırda ⏎ ile gösterilir.
        let shown = val.replace('\n', " ⏎ ");
        if val.is_empty() {
            p.text(hint, tr, 13.0, false, TEXT, 0.3 * a, Align::Left);
        } else {
            p.text(&shown, tr, 13.0, false, TEXT, 0.95 * a, Align::Left);
        }
        if focus && blink {
            let cx = (tr.x + p.fonts.width(&shown, 13.0, false) + 1.0).min(tr.right());
            p.fill(R::new(cx, field.y + 9.0, 1.5, field.h - 18.0), p.accent, a);
        }
        p.hit(field, Act::Input(1 + k as u8));
        y += 62.0;
    }
    if let Some(e) = &inp.error {
        p.text(e, R::new(x, y - 4.0, fw, 18.0), 12.0, false, 0xFF9F6B, a, Align::Left);
        y += 22.0;
    }
    let ok = R::new(card.right() - 16.0 - 104.0, y, 104.0, 32.0);
    p.button(ok, Some('\u{E73E}'), t!("Ekle", "Add"), Act::Input(3), true, false);
    p.button(R::new(ok.x - 8.0 - 104.0, y, 104.0, 32.0), None, t!("Vazgeç", "Cancel"), Act::Input(4), true, false);
    let keys = if fields.len() > 1 { t!("Tab: alan değiştir · Ctrl+V: yapıştır · Enter: ekle · Esc: vazgeç", "Tab: next field · Ctrl+V: paste · Enter: add · Esc: cancel") } else { t!("Ctrl+V: yapıştır · Enter: ekle · Esc: vazgeç", "Ctrl+V: paste · Enter: add · Esc: cancel") };
    p.text(keys, R::new(x, y, ok.x - 8.0 - 104.0 - x - 8.0, 32.0), 11.5, false, TEXT, 0.4 * a, Align::Left);
}
