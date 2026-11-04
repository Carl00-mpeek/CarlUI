//! Bu modül "yazılan metne göre ne gösterilir, hangi sırayla?" sorununu çözüyor.
//!
//! Eşleştirme Türkçe karakterlere duyarsızdır ("ayarlar" = "AYARLAR",
//! "gorev" = "Görev", "i" = "ı"). Puan sırası: tam eşleşme → baştan
//! eşleşme → kelime başı → baş harfler ("vsc" → Visual Studio Code) → içinde
//! geçme → harf sırası. Sık açılanlar küçük bir bonus alır; böylece "c" yazınca
//! en çok kullanılan "C" ile başlayan uygulama üste çıkar.
//!
//! Özel sonuçlar her zaman en üsttedir: hesap, birim/döviz çevirisi, adres,
//! "/" komutları ve yapay zekâ kısayolları (chatgpt/claude/gemini/codex …).

use crate::{
    calc,
    currency::{Rate, Rates},
    files::Files,
    index::{self, App, Running},
};
use carlui_core::config::LauncherConfig;
use std::{collections::HashMap, path::Path, sync::Arc};

#[derive(Clone, Debug, PartialEq)]
pub enum Icon {
    /// Shell'den ikon: dosya yolu ya da `shell:AppsFolder\…`.
    Shell(String),
    /// Segoe Fluent Icons karakteri, vurgu renkli kare üstünde.
    Glyph(char),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sys {
    Lock,
    Sleep,
    SignOut,
    Restart,
    Shutdown,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// Açık penceresi varsa öne getir, yoksa başlat.
    App { launch: String, hwnd: Option<isize> },
    Open(String),
    Admin(String),
    Reveal(String),
    Copy(String),
    Close(isize),
    Restart { pid: u32, hwnd: isize, launch: String },
    System(Sys),
    /// Yapay zekâ: adres varsa açılır; yoksa uygulama açılıp istem pencereye yazılır.
    Ai { url: Option<String>, launch: Option<String>, window_hint: &'static str, prompt: String },
    AllApps,
    /// Hızlı ayar: Wi-Fi, Bluetooth, ses, parlaklık, karanlık mod.
    Quick(crate::quickset::Quick),
    /// Adada geri sayım / kronometre / durdur (adanın kendisi uygular).
    Timer { secs: u32, label: String },
    Stopwatch,
    TimerStop,
    /// Adanın indirme yöneticisiyle İndirilenler'e indir / süreni iptal et.
    Download(String),
    DownloadCancel,
    /// Döviz kuru gibi henüz hazır olmayan sonuç.
    Pending,
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub title: String,
    pub sub: String,
    pub icon: Icon,
    pub act: Act,
    /// Sağda soluk etiket ("Uygulama", "Ayar" …).
    pub tag: &'static str,
    /// Kullanım sayacı anahtarı (boşsa sayılmaz).
    pub key: String,
    /// Dosya sistemi yolu (konumu aç / yolu kopyala / yönetici için).
    pub path: String,
    /// Açık uygulama: (pencere, süreç).
    pub run: Option<(isize, u32)>,
    /// Tab ile kutuya yazılacak metin; boşsa (uygulama, ayar, dosya…) başlık.
    pub complete: String,
}

impl Item {
    pub(crate) fn new(title: impl Into<String>, sub: impl Into<String>, icon: Icon, act: Act, tag: &'static str) -> Self {
        Self { title: title.into(), sub: sub.into(), icon, act, tag, key: String::new(), path: String::new(), run: None, complete: String::new() }
    }

    /// Ekranda gösterilen etiket (iç anahtar Türkçe kalır, karşılaştırmalar bozulmasın).
    pub fn tag_label(&self) -> &'static str {
        tag_label(self.tag)
    }

    /// Tab tamamlaması: kısayol/komut metni ya da adı olan sonuçların başlığı.
    pub fn completion(&self) -> Option<String> {
        if !self.complete.is_empty() {
            return Some(self.complete.clone());
        }
        matches!(self.tag, "Uygulama" | "Ayar" | "Konum" | "Dosya" | "Klasör").then(|| self.title.clone())
    }
}

/// Etiketin seçili dildeki adı.
pub fn tag_label(tag: &'static str) -> &'static str {
    if carlui_core::lang::is_tr() {
        return tag;
    }
    match tag {
        "Uygulama" => "App",
        "Ayar" => "Setting",
        "Konum" => "Location",
        "Dosya" => "File",
        "Klasör" => "Folder",
        "Kısayol" => "Shortcut",
        "Komut" => "Command",
        "YZ" => "AI",
        "Hesap" => "Math",
        "Çeviri" => "Convert",
        "Döviz" => "Currency",
        "Pencere" => "Window",
        "İndir" => "Download",
        "Son" => "Recent",
        "Pano" => "Clipboard",
        "Zaman" => "Timer",
        t => t,
    }
}

/// Küçük harf, Türkçe karakterler sadeleştirilmiş.
pub fn norm(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let c = match c {
            'İ' | 'I' | 'ı' => 'i',
            'Ş' | 'ş' => 's',
            'Ğ' | 'ğ' => 'g',
            'Ü' | 'ü' => 'u',
            'Ö' | 'ö' => 'o',
            'Ç' | 'ç' => 'c',
            'Â' | 'â' => 'a',
            'É' | 'é' => 'e',
            c => c,
        };
        out.extend(c.to_lowercase());
    }
    out
}

fn words(t: &str) -> impl Iterator<Item = &str> {
    t.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty())
}

