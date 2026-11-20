//! Bu modül "kendi arama komutlarımı ekleyebileyim" sorununu çözüyor:
//! `%APPDATA%\CarlUI\plugins` klasöründeki her `.toml` dosyası bir eklentidir.
//! Eklentinin anahtar kelimesiyle başlayan arama (ör. "gh carlui") o
//! eklentinin sonucunu verir. Üç tür vardır:
//!
//! ```toml
//! name = "GitHub"
//! keywords = ["gh", "github"]
//! glyph = "E943"                     # isteğe bağlı, Segoe Fluent Icons kodu
//! url = "https://github.com/search?q={q}"
//! ```
//!
//! * `url`: adres şablonu; `{q}` yazılan metinle (adres kodlanmış) değişir.
//! * `run` + `args`: program ya da betik yazılan metinle çalıştırılır
//!   (`args = "--ara {q}"`; `{q}` tırnak içinde verilir).
//! * `list` + `args`: betik çalıştırılır, çıktısının her satırı
//!   `başlık|alt yazı|hedef` biçiminde bir sonuçtur. Hedef boşsa başlık
//!   kopyalanır; değilse açılır (adres, dosya, `ms-settings:`…). Betik arka
//!   planda, en çok 5 saniye çalışır; sonuç bir dakika önbellekte kalır.
//!
//! Klasör iki saniyede bir yoklanır (dosyaların değişme zamanına bakılır);
//! eklenen ya da düzenlenen eklenti yeniden başlatmadan çalışır.

use crate::search::{enc, norm, Act, Icon, Item};
use carlui_core::warn;
use serde::Deserialize;
use std::{
    collections::HashMap,
    io::Read,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};

const NO_WINDOW: u32 = 0x0800_0000;
const LIST_TIMEOUT: Duration = Duration::from_secs(5);
const LIST_MAX: usize = 8;

#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Plugin {
    pub name: String,
    pub keywords: Vec<String>,
    /// Tek anahtar kelime de yazılabilsin (`keyword = "gh"`).
    pub keyword: String,
    pub glyph: String,
    pub url: String,
    pub run: String,
    pub list: String,
    pub args: String,
    /// Açıklama satırı (boşsa türüne göre yazılır).
    pub description: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Url,
    Run,
    List,
}

impl Plugin {
    pub fn parse(text: &str) -> Option<Plugin> {
        let mut p: Plugin = toml::from_str(text).ok()?;
        if !p.keyword.trim().is_empty() {
            let k = std::mem::take(&mut p.keyword);
            p.keywords.insert(0, k);
        }
        p.keywords = p.keywords.iter().map(|k| norm(k.trim().trim_start_matches('@'))).filter(|k| !k.is_empty() && !k.contains(' ')).collect();
        if p.name.trim().is_empty() || p.keywords.is_empty() {
            return None;
        }
        p.kind()?;
        Some(p)
    }

    pub fn kind(&self) -> Option<Kind> {
        if !self.url.trim().is_empty() {
            Some(Kind::Url)
        } else if !self.list.trim().is_empty() {
            Some(Kind::List)
        } else if !self.run.trim().is_empty() {
            Some(Kind::Run)
        } else {
            None
        }
    }

    fn icon(&self) -> Icon {
        let fallback = match self.kind() {
            Some(Kind::Url) => '\u{E774}',
            Some(Kind::List) => '\u{E8FD}',
            _ => '\u{E756}',
        };
        let c = u32::from_str_radix(self.glyph.trim().trim_start_matches("U+").trim_start_matches("0x"), 16).ok().and_then(char::from_u32);
        Icon::Glyph(c.unwrap_or(fallback))
    }
}

/// `{q}` yerine tırnaklı sorgu. Tırnak işareti atılır: argüman bölünemesin.
pub fn fill_args(args: &str, q: &str) -> String {
    let q: String = q.chars().filter(|c| *c != '"' && !c.is_control()).collect();
    args.replace("{q}", &format!("\"{q}\""))
}

