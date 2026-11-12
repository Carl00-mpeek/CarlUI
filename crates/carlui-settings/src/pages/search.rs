//! Arama: adadaki arama, dosya araması, döviz çevirisi, web arama motoru.

use crate::{
    input::Form,
    ui::{edit, Flow, P},
    Act, App,
};
use carlui_ui::paint::R;

/// "+ Arama motoru ekle" satırı ve açılınca aşağı doğru büyüyen form.
fn engines_row(app: &App, p: &mut P, f: &mut Flow, custom: Option<String>) {
    // Kayıtlı motor sayısı kartta görünür; nasıl ekleneceği "i" düğmesinde.
    let r = if app.cfg.launcher.engines.is_empty() {
        f.row(p, t!("Arama motoru ekle", "Add search engine"), t!("Listede olmayan bir site ekleyin: ad ve {q} içeren arama adresi.", "Add a site that isn't listed: a name and a search address containing {q}."), 230.0)
    } else {
        let n = app.cfg.launcher.engines.len();
        let sub = t!(format!("{n} özel motor kayıtlı. Seçili özel motor silinebilir."), format!("{n} custom engines saved. The selected custom engine can be deleted."));
        f.row_sub(p, t!("Arama motoru ekle", "Add search engine"), "", &sub, None, 230.0)
    };
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

    super::form::draw(app, p, f, Form::Engine, &[(t!("Ad", "Name"), t!("ör. Wikipedia", "e.g. Wikipedia")), (t!("Arama adresi", "Search address"), t!("ör. https://tr.wikipedia.org/w/index.php?search={q}", "e.g. https://en.wikipedia.org/w/index.php?search={q}"))]);
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

    // Arama kısaltmalarının listesi bölüm başlığının "i" düğmesinde (paragraf başına bir satır).
    f.section_info(
        p,
        t!("Kısayollar", "Shortcuts"),
        t!(
            "yt · chatgpt · claude · grok · gemini · wp — yalnız yazınca açar, ardından metin yazınca arar/sorar. @ hepsini listeler.\nwp WhatsApp uygulamasında (kurulu değilse Web'de), wwp her zaman WhatsApp Web'de açar.\n/lock · /restart · /shutdown — kilitle, yeniden başlat, kapat (son ikisi iki kez Enter ister).\n:gül · :kalp — emoji kopyalar. > — açık pencereleri listeler. son — son açılan dosyalar.\nwifi kapat · bluetooth aç · ses 30 · sessiz · parlaklık 70 · karanlık mod — hızlı ayarlar.",
            "yt · chatgpt · claude · grok · gemini · wp — on their own they open the site; followed by text they search or ask. @ lists them all.\nwp opens in the WhatsApp app (or on the web if it's not installed); wwp always opens WhatsApp Web.\n/lock · /restart · /shutdown — lock, restart, shut down (the last two need Enter twice).\n:smile · :heart — copies an emoji. > — lists open windows. recent — recently opened files.\nwifi off · bluetooth on · volume 30 · mute · brightness 70 · dark mode — quick settings."
        ),
    );
    super::contacts::paint(&app.cfg, p, f);
}