/// `t` ve `q` normalleştirilmiş olmalı. 0 = eşleşmiyor.
pub fn score(t: &str, q: &str) -> i32 {
    let q = q.trim();
    if q.is_empty() {
        return 0;
    }
    let pen = (t.chars().count() as i32 - q.chars().count() as i32).clamp(0, 60);
    if t == q {
        return 1000;
    }
    if t.starts_with(q) {
        return 900 - pen;
    }
    if words(t).any(|w| w.starts_with(q)) {
        return 760 - pen;
    }
    let qw: Vec<&str> = q.split(' ').filter(|w| !w.is_empty()).collect();
    if qw.len() > 1 && qw.iter().all(|w| words(t).any(|tw| tw.starts_with(w))) {
        return 720 - pen;
    }
    let compact: String = q.chars().filter(|c| !c.is_whitespace()).collect();
    let initials: String = words(t).filter_map(|w| w.chars().next()).collect();
    if compact.chars().count() >= 2 && initials.starts_with(&compact) {
        return 660 - pen;
    }
    if t.contains(q) {
        return 500 - pen;
    }
    // Harf sırası: "ntpd" → notepad. Aradaki boşluklar cezalandırılır.
    if compact.chars().count() >= 3 {
        let mut it = t.chars().enumerate();
        let mut gaps = 0;
        let mut last = None;
        for qc in compact.chars() {
            match it.by_ref().find(|(_, c)| *c == qc) {
                Some((i, _)) => {
                    if let Some(l) = last {
                        gaps += i - l - 1;
                    }
                    last = Some(i);
                }
                None => return 0,
            }
        }
        return (320 - gaps as i32 * 25).max(0);
    }
    0
}

fn boost(usage: &HashMap<String, u32>, key: &str) -> i32 {
    usage.get(key).map(|&n| ((n as f32).ln_1p() * 90.0) as i32).unwrap_or(0).min(320)
}