/// `.ps1` / `.bat` / `.cmd` betiklerini uygun yorumlayıcıyla çalıştıran (program, argümanlar).
pub fn command_line(prog: &str, args: &str) -> (String, String) {
    let prog = carlui_core::config::expand_env(prog.trim());
    let lower = prog.to_lowercase();
    if lower.ends_with(".ps1") {
        ("powershell.exe".into(), format!("-NoProfile -File \"{prog}\" {args}"))
    } else if lower.ends_with(".bat") || lower.ends_with(".cmd") {
        ("cmd.exe".into(), format!("/d /c \"\"{prog}\" {args}\""))
    } else {
        (prog, args.to_string())
    }
}

/// `list` betiğinin çıktısı: `başlık|alt|hedef` satırları.
pub fn parse_lines(out: &str) -> Vec<(String, String, String)> {
    out.lines()
        .filter_map(|l| {
            let mut it = l.splitn(3, '|').map(str::trim);
            let title = it.next().filter(|t| !t.is_empty())?.to_string();
            Some((title, it.next().unwrap_or("").to_string(), it.next().unwrap_or("").to_string()))
        })
        .take(LIST_MAX)
        .collect()
}

/// Eklenti klasörü önbelleği: (son yoklama, dosyaların imzası, eklentiler).
type Loaded = (Instant, Vec<(PathBuf, SystemTime)>, Arc<Vec<Plugin>>);
static PLUGINS: Mutex<Option<Loaded>> = Mutex::new(None);

fn stamp(dir: &Path) -> Vec<(PathBuf, SystemTime)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<(PathBuf, SystemTime)> = rd
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("toml")))
        .filter_map(|e| Some((e.path(), e.metadata().and_then(|m| m.modified()).ok()?)))
        .collect();
    v.sort();
    v
}

/// Yüklü eklentiler (klasör en çok iki saniyede bir yoklanır).
pub fn all() -> Arc<Vec<Plugin>> {
    let mut g = PLUGINS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, _, list)) = g.as_ref() {
        if at.elapsed() < Duration::from_secs(2) {
            return list.clone();
        }
    }
    let files = stamp(&carlui_core::paths::plugins_dir());
    if let Some((at, old, list)) = g.as_mut() {
        if *old == files {
            *at = Instant::now();
            return list.clone();
        }
    }
    let list: Vec<Plugin> = files
        .iter()
        .filter_map(|(p, _)| {
            let text = std::fs::read_to_string(p).ok()?;
            let r = Plugin::parse(&text);
            if r.is_none() {
                warn!("eklenti okunamadı: {} (name, keywords ve url/run/list gerekli)", p.display());
            }
            r
        })
        .collect();
    let list = Arc::new(list);
    *g = Some((Instant::now(), files, list.clone()));
    list
}

#[derive(Clone)]
enum Listed {
    Loading,
    Ready(Vec<(String, String, String)>),
    Failed,
}

static LISTS: Mutex<Option<HashMap<(String, String), (Listed, Instant)>>> = Mutex::new(None);

