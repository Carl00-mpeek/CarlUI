//! Bu modül "wp ad mesaj" ile WhatsApp sohbetini mesaj hazır açma sorununu
//! şu yöntemle çözüyor: ayarlardan içe aktarılan rehberde (ad → numara) ad
//! aranır; bulunan kişinin sohbeti `send?phone=…&text=…` bağlantısıyla açılır.
//! WhatsApp masaüstü uygulaması kuruluysa (`whatsapp:` protokolü) o, değilse
//! WhatsApp Web kullanılır. Kişiler kapalıysa ya da hiç kişi yoksa yalnızca
//! WhatsApp açılır. Mesaj hiçbir zaman kendiliğinden gönderilmez.

use crate::search::{enc, norm, Act, Ctx, Icon, Item};
use std::sync::OnceLock;

const GLYPH: char = '\u{E8BD}';
const MAX_SUGGEST: usize = 5;

/// WhatsApp masaüstü uygulaması `whatsapp:` protokolünü kaydetmiş mi?
fn app_installed() -> bool {
    static APP: OnceLock<bool> = OnceLock::new();
    *APP.get_or_init(|| unsafe {
        use windows::Win32::System::Registry::*;
        let mut key = HKEY::default();
        let ok = RegOpenKeyExW(HKEY_CLASSES_ROOT, windows::core::w!("whatsapp"), 0, KEY_READ, &mut key).is_ok();
        if ok {
            let _ = RegCloseKey(key);
        }
        ok
    })
}

/// Yüzde kodlama (boşluk %20): WhatsApp metni "+" işaretini olduğu gibi gösterir.
fn enc_pct(s: &str) -> String {
    enc(s).replace('+', "%20")
}

struct Links {
    app: bool,
}

impl Links {
    fn home(&self) -> String {
        if self.app { "whatsapp://".into() } else { "https://web.whatsapp.com".into() }
    }

    fn send(&self, phone: &str, msg: &str) -> String {
        let base = if self.app { "whatsapp://send" } else { "https://web.whatsapp.com/send" };
        let text = enc_pct(msg);
        match (phone.is_empty(), msg.is_empty()) {
            (true, _) => format!("{base}?text={text}"),
            (false, true) => format!("{base}?phone={phone}"),
            (false, false) => format!("{base}?phone={phone}&text={text}"),
        }
    }

    fn where_(&self) -> &'static str {
        if self.app { t!("WhatsApp uygulamasında", "In the WhatsApp app") } else { t!("WhatsApp Web'de", "In WhatsApp Web") }
    }
}

fn open_item(l: &Links) -> Item {
    let sub = if l.app { t!("Masaüstü uygulamasını aç", "Open the desktop app") } else { t!("Varsayılan tarayıcıda aç", "Open in default browser") };
    Item::new(if l.app { "WhatsApp" } else { "WhatsApp Web" }, sub, Icon::Glyph(GLYPH), Act::Open(l.home()), "Kısayol")
}

fn chat_item(l: &Links, name: &str, phone: &str, msg: &str) -> Item {
    let title = if msg.is_empty() { format!("WhatsApp: {name}") } else { format!("WhatsApp → {name}: {msg}") };
    let sub = t!(format!("{} mesaj hazır açılır; göndermek için orada Enter", l.where_()), format!("{} the message opens ready; press Enter there to send", l.where_()));
    Item::new(title, sub, Icon::Glyph(GLYPH), Act::Open(l.send(phone, msg)), "Kısayol")
}

/// "wp"/"wwp" sonrası metin için sonuçlar. `prefix` Tab tamamlamasının başı ("wp ");
/// `web`: uygulama kurulu olsa da WhatsApp Web kullanılır (wwp).
pub(crate) fn items(prefix: &str, rest: &str, web: bool, cx: &Ctx) -> Vec<Item> {
    build(&Links { app: !web && app_installed() }, prefix, rest, cx)
}