pub(crate) fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(*b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub struct SysCmd {
    pub sys: Sys,
    pub name: &'static str,
    pub en: &'static str,
    pub keywords: &'static str,
    pub glyph: char,
}

pub const SYSTEM: &[SysCmd] = &[
    SysCmd { sys: Sys::Lock, name: "Kilitle", en: "Lock", keywords: "/lock /kilitle lock kilit ekranı kilitle", glyph: '\u{E72E}' },
    SysCmd { sys: Sys::Sleep, name: "Uyku", en: "Sleep", keywords: "/uyku sleep uyut", glyph: '\u{E708}' },
    SysCmd { sys: Sys::SignOut, name: "Oturumu kapat", en: "Sign out", keywords: "/oturum sign out log off çıkış yap", glyph: '\u{E748}' },
    SysCmd { sys: Sys::Restart, name: "Yeniden başlat", en: "Restart", keywords: "/restart /yeniden restart reboot", glyph: '\u{E777}' },
    SysCmd { sys: Sys::Shutdown, name: "Bilgisayarı kapat", en: "Shut down", keywords: "/shutdown /kapat shutdown power off", glyph: '\u{E7E8}' },
];

pub fn sys_item(c: &SysCmd) -> Item {
    let sub = match c.sys {
        Sys::Restart | Sys::Shutdown => t!("Onay için iki kez Enter", "Press Enter twice to confirm"),
        _ => t!("Sistem komutu", "System command"),
    };
    let mut it = Item::new(t!(c.name, c.en), sub, Icon::Glyph(c.glyph), Act::System(c.sys), "Komut");
    it.key = format!("sys:{:?}", c.sys);
    it.complete = c.keywords.split(' ').next().unwrap_or("").to_string();
    it
}

pub fn app_item(a: &App, run: &[Running]) -> Item {
    let w = index::window_of(a, run);
    let sub = match (&w, a.target.is_empty()) {
        (Some(_), _) => t!("Açık · Enter ile öne getir", "Open · Enter to bring to front").to_string(),
        (None, false) => a.target.clone(),
        (None, true) => t!("Uygulama", "App").to_string(),
    };
    let mut it = Item::new(&a.name, sub, Icon::Shell(a.launch_path()), Act::App { launch: a.launch_path(), hwnd: w.as_ref().map(|w| w.hwnd) }, "Uygulama");
    it.key = format!("app:{}", a.parsing.to_lowercase());
    it.path = if a.target.contains(":\\") { a.target.clone() } else { String::new() };
    it.run = w.map(|w| (w.hwnd, w.pid));
    it
}

fn file_item(p: &Path, dir: bool) -> Item {
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let parent = p.parent().map(|d| d.display().to_string()).unwrap_or_default();
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    let parent = if !home.is_empty() && parent.starts_with(&home) { format!("~{}", &parent[home.len()..]) } else { parent };
    let s = p.display().to_string();
    let mut it = Item::new(name, parent, Icon::Shell(s.clone()), Act::Open(s.clone()), if dir { "Klasör" } else { "Dosya" });
    it.key = format!("file:{}", s.to_lowercase());
    it.path = s;
    it
}

/// Hızlı erişim ekranı (boş arama): sık kullanılanlar + kısayollar.
pub fn home(apps: &[App], run: &[Running], usage: &HashMap<String, u32>) -> (Vec<Item>, Vec<Item>) {
    let mut ranked: Vec<(u32, &App)> = apps.iter().filter_map(|a| usage.get(&format!("app:{}", a.parsing.to_lowercase())).map(|&n| (n, a))).collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.cmp(&b.1.name)));
    let mut tiles: Vec<Item> = ranked.iter().take(6).map(|(_, a)| app_item(a, run)).collect();
    // İlk kullanımda boş kalmasın: bilinen birkaç uygulama.
    if tiles.len() < 6 {
        for want in ["Dosya Gezgini", "File Explorer", "Microsoft Edge", "Google Chrome", "Firefox", "Not Defteri", "Notepad", "Hesap Makinesi", "Calculator", "Ayarlar", "Settings"] {
            if tiles.len() >= 6 {
                break;
            }
            if let Some(a) = apps.iter().find(|a| a.name == want) {
                if !tiles.iter().any(|t| t.title == a.name) {
                    tiles.push(app_item(a, run));
                }
            }
        }
    }
    let mut chips = vec![
        Item::new(t!("Tüm uygulamalar", "All apps"), "", Icon::Glyph('\u{E71D}'), Act::AllApps, ""),
        Item::new(t!("Ayarlar", "Settings"), "", Icon::Glyph('\u{E713}'), Act::Open("ms-settings:".into()), ""),
        Item::new(t!("Dosya Gezgini", "File Explorer"), "", Icon::Glyph('\u{EC50}'), Act::Open("explorer.exe".into()), ""),
    ];
    for c in SYSTEM.iter().filter(|c| matches!(c.sys, Sys::Lock | Sys::Restart | Sys::Shutdown)) {
        let mut it = sys_item(c);
        it.title = t!(c.name, c.en).into();
        chips.push(it);
    }
    (tiles, chips)
}

pub fn all_apps(apps: &[App], run: &[Running]) -> Vec<Item> {
    apps.iter().map(|a| app_item(a, run)).collect()
}