fn run_list(p: &Plugin, q: &str) -> Option<String> {
    let (prog, args) = command_line(&p.list, &fill_args(&p.args, q));
    let mut child = Command::new(prog).raw_arg(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).creation_flags(NO_WINDOW).spawn().ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < LIST_TIMEOUT => std::thread::sleep(Duration::from_millis(30)),
            _ => {
                let _ = child.kill();
                warn!("eklenti '{}' {} sn içinde bitmedi", p.name, LIST_TIMEOUT.as_secs());
                return None;
            }
        }
    }
    let mut buf = Vec::new();
    child.stdout.take()?.take(64 * 1024).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn listed(p: &Plugin, q: &str, wake: Arc<dyn Fn() + Send + Sync>) -> Listed {
    let key = (p.name.clone(), q.to_string());
    {
        let mut g = LISTS.lock().unwrap_or_else(|e| e.into_inner());
        let m = g.get_or_insert_with(HashMap::new);
        m.retain(|_, (_, at)| at.elapsed() < Duration::from_secs(60));
        if let Some((r, _)) = m.get(&key) {
            return r.clone();
        }
        m.insert(key.clone(), (Listed::Loading, Instant::now()));
    }
    let p = p.clone();
    let _ = std::thread::Builder::new().name("plugin".into()).spawn(move || {
        let r = run_list(&p, &key.1).map(|o| Listed::Ready(parse_lines(&o))).unwrap_or(Listed::Failed);
        LISTS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(key, (r, Instant::now()));
        wake();
    });
    Listed::Loading
}

fn base(p: &Plugin, title: String, sub: String, act: Act, kw: &str) -> Item {
    let mut it = Item::new(title, sub, p.icon(), act, "Eklenti");
    it.complete = format!("{kw} ");
    it.key = format!("plugin:{}", p.keywords[0]);
    it
}

/// Bir eklentinin sonuçları (`rest` = anahtar kelimeden sonrası).
pub fn items_for(p: &Plugin, kw: &str, rest: &str, wake: Arc<dyn Fn() + Send + Sync>) -> Vec<Item> {
    let desc = |d: &str| if p.description.trim().is_empty() { d.to_string() } else { p.description.clone() };
    match p.kind() {
        Some(Kind::Url) => {
            let url = if rest.is_empty() && p.url.contains("{q}") {
                // Sorgusuz: sitenin kendisi.
                p.url.split(['?', '#']).next().unwrap_or(&p.url).trim_end_matches('/').to_string()
            } else {
                p.url.replace("{q}", &enc(rest))
            };
            let title = if rest.is_empty() { p.name.clone() } else { format!("{}: {rest}", p.name) };
            vec![base(p, title, desc(&url), Act::Open(url), kw)]
        }
        Some(Kind::Run) => {
            let (exe, args) = command_line(&p.run, &fill_args(&p.args, rest));
            let title = if rest.is_empty() { p.name.clone() } else { format!("{}: {rest}", p.name) };
            vec![base(p, title, desc(t!("Eklenti · Enter ile çalıştır", "Plugin · Enter to run")), Act::Run { exe, args }, kw)]
        }
        Some(Kind::List) => match listed(p, rest, wake) {
            Listed::Loading => vec![base(p, p.name.clone(), t!("Aranıyor…", "Searching…").into(), Act::Pending, kw)],
            Listed::Failed => vec![base(p, p.name.clone(), t!("Eklenti çalışmadı (günlüğe bakın)", "Plugin failed (see the log)").into(), Act::Pending, kw)],
            Listed::Ready(v) if v.is_empty() => vec![base(p, p.name.clone(), t!("Sonuç yok", "No results").into(), Act::None, kw)],
            Listed::Ready(v) => v
                .into_iter()
                .map(|(title, sub, target)| {
                    let act = if target.is_empty() { Act::Copy(title.clone()) } else { Act::Open(target.clone()) };
                    let sub = if sub.is_empty() { p.name.clone() } else { sub };
                    let mut it = base(p, title, sub, act, kw);
                    if target.contains(":\\") {
                        it.path = target;
                    }
                    it
                })
                .collect(),
        },
        None => Vec::new(),
    }
}

