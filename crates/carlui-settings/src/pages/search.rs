//! Arama: adadaki arama, dosya araması, döviz çevirisi, web arama motoru.

use crate::{
    ui::{edit, Flow, P},
    Act, App,
};
use carlui_ui::{
    paint::{Align, R, TEXT},
    tween,
};

/// "+ Arama motoru ekle" satırı ve açılınca aşağı doğru büyüyen form.
fn engines_row(app: &App, p: &mut P, f: &mut Flow, custom: Option<String>) {
    let desc = if app.cfg.launcher.engines.is_empty() {
        t!("Listede olmayan bir site ekleyin: ad ve {q} içeren arama adresi.", "Add a site that isn't listed: a name and a search address containing {q}.").to_string()
    } else {
        t!(format!("{} özel motor kayıtlı. Seçili özel motor silinebilir.", app.cfg.launcher.engines.len()), format!("{} custom engines saved. The selected custom engine can be deleted.", app.cfg.launcher.engines.len()))
    };
    let r = f.row(p, t!("Arama motoru ekle", "Add search engine"), &desc, 230.0);
    let by = r.y + (r.h - 32.0) / 2.0;
    let add = R::new(r.right() - 16.0 - 104.0, by, 104.0, 32.0);
    p.button(add, Some('\u{E710}'), t!("Ekle", "Add"), Act::Input(0), app.input.is_none(), false);
    if let Some(name) = custom {
        let del = R::new(add.x - 8.0 - 104.0, by, 104.0, 32.0);
        let n = name.clone();
        p.button(
            del,
            Some('\u{E74D}'),
            t!("Sil", "Delete"),
            edit(move |c| {
                c.launcher.engines.remove(&n);
                c.launcher.web_search = ENGINES[0].1.into();
            }),
            true,
            true,
        );
    }

    // Form: açılırken yüksekliği yaylanarak büyür, alanlar sırayla belirir.
    let open = app.input.is_some();
    let t = tween::to(tween::key("engine-form"), if open { 1.0 } else { 0.0 }, 14.0);
    if t <= 0.01 {
        return;
    }
    let full = 190.0 + if app.input.as_ref().is_some_and(|i| i.error.is_some()) { 22.0 } else { 0.0 };
    let card = R::new(f.x, f.y, f.w, full * t);
    p.round(card, 8.0, 0xFFFFFF, 0.05 * t);
    p.outline(card, 8.0, p.accent, 0.35 * t);
    f.y += full * t + 6.0;
    let Some(inp) = app.input.as_ref() else { return };
    if t < 0.6 {
        return;
    }
    let a = ((t - 0.6) / 0.4).clamp(0.0, 1.0);
    let blink = (inp.since.elapsed().as_millis() / 530) % 2 == 0;
    let x = card.x + 16.0;
    let fw = card.w - 32.0;
    let mut y = card.y + 14.0;
    for (k, (label, val, hint)) in [(t!("Ad", "Name"), &inp.name, t!("ör. Wikipedia", "e.g. Wikipedia")), (t!("Arama adresi", "Search address"), &inp.url, t!("ör. https://tr.wikipedia.org/w/index.php?search={q}", "e.g. https://en.wikipedia.org/w/index.php?search={q}"))].iter().enumerate() {
        p.text(label, R::new(x, y, fw, 18.0), 12.0, false, TEXT, 0.6 * a, Align::Left);
        let field = R::new(x, y + 20.0, fw, 34.0);
        let focus = inp.field as usize == k;
        let ft = tween::to(tween::key(("field", k)), if focus { 1.0 } else { 0.0 }, 18.0);
        p.round(field, 6.0, 0x000000, (0.18 + 0.08 * ft) * a);
        p.outline(field, 6.0, carlui_ui::paint::mix(0xFFFFFF, p.accent, ft), (0.12 + 0.5 * ft) * a);
        let tr = R::new(field.x + 10.0, field.y, field.w - 20.0, field.h);
        if val.is_empty() {
            p.text(hint, tr, 13.0, false, TEXT, 0.3 * a, Align::Left);
        } else {
            p.text(val, tr, 13.0, false, TEXT, 0.95 * a, Align::Left);
        }
        if focus && blink {
            let cx = (tr.x + p.fonts.width(val, 13.0, false) + 1.0).min(tr.right());
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
    p.text(t!("Tab: alan değiştir · Ctrl+V: yapıştır · Enter: ekle · Esc: vazgeç", "Tab: next field · Ctrl+V: paste · Enter: add · Esc: cancel"), R::new(x, y, ok.x - 8.0 - 104.0 - x - 8.0, 32.0), 11.5, false, TEXT, 0.4 * a, Align::Left);
}

const ENGINES: [(&str, &str); 3] = [
    ("Google", "https://www.google.com/search?q={q}"),
    ("Bing", "https://www.bing.com/search?q={q}"),
    ("DuckDuckGo", "https://duckduckgo.com/?q={q}"),
];

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let c = &app.cfg.launcher;
    f.section(p, t!("Arama", "Search"));
    f.toggle(p, t!("CarlUI araması", "CarlUI search"), t!("Kapalıysa Windows'un kendi araması açılır.", "When off, the Windows search opens instead."), c.enabled, |c| c.launcher.enabled ^= true);
    f.toggle(p, t!("Dosya adı araması", "File name search"), t!("Kullanıcı klasörlerinde dosya adlarına bakar.", "Looks at file names in your user folders."), c.file_search, |c| c.launcher.file_search ^= true);
    f.toggle(p, t!("Döviz çevirisi", "Currency conversion"), t!("\"100 usd\" gibi yazınca güncel kurla çevirir (internet gerekir).", "Type something like \"100 usd\" to convert at the current rate (needs internet)."), c.currency, |c| c.launcher.currency ^= true);
    f.toggle(p, t!("İndirme yöneticisi", "Download manager"), t!("Dosya bağlantısı yapıştırınca ya da \"indir <adres>\" yazınca İndirilenler'e iner; yüzde ve hız adada görünür. Tarayıcı indirmeleri için aşağıdaki eklentiye bakın.", "Paste a file link or type \"download <address>\" to save it to Downloads; progress and speed show on the island. For browser downloads, see the extension below."), c.download_manager, |c| c.launcher.download_manager ^= true);

    // Hazır motorlar + "+" ile eklenenler; listede olmayan adres "Özel" görünür.
    let mut all: Vec<(String, String)> = ENGINES.iter().map(|(n, u)| (n.to_string(), u.to_string())).collect();
    all.extend(c.engines.iter().map(|(n, u)| (n.clone(), u.clone())));
    let sel = all.iter().position(|(_, u)| *u == c.web_search);
    let mut items: Vec<(&str, crate::Act)> = all
        .iter()
        .map(|(n, u)| {
            let u = u.clone();
            (n.as_str(), edit(move |c| c.launcher.web_search = u.clone()))
        })
        .collect();
    if sel.is_none() {
        let v = c.web_search.clone();
        items.push((t!("Özel", "Custom"), edit(move |c| c.launcher.web_search = v.clone())));
    }
    let each = if items.len() > 4 { 88.0 } else { 104.0 };
    f.choice(p, t!("Web araması", "Web search"), t!("Sonuç bulunamayınca bu sitede aranır.", "Searches this site when nothing else matches."), &items, sel.unwrap_or(items.len() - 1), each);
    engines_row(app, p, f, sel.filter(|&i| i >= ENGINES.len()).map(|i| all[i].0.clone()));
    super::extension::paint(p, f);

    f.section(p, t!("Kısayollar", "Shortcuts"));
    f.note(p, t!("yt · chatgpt · claude · grok · gemini · wp — yalnız yazınca açar, ardından metin yazınca arar/sorar. @ hepsini listeler.", "yt · chatgpt · claude · grok · gemini · wp — on their own they open the site; followed by text they search or ask. @ lists them all."));
    f.note(p, t!("wp WhatsApp uygulamasında (kurulu değilse Web'de), wwp her zaman WhatsApp Web'de açar.", "wp opens in the WhatsApp app (or on the web if it's not installed); wwp always opens WhatsApp Web."));
    f.note(p, t!("/lock · /restart · /shutdown — kilitle, yeniden başlat, kapat (son ikisi iki kez Enter ister).", "/lock · /restart · /shutdown — lock, restart, shut down (the last two need Enter twice)."));
    f.note(p, t!(":gül · :kalp — emoji kopyalar. > — açık pencereleri listeler. son — son açılan dosyalar.", ":smile · :heart — copies an emoji. > — lists open windows. recent — recently opened files."));
    f.note(p, t!("wifi kapat · bluetooth aç · ses 30 · sessiz · parlaklık 70 · karanlık mod — hızlı ayarlar.", "wifi off · bluetooth on · volume 30 · mute · brightness 70 · dark mode — quick settings."));
    super::contacts::paint(&app.cfg, p, f);
}