pub struct Ctx<'a> {
    pub apps: &'a [App],
    pub run: &'a [Running],
    pub usage: &'a HashMap<String, u32>,
    pub files: &'a Files,
    pub rates: &'a Rates,
    pub cfg: &'a LauncherConfig,
    /// Arka plan sonucu (kur) hazır olunca aramayı yenilet.
    pub wake: Arc<dyn Fn() + Send + Sync>,
}

/// Kısayol kelimeleri: "yt", "yt kedi", "@chatgpt soru", "wp demhat selam".
/// Baştaki "@" isteğe bağlıdır; yalnızca "@" yazınca hepsi listelenir.
#[derive(Clone, Copy, PartialEq)]
enum Kw {
    YouTube,
    /// Adres şablonu (`{q}` = istem); boşsa uygulama/sayfa açılıp istem yazılır.
    Ai(&'static str, &'static str),
    /// `true`: her zaman WhatsApp Web (wwp); değilse kuruluysa uygulama (wp).
    WhatsApp(bool),
}

/// (kelimeler, ad, tür). İlk kelime Tab tamamlamasında kullanılır.
const KEYWORDS: &[(&[&str], &str, Kw)] = &[
    (&["yt", "youtube"], "YouTube", Kw::YouTube),
    (&["chatgpt", "gpt"], "ChatGPT", Kw::Ai("ChatGPT", "https://chatgpt.com/?q={q}")),
    (&["claude"], "Claude", Kw::Ai("Claude", "https://claude.ai/new?q={q}")),
    (&["grok"], "Grok", Kw::Ai("Grok", "https://grok.com/?q={q}")),
    (&["perplexity"], "Perplexity", Kw::Ai("Perplexity", "https://www.perplexity.ai/search?q={q}")),
    (&["gemini"], "Gemini", Kw::Ai("Gemini", "")),
    (&["codex"], "Codex", Kw::Ai("Codex", "")),
    (&["wp", "whatsapp"], "WhatsApp", Kw::WhatsApp(false)),
    (&["wwp"], "WhatsApp Web", Kw::WhatsApp(true)),
];

fn kw_item(kw: Kw, name: &str, rest: &str, cx: &Ctx) -> Item {
    let g = |c: char| Icon::Glyph(c);
    match kw {
        Kw::YouTube if rest.is_empty() => Item::new("YouTube", t!("Varsayılan tarayıcıda aç", "Open in default browser"), g('\u{E714}'), Act::Open("https://www.youtube.com".into()), "Kısayol"),
        Kw::YouTube => Item::new(t!(format!("YouTube'da ara: {rest}"), format!("Search YouTube: {rest}")), t!("Varsayılan tarayıcıda", "In default browser"), g('\u{E714}'), Act::Open(format!("https://www.youtube.com/results?search_query={}", enc(rest))), "Kısayol"),
        Kw::Ai(hint, url) => {
            let prompt = rest.to_string();
            let act = if !url.is_empty() {
                let u = if prompt.is_empty() { url.split('?').next().unwrap_or(url).to_string() } else { url.replace("{q}", &enc(&prompt)) };
                Act::Ai { url: Some(u), launch: None, window_hint: hint, prompt: String::new() }
            } else if hint == "Codex" {
                match cx.apps.iter().find(|a| norm(&a.name).contains("codex")) {
                    Some(a) => Act::Ai { url: None, launch: Some(a.launch_path()), window_hint: "Codex", prompt },
                    None => Act::Ai { url: Some("https://chatgpt.com/codex".into()), launch: None, window_hint: "Codex", prompt },
                }
            } else {
                Act::Ai { url: Some("https://gemini.google.com/app".into()), launch: None, window_hint: "Gemini", prompt }
            };
            let title = if rest.is_empty() { name.to_string() } else { format!("{name}: {rest}") };
            Item::new(title, if rest.is_empty() { t!("Yapay zekâ · aç", "AI · open") } else { t!("Yapay zekâya sor", "Ask AI") }, g('\u{E99A}'), act, "YZ")
        }
        Kw::WhatsApp(web) => crate::whatsapp::items("", "", web, cx).remove(0),
    }
}