/// Aramanın ilk kelimesi bir eklentinin anahtar kelimesiyse onun sonuçları.
/// "@" ile başlayan yarım kelimede uyan eklentiler listelenir.
pub fn matches(list: &[Plugin], raw: &str, wake: Arc<dyn Fn() + Send + Sync>) -> Option<Vec<Item>> {
    let t = raw.trim_start();
    let at = t.starts_with('@');
    let body = t.trim_start_matches('@');
    let (word, rest) = match body.find(' ') {
        Some(i) => (&body[..i], body[i + 1..].trim()),
        None => (body, ""),
    };
    let w = norm(word);
    if w.is_empty() && !at {
        return None;
    }
    let prefix = if at { "@" } else { "" };
    if let Some(p) = list.iter().find(|p| p.keywords.contains(&w)) {
        return Some(items_for(p, &format!("{prefix}{w}"), rest, wake));
    }
    if !at || body.contains(' ') {
        return None;
    }
    let v: Vec<Item> = list
        .iter()
        .filter(|p| p.keywords.iter().any(|k| k.starts_with(&w)))
        .map(|p| {
            let mut it = base(p, p.name.clone(), format!("@{} · {}", p.keywords[0], t!("Eklenti", "Plugin")), Act::None, &format!("@{}", p.keywords[0]));
            it.act = Act::None;
            it
        })
        .collect();
    (!v.is_empty()).then_some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wake() -> Arc<dyn Fn() + Send + Sync> {
        Arc::new(|| {})
    }

    #[test]
    fn eklenti_dosyasi_okunur() {
        let p = Plugin::parse("name = \"GitHub\"\nkeywords = [\"gh\", \"GitHub\"]\nurl = \"https://github.com/search?q={q}\"").unwrap();
        assert_eq!(p.keywords, vec!["gh", "github"]);
        assert_eq!(p.kind(), Some(Kind::Url));
        let p = Plugin::parse("name = \"Betik\"\nkeyword = \"@bt\"\nrun = \"a.ps1\"").unwrap();
        assert_eq!(p.keywords, vec!["bt"]);
        assert_eq!(p.kind(), Some(Kind::Run));
        // Adı, anahtar kelimesi ya da türü olmayan eklenti yok sayılır.
        assert!(Plugin::parse("name = \"x\"\nurl = \"https://a\"").is_none());
        assert!(Plugin::parse("name = \"x\"\nkeyword = \"x\"").is_none());
        assert!(Plugin::parse("bozuk = = toml").is_none());
    }

    #[test]
    fn adres_eklentisi_sorguyu_kodlar() {
        let list = vec![Plugin::parse("name = \"GitHub\"\nkeyword = \"gh\"\nurl = \"https://github.com/search?q={q}\"").unwrap()];
        let v = matches(&list, "gh carl ui", wake()).unwrap();
        assert_eq!(v[0].act, Act::Open("https://github.com/search?q=carl+ui".into()));
        assert_eq!(v[0].complete, "gh ");
        // Sorgusuz: sitenin kendisi.
        assert_eq!(matches(&list, "gh", wake()).unwrap()[0].act, Act::Open("https://github.com/search".into()));
        assert!(matches(&list, "ghx", wake()).is_none());
        assert_eq!(matches(&list, "@g", wake()).unwrap()[0].complete, "@gh ");
    }

    #[test]
    fn betik_argumanlari_tirnaklanir() {
        assert_eq!(fill_args("--ara {q}", "a \"b\" & c"), "--ara \"a b & c\"");
        let (exe, args) = command_line("C:\\x\\ara.ps1", "\"q\"");
        assert_eq!(exe, "powershell.exe");
        assert_eq!(args, "-NoProfile -File \"C:\\x\\ara.ps1\" \"q\"");
        assert_eq!(command_line("notepad.exe", "a").0, "notepad.exe");
    }

    #[test]
    fn liste_ciktisi_ayrilir() {
        let v = parse_lines("Bir|alt|https://a\n\n  |boş başlık\nİki\nÜç||C:\\x.txt\n");
        assert_eq!(v.len(), 3);
        assert_eq!(v[0], ("Bir".into(), "alt".into(), "https://a".into()));
        assert_eq!(v[1], ("İki".into(), String::new(), String::new()));
        assert_eq!(v[2].2, "C:\\x.txt");
    }
}