fn build(l: &Links, prefix: &str, rest: &str, cx: &Ctx) -> Vec<Item> {
    let contacts = &cx.cfg.whatsapp;
    if rest.is_empty() || !cx.cfg.whatsapp_contacts || contacts.is_empty() {
        return vec![open_item(l)];
    }
    let words: Vec<&str> = rest.split_whitespace().collect();
    let msg_after = |k: usize| words[k..].join(" ");
    let first = words[0].trim_start_matches('@');

    // Doğrudan numara: "wp +905551234567 selam".
    let digits: String = first.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 10 && first.chars().all(|c| c.is_ascii_digit() || c == '+') {
        return vec![chat_item(l, first, &digits, &msg_after(1))];
    }

    // Tam ad (en uzun eşleşme önce): "wp demhat yılmaz selam".
    let normed: Vec<(String, &String, &String)> = contacts.iter().map(|(n, p)| (norm(n), n, p)).collect();
    for k in (1..=words.len().min(4)).rev() {
        let cand = norm(&words[..k].join(" ").trim_start_matches('@').to_string());
        if let Some((_, n, p)) = normed.iter().find(|(nn, ..)| *nn == cand) {
            let mut it = chat_item(l, n, p, &msg_after(k));
            it.complete = format!("{prefix}{n} ");
            return vec![it];
        }
    }

    // Kısmi ad: adın herhangi bir kelimesi yazılanla başlıyorsa öner (Tab ile tamamlanır).
    let who = norm(first);
    let msg = msg_after(1);
    let mut hits: Vec<(u8, &String, &String)> = normed
        .iter()
        .filter_map(|(nn, n, p)| {
            let rank = if nn.starts_with(&who) {
                0
            } else if nn.split(' ').any(|w| w.starts_with(&who)) {
                1
            } else {
                return None;
            };
            Some((rank, *n, *p))
        })
        .collect();
    hits.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(b.1)));
    if !hits.is_empty() {
        return hits
            .into_iter()
            .take(MAX_SUGGEST)
            .map(|(_, n, p)| {
                let mut it = chat_item(l, n, p, &msg);
                it.complete = format!("{prefix}{n} ");
                it
            })
            .collect();
    }

    vec![Item::new(
        t!(format!("WhatsApp: “{first}” rehberde yok"), format!("WhatsApp: “{first}” is not in contacts")),
        t!(format!("{} kişi seçerek gönder", l.where_()), format!("{}, pick a contact to send", l.where_())),
        Icon::Glyph(GLYPH),
        Act::Open(l.send("", &msg)),
        "Kısayol",
    )]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{currency::Rates, files::Files};
    use carlui_core::config::LauncherConfig;
    use std::{collections::HashMap, sync::Arc};

    fn run(on: bool, rest: &str) -> Vec<Item> {
        let mut cfg = LauncherConfig::default();
        cfg.whatsapp_contacts = on;
        cfg.whatsapp.insert("Demhat Yılmaz".into(), "905551234567".into());
        cfg.whatsapp.insert("Ali Kaya".into(), "905320000001".into());
        cfg.whatsapp.insert("Ali Veli".into(), "905320000002".into());
        let (files, rates, usage) = (Files::default(), Rates::default(), HashMap::new());
        let cx = Ctx { apps: &[], run: &[], usage: &usage, files: &files, rates: &rates, cfg: &cfg, wake: Arc::new(|| {}) };
        build(&Links { app: false }, "wp ", rest, &cx)
    }

    fn url(i: &Item) -> &str {
        match &i.act {
            Act::Open(u) => u,
            _ => "",
        }
    }

    #[test]
    fn off_only_opens() {
        let v = run(false, "demhat selam");
        assert_eq!(v.len(), 1);
        assert_eq!(url(&v[0]), "https://web.whatsapp.com");
        assert_eq!(url(&run(true, "")[0]), "https://web.whatsapp.com");
    }

    #[test]
    fn contacts() {
        assert_eq!(url(&run(true, "demhat selam nasılsın")[0]), "https://web.whatsapp.com/send?phone=905551234567&text=selam%20nas%C4%B1ls%C4%B1n");
        assert_eq!(url(&run(true, "@demhat")[0]), "https://web.whatsapp.com/send?phone=905551234567");
        assert_eq!(url(&run(true, "demhat yılmaz naber")[0]), "https://web.whatsapp.com/send?phone=905551234567&text=naber");
        let v = run(true, "ali selam");
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].complete, "wp Ali Kaya ");
        assert_eq!(url(&run(true, "ali veli selam")[0]), "https://web.whatsapp.com/send?phone=905320000002&text=selam");
        assert_eq!(url(&run(true, "yilmaz")[0]), "https://web.whatsapp.com/send?phone=905551234567");
        assert!(run(true, "zeynep selam")[0].title.contains("rehberde yok"));
        assert_eq!(url(&run(true, "+905551112233 hey")[0]), "https://web.whatsapp.com/send?phone=905551112233&text=hey");
    }

    #[test]
    fn app_links() {
        let l = Links { app: true };
        assert_eq!(l.send("90555", "a b"), "whatsapp://send?phone=90555&text=a%20b");
        assert_eq!(l.home(), "whatsapp://");
    }
}