/// Kısayol kelimesiyle başlayan arama. `@` ile başlayıp kelime tamamlanmamışsa
/// eşleşen kısayollar listelenir (Tab ile tamamlanır).
fn keyword_items(raw: &str, cx: &Ctx) -> Vec<Item> {
    let t = raw.trim_start();
    let at = t.starts_with('@');
    let body = t.trim_start_matches('@');
    let (word, rest) = match body.find(' ') {
        Some(i) => (&body[..i], body[i + 1..].trim()),
        None => (body, ""),
    };
    let w = norm(word);
    if let Some((words, name, kw)) = KEYWORDS.iter().find(|(ws, ..)| ws.contains(&w.as_str())) {
        let prefix = format!("{}{} ", if at { "@" } else { "" }, words[0]);
        if let Kw::WhatsApp(web) = *kw {
            // Kişi önerileri kendi tamamlamasını taşır ("wp Ali Kaya ").
            return crate::whatsapp::items(&prefix, rest, web, cx)
                .into_iter()
                .map(|mut it| {
                    if it.complete.is_empty() {
                        it.complete = prefix.clone();
                    }
                    it.key = format!("kw:{}", words[0]);
                    it
                })
                .collect();
        }
        let mut it = kw_item(*kw, name, rest, cx);
        it.complete = format!("{}{} ", if at { "@" } else { "" }, words[0]);
        it.key = format!("kw:{}", words[0]);
        return vec![it];
    }
    if !at || body.contains(' ') {
        return Vec::new();
    }
    KEYWORDS
        .iter()
        .filter(|(ws, ..)| ws.iter().any(|k| k.starts_with(&w)))
        .map(|(ws, name, kw)| {
            let mut it = kw_item(*kw, name, "", cx);
            it.sub = format!("@{} · {}", ws[0], it.sub);
            it.complete = format!("@{} ", ws[0]);
            it.key = format!("kw:{}", ws[0]);
            it
        })
        .collect()
}

fn looks_like_url(q: &str) -> Option<String> {
    let q = q.trim();
    if q.contains(' ') || q.len() < 4 {
        return None;
    }
    if q.starts_with("http://") || q.starts_with("https://") {
        return Some(q.to_string());
    }
    let host = q.split('/').next().unwrap_or("");
    let tld = host.rsplit('.').next().unwrap_or("");
    (host.contains('.') && tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()) && !host.starts_with('.')).then(|| format!("https://{q}"))
}

pub fn build(raw: &str, cx: &Ctx) -> Vec<Item> {
    let q = norm(raw.trim());
    let mut top: Vec<Item> = Vec::new();
    if q.is_empty() {
        return top;
    }

    // Hesap / birim / döviz
    if let Some(v) = calc::eval(raw) {
        top.push(Item::new(format!("= {}", calc::fmt(v)), format!("{} · {}", raw.trim(), t!("Enter ile kopyala", "Enter to copy")), Icon::Glyph('\u{E8EF}'), Act::Copy(calc::plain(v)), "Hesap"));
    } else if let Some((v, text)) = calc::convert_units(raw) {
        top.push(Item::new(text, t!("Birim çevirisi · Enter ile kopyala", "Unit conversion · Enter to copy"), Icon::Glyph('\u{E8AB}'), Act::Copy(calc::plain(v)), "Çeviri"));
    } else if let Some((amount, from, to)) = cx.currency_enabled().then(|| calc::currency_request(raw)).flatten() {
        let wake = cx.wake.clone();
        match cx.rates.get(from, to, move || wake()) {
            Rate::Ready { rate, date } => {
                let r = (amount * rate * 100.0).round() / 100.0;
                top.push(Item::new(
                    format!("{} {from} = {} {to}", calc::fmt(amount), calc::fmt(r)),
                    format!("1 {from} = {} {to} · {date} · {}", calc::fmt((rate * 10000.0).round() / 10000.0), t!("Enter ile kopyala", "Enter to copy")),
                    Icon::Glyph('\u{E8C7}'),
                    Act::Copy(calc::plain(r)),
                    "Döviz",
                ));
            }
            Rate::Loading => top.push(Item::new(format!("{} {from} → {to}", calc::fmt(amount)), t!("Kur alınıyor…", "Fetching rate…"), Icon::Glyph('\u{E8C7}'), Act::Pending, "Döviz")),
            Rate::Failed => top.push(Item::new(format!("{} {from} → {to}", calc::fmt(amount)), t!("Kur alınamadı, bağlantıyı kontrol et", "Couldn't get the rate, check your connection"), Icon::Glyph('\u{E8C7}'), Act::Pending, "Döviz")),
        }
    }

    // ">" açık pencereler (Alt+Tab gibi)
    if let Some(rest) = q.strip_prefix('>') {
        top.extend(crate::openwin::matches(cx.run, rest.trim(), true).into_iter().map(|(_, it)| it));
        return top;
    }

    // Çeviri ("tr: hello", "çevir merhaba") ve pano geçmişi ("pano adres")
    if let Some(it) = crate::translate::items(raw, cx.wake.clone()) {
        top.push(it);
        return top;
    }
    if let Some(list) = crate::clipsearch::items(&q) {
        top.extend(list);
        return top;
    }

    // İndirme yöneticisi ("indir <adres>", "indirmeyi iptal et")
    if cx.cfg.download_manager {
        if let Some(list) = crate::download::command(raw) {
            top.extend(list);
            return top;
        }
    }

    // Zamanlayıcı ("5dk", "25dk çalış", "kronometre")
    top.extend(crate::timer::items(raw));

    // ":" emoji (":gül", ":kalp")
    if let Some(rest) = raw.trim_start().strip_prefix(':') {
        top.extend(crate::emoji::matches(rest));
        return top;
    }

    // "son" son açılan dosyalar
    if let Some(list) = crate::recent::listing(&q) {
        top.extend(list);
    }

    // "/" komutları
    if q.starts_with('/') {
        for c in SYSTEM {
            if q == "/" || c.keywords.split(' ').any(|k| k.starts_with(&q)) {
                top.push(sys_item(c));
            }
        }
        return top;
    }

    // Kısayol kelimeleri (yt, chatgpt, wp …); "@" ile başlıyorsa yalnızca onlar.
    let kws = keyword_items(raw, cx);
    if q.starts_with('@') {
        top.extend(kws);
        return top;
    }
    top.extend(kws);

    // Yol ya da adres
    let trimmed = raw.trim();
    if trimmed.len() >= 3 && (trimmed.chars().nth(1) == Some(':') || trimmed.starts_with("\\\\") || trimmed.starts_with('%')) {
        let p = carlui_core::config::expand_env(trimmed);
        if Path::new(&p).exists() {
            let mut it = file_item(Path::new(&p), Path::new(&p).is_dir());
            it.tag = "Konum";
            top.push(it);
        }
    } else if let Some(url) = looks_like_url(trimmed) {
        let dl = cx.cfg.download_manager.then(|| crate::download::suggest(&url)).flatten();
        top.push(Item::new(t!(format!("{trimmed} adresine git"), format!("Go to {trimmed}")), url.clone(), Icon::Glyph('\u{E774}'), Act::Open(url), "Web"));
        top.extend(dl);
    }

    // Uygulamalar, ayarlar, konumlar, komutlar
    let mut ranked: Vec<(i32, Item)> = Vec::new();
    for a in cx.apps {
        let s = score(&norm(&a.name), &q);
        if s >= 250 {
            let it = app_item(a, cx.run);
            ranked.push((s + 15 + boost(cx.usage, &it.key), it));
        }
    }
    for st in index::SETTINGS {
        let s = score(&norm(st.name), &q).max(score(&norm(st.en), &q)).max(norm(st.keywords).split(' ').map(|k| score(k, &q)).max().unwrap_or(0).min(700) - 60);
        if s >= 400 {
            let mut it = Item::new(t!(st.name, st.en), t!("Windows ayarı", "Windows setting"), Icon::Glyph(st.glyph), Act::Open(st.uri.into()), "Ayar");
            it.key = format!("set:{}", st.uri);
            ranked.push((s + boost(cx.usage, &it.key), it));
        }
    }
    // Carl Vault (yanındaki carlui-vault.exe).
    let vs = ["carl vault", "vault", "kasa", "parola", "sifre", "password"].iter().map(|k| score(k, &q)).max().unwrap_or(0);
    if vs >= 400 {
        let exe = carlui_core::paths::sibling_exe("carlui-vault.exe").to_string_lossy().into_owned();
        let mut it = Item::new("Carl Vault".to_string(), t!("Şifreli parola kasası", "Encrypted password vault"), Icon::Glyph('\u{E72E}'), Act::Open(exe), "Uygulama");
        it.key = "carlui:vault".into();
        ranked.push((vs + boost(cx.usage, &it.key), it));
    }
    for (name, path, kw) in index::places() {
        let s = score(&norm(&name), &q).max(norm(kw).split(' ').map(|k| score(k, &q)).max().unwrap_or(0).min(700) - 60);
        if s >= 400 {
            let icon = if path.contains(":\\") { Icon::Shell(path.clone()) } else { Icon::Glyph('\u{EC50}') };
            let mut it = Item::new(name, path.clone(), icon, Act::Open(path.clone()), "Konum");
            it.key = format!("place:{}", path.to_lowercase());
            if path.contains(":\\") {
                it.path = path;
            }
            ranked.push((s + boost(cx.usage, &it.key), it));
        }
    }
    for c in SYSTEM {
        let s = score(&norm(c.name), &q).max(score(&norm(c.en), &q)).max(norm(c.keywords).split(' ').map(|k| score(k, &q)).max().unwrap_or(0).min(700) - 60);
        if s >= 500 {
            let it = sys_item(c);
            ranked.push((s - 40 + boost(cx.usage, &it.key), it));
        }
    }
    // Açık pencereler uygulamanın hemen altında.
    ranked.extend(crate::openwin::matches(cx.run, &q, false).into_iter().map(|(s, it)| (s - 10, it)));
    // Hızlı ayarlar ("wifi kapat", "ses 30") ve adı uyan son dosyalar.
    ranked.extend(crate::quickset::matches(&q).into_iter().map(|(s, mut it)| {
        let b = boost(cx.usage, &it.key);
        it.complete = it.title.clone();
        (s + b, it)
    }));
    ranked.extend(crate::recent::matches(&q));
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.title.len().cmp(&b.1.title.len())));
    let mut seen = std::collections::HashSet::new();
    let mut out = top;
    for (_, it) in ranked {
        if seen.insert(norm(&it.title)) {
            out.push(it);
        }
        if out.len() >= 9 {
            break;
        }
    }

    // Dosyalar
    if cx.cfg.file_search && q.chars().count() >= 2 {
        let wake = cx.wake.clone();
        cx.files.ensure(move || wake());
        for (_, p, dir) in cx.files.search(raw.trim(), 6) {
            out.push(file_item(&p, dir));
        }
    }

    // Web
    let url = cx.cfg.web_search.replace("{q}", &enc(trimmed));
    out.push(Item::new(t!(format!("“{trimmed}” için web'de ara"), format!("Search the web for “{trimmed}”")), t!("Varsayılan tarayıcıda", "In default browser"), Icon::Glyph('\u{E721}'), Act::Open(url), "Web"));
    out
}

impl Ctx<'_> {
    fn currency_enabled(&self) -> bool {
        self.cfg.currency
    }
}

/// Seçili sonuç için eylemler (Tab ya da sağ tık).
pub fn actions(it: &Item) -> Vec<Item> {
    let mut v = Vec::new();
    let g = |c: char| Icon::Glyph(c);
    match &it.act {
        Act::App { launch, .. } => {
            if let Some((hwnd, pid)) = it.run {
                v.push(Item::new(t!("Öne getir", "Bring to front"), &it.title, g('\u{E8A7}'), it.act.clone(), ""));
                v.push(Item::new(t!("Yeni pencere", "New window"), &it.title, g('\u{E710}'), Act::Open(launch.clone()), ""));
                v.push(Item::new(t!("Kapat", "Close"), t!("Uygulamaya kapanmasını söyler (zorlamaz)", "Asks the app to close (doesn't force)"), g('\u{E711}'), Act::Close(hwnd), ""));
                v.push(Item::new(t!("Yeniden başlat", "Restart"), t!("Kapatır ve yeniden açar", "Closes and reopens it"), g('\u{E72C}'), Act::Restart { pid, hwnd, launch: launch.clone() }, ""));
            } else {
                v.push(Item::new(t!("Aç", "Open"), &it.title, g('\u{E8A7}'), it.act.clone(), ""));
            }
        }
        _ => v.push(Item::new(t!("Aç", "Open"), &it.title, g('\u{E8A7}'), it.act.clone(), "")),
    }
    if !it.path.is_empty() {
        let p = &it.path;
        if p.to_lowercase().ends_with(".exe") {
            v.push(Item::new(t!("Yönetici olarak çalıştır", "Run as administrator"), p, g('\u{E7EF}'), Act::Admin(p.clone()), ""));
        }
        v.push(Item::new(t!("Dosya konumunu aç", "Open file location"), p, g('\u{E838}'), Act::Reveal(p.clone()), ""));
        v.push(Item::new(t!("Yolu kopyala", "Copy path"), p, g('\u{E8C8}'), Act::Copy(p.clone()), ""));
    }
    if let Act::Copy(s) = &it.act {
        if v.len() == 1 {
            v[0] = Item::new(t!("Sonucu kopyala", "Copy result"), s, g('\u{E8C8}'), it.act.clone(), "");
        }
    }
    // Eylemler de sayılsın diye anahtarı taşı.
    for a in &mut v {
        a.key = it.key.clone();
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoring() {
        let t = norm("Visual Studio Code");
        assert!(score(&t, "vsc") >= 600);
        assert!(score(&t, "visual") > score(&t, "code"));
        assert!(score(&t, "studio code") >= 700);
        assert_eq!(score(&norm("Görev Yöneticisi"), &norm("gorev")), 900 - 11);
        assert!(score(&norm("Notepad"), "ntpd") > 0);
        assert_eq!(score(&norm("Discord"), "xyz"), 0);
        assert_eq!(norm("IŞIK İzmir"), "isik izmir");
    }

    fn ctx_run(q: &str, f: impl FnOnce(Vec<Item>)) {
        let mut cfg = LauncherConfig::default();
        cfg.whatsapp_contacts = true;
        cfg.whatsapp.insert("Demhat".into(), "905551234567".into());
        let files = Files::default();
        let rates = Rates::default();
        let usage = HashMap::new();
        let cx = Ctx { apps: &[], run: &[], usage: &usage, files: &files, rates: &rates, cfg: &cfg, wake: Arc::new(|| {}) };
        f(build(q, &cx));
    }

    #[test]
    fn keywords() {
        ctx_run("yt", |v| assert_eq!(v[0].act, Act::Open("https://www.youtube.com".into())));
        ctx_run("yt kedi videosu", |v| assert_eq!(v[0].act, Act::Open("https://www.youtube.com/results?search_query=kedi+videosu".into())));
        ctx_run("@yt", |v| assert_eq!(v[0].complete, "@yt "));
        ctx_run("grok merhaba", |v| assert!(matches!(&v[0].act, Act::Ai { url: Some(u), .. } if u == "https://grok.com/?q=merhaba")));
        ctx_run("@c", |v| assert!(v.iter().any(|i| i.title == "ChatGPT") && v.iter().any(|i| i.title == "Claude")));
        ctx_run("wp demhat selam nasılsın", |v| assert!(matches!(&v[0].act, Act::Open(u) if u.ends_with("send?phone=905551234567&text=selam%20nas%C4%B1ls%C4%B1n"))));
        ctx_run("@wp @demhat selam", |v| assert!(matches!(&v[0].act, Act::Open(u) if u.contains("phone=905551234567"))));
        ctx_run("@wp dem", |v| assert_eq!(v[0].complete, "@wp Demhat "));
        ctx_run("wp ali", |v| assert!(v[0].title.contains("rehberde yok")));
        ctx_run("wwp", |v| assert_eq!(v[0].act, Act::Open("https://web.whatsapp.com".into())));
        ctx_run("wwp demhat selam", |v| assert_eq!(v[0].act, Act::Open("https://web.whatsapp.com/send?phone=905551234567&text=selam".into())));
        ctx_run("/lock", |v| assert_eq!(v[0].act, Act::System(Sys::Lock)));
        ctx_run("/shutdown", |v| assert_eq!(v[0].act, Act::System(Sys::Shutdown)));
        // Kısayol kelimesi olmayanlar normal aramaya gider.
        ctx_run("ytx", |v| assert!(v.iter().all(|i| i.tag != "Kısayol")));
    }

    #[test]
    fn urls() {
        assert_eq!(looks_like_url("github.com"), Some("https://github.com".into()));
        assert_eq!(looks_like_url("3.5"), None);
        assert_eq!(looks_like_url("not defteri"), None);
    }
}
