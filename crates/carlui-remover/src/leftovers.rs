//! Kaldırmadan sonra geride kalanları bulur. Hiçbir şey silmez; yalnızca
//! kullanıcıya gösterilecek listeyi çıkarır.
//!
//! İki yoldan aranır:
//! - Ad: `%APPDATA%\<ad>`, `HKCU\Software\<yayıncı>\<ad>` gibi tam ad eşleşmeleri.
//! - Kurulum klasörü (çapa): kısayol hedefi, başlangıç kaydı, hizmet, zamanlanmış
//!   görev, "uygulama yolu" kaydı bu klasörün içini gösteriyorsa uygulamanındır.
//!   Çapa yalnızca adı uygulamanınkini taşıyan, başka uygulamayla paylaşılmayan
//!   ve korunmayan bir klasör olabilir.

use crate::{
    apps::App,
    links,
    names::{self, base, norm, Names},
    reg::{self, Key},
    safety::{self, Guard},
    system,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
};

const REPARSE: u32 = 0x400;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Folder,
    File,
    Key,
    /// Kayıt anahtarındaki tek değer (`path` anahtar, `value` değerin adı).
    Value,
    /// Windows hizmeti (`path` hizmetin adı).
    Service,
    /// Zamanlanmış görev (`path` `\Klasör\Ad`).
    Task,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Left {
    pub kind: Kind,
    pub path: String,
    #[serde(default)]
    pub value: String,
    pub size: u64,
    pub on: bool,
}

impl Left {
    fn new(kind: Kind, path: String, on: bool) -> Self {
        Self { kind, path, value: String::new(), size: 0, on }
    }

    /// Listede ve raporda görünen ad.
    pub fn label(&self) -> String {
        match self.kind {
            Kind::Value => format!("{} › {}", self.path, self.value),
            Kind::Service => t!(format!("Hizmet: {}", self.path), format!("Service: {}", self.path)),
            Kind::Task => t!(format!("Zamanlanmış görev: {}", self.path), format!("Scheduled task: {}", self.path)),
            _ => self.path.clone(),
        }
    }

    /// Diskteki bir öğe mi (boyutu ve Gezgin'de gösterilebilir)?
    pub fn on_disk(&self) -> bool {
        matches!(self.kind, Kind::Folder | Kind::File)
    }
}

fn entries(dir: &Path) -> Vec<(String, PathBuf, bool)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    rd.flatten().filter_map(|e| Some((e.file_name().to_string_lossy().into_owned(), e.path(), e.file_type().ok()?.is_dir()))).collect()
}

/// Bağlantılar izlenmez: içi başka yeri gösteren klasör hesaba katılmaz.
fn linked(p: &Path) -> bool {
    std::fs::symlink_metadata(p).map_or(true, |m| m.file_attributes() & REPARSE != 0)
}

pub fn size(root: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(m) = e.metadata() else { continue };
            if m.file_attributes() & REPARSE != 0 {
                continue;
            }
            if m.is_dir() {
                stack.push(e.path());
            } else {
                total += m.len();
            }
        }
    }
    total
}

fn is_link(name: &str) -> Option<&str> {
    let (stem, ext) = name.rsplit_once('.')?;
    ext.eq_ignore_ascii_case("lnk").then_some(stem)
}

/// `%APPDATA%\<ad>` ve `%APPDATA%\<yayıncı>\<ad>` klasörleri.
fn data_folders(root: &Path, want: &Names, out: &mut Vec<PathBuf>) {
    for (name, path, dir) in entries(root) {
        let k = norm(&name);
        if !dir {
            continue;
        }
        if want.top.contains(&k) {
            out.push(path);
        } else if want.pubs.contains(&k) {
            out.extend(entries(&path).into_iter().filter(|(n, _, d)| *d && want.scoped.contains(&norm(n))).map(|(_, p, _)| p));
        }
    }
}

/// Başlat menüsü: uygulama klasörü, kısayolu, yayıncı klasöründeki kısayolu.
fn menu_items(root: &Path, want: &Names, out: &mut Vec<PathBuf>) {
    for (name, path, dir) in entries(root) {
        if !dir {
            if is_link(&name).is_some_and(|s| want.top.contains(&norm(s))) {
                out.push(path);
            }
        } else if want.top.contains(&norm(&name)) {
            out.push(path);
        } else if want.pubs.contains(&norm(&name)) {
            out.extend(entries(&path).into_iter().filter(|(n, _, d)| !*d && is_link(n).is_some_and(|s| want.scoped.contains(&norm(s)))).map(|(_, p, _)| p));
        }
    }
}

fn keys(root: &str, want: &Names, out: &mut Vec<String>) {
    let Some(k) = Key::open(root) else { return };
    for sub in k.subkeys() {
        let n = norm(&sub);
        let path = format!(r"{root}\{sub}");
        if want.top.contains(&n) {
            out.push(path);
        } else if want.pubs.contains(&n) {
            let Some(p) = Key::open(&path) else { continue };
            out.extend(p.subkeys().into_iter().filter(|s| want.scoped.contains(&norm(s))).map(|s| format!(r"{path}\{s}")));
        }
    }
}

/// Bir yol başka bir kurulu uygulamanın klasörüyle çakışıyor mu?
fn claimed(path: &str, others: &[App]) -> bool {
    others.iter().any(|o| !o.location.is_empty() && safety::overlaps(path, &o.location))
}

/// İç içe olanlardan yalnızca dıştaki kalır (o zaten içindekini de kapsar).
fn outermost(mut v: Vec<String>) -> Vec<String> {
    v.sort_by_key(|s| s.to_lowercase());
    v.dedup_by_key(|s| s.to_lowercase());
    let all = v.clone();
    v.retain(|p| !all.iter().any(|o| o != p && safety::overlaps(o, p) && o.len() < p.len()));
    v
}

/// Kurulum yeri uygulamanın adını taşımıyorsa (`D:\Oyunlar`) içinde başka şeyler
/// de olabilir: listelenir ama kendiliğinden seçilmez.
fn own_folder(app: &App) -> bool {
    names::related(&app.location, &app.name)
}

/// Uygulamanın tek başına sahip olduğu kurulum klasörü; yoksa `None`.
fn anchor(app: &App, others: &[App], guard: &Guard) -> Option<String> {
    let l = app.location.trim_end_matches('\\');
    (!l.is_empty() && !app.is_store() && own_folder(app) && guard.path_ok(l) && !claimed(l, others)).then(|| l.to_string())
}

/// Komut ya da yol metni `dir` klasörünün içini gösteriyor mu?
fn points(text: &str, dir: &str) -> bool {
    let t = text.to_lowercase().replace('/', "\\");
    let d = dir.to_lowercase();
    let d = d.trim_end_matches('\\');
    !d.is_empty() && (t.contains(&format!("{d}\\")) || t.trim().trim_matches('"') == d)
}

/// `Foo.Bar_1.2.3.4_x64__8wekyb3d8bbwe` → `Foo.Bar_8wekyb3d8bbwe`.
fn family(full: &str) -> Option<String> {
    let p: Vec<&str> = full.split('_').collect();
    (p.len() == 5 && !p[0].is_empty() && !p[4].is_empty()).then(|| format!("{}_{}", p[0], p[4]))
}

/// Komutun çalıştırdığı dosya tam yol ve diskte yok.
fn dead(cmd: &str) -> bool {
    let exe = system::exe_of(cmd);
    exe.contains(":\\") && !Path::new(&exe).exists()
}

/// Uygulamayı gösteren kısayollar: hedefi çapanın içinde olanlar ve adı
/// uygulamanın olup hedefi artık olmayanlar (alt klasörler ve görev çubuğu dahil).
fn shortcuts(app: &App, want: &Names, anchor: Option<&str>, out: &mut Vec<PathBuf>) {
    for place in safety::link_places() {
        for lnk in links::under(&place, 4) {
            let named = lnk.file_stem().is_some_and(|s| want.top.contains(&norm(&s.to_string_lossy())));
            let hit = match links::target(&lnk) {
                // Hedef bir komut değil, tam yoldur: boşluklu yollar bölünmez.
                Some(t) => anchor.is_some_and(|a| points(&t, a)) || (!Path::new(&t).exists() && (named || (!app.location.is_empty() && points(&t, &app.location)))),
                None => false,
            };
            if hit {
                out.push(lnk);
            }
        }
    }
}

/// `App Paths\foo.exe` ve `Classes\Applications\foo.exe`: çapanın içini gösteren
/// uygulama kayıtları.
fn registrations(anchor: &str, out: &mut Vec<String>) {
    for root in [r"HKCU\Software", r"HKLM\SOFTWARE", r"HKLM\SOFTWARE\WOW6432Node"] {
        let ap = format!(r"{root}\Microsoft\Windows\CurrentVersion\App Paths");
        if let Some(k) = Key::open(&ap) {
            for s in k.subkeys() {
                let p = format!(r"{ap}\{s}");
                if Key::open(&p).is_some_and(|e| ["", "Path"].iter().any(|v| e.string(v).is_some_and(|d| points(&d, anchor)))) {
                    out.push(p);
                }
            }
        }
        let ca = format!(r"{root}\Classes\Applications");
        if let Some(k) = Key::open(&ca) {
            for s in k.subkeys() {
                let p = format!(r"{ca}\{s}");
                if Key::open(&format!(r"{p}\shell\open\command")).and_then(|c| c.string("")).is_some_and(|d| points(&d, anchor)) {
                    out.push(p);
                }
            }
        }
    }
}

/// Başlangıç kayıtları (`Run`, `RunOnce`) ve Görev Yöneticisi'ndeki
/// açık/kapalı izleri. `links`: bulunan kısayolların küçük harfli dosya adları.
fn startup_values(app: &App, want: &Names, anchor: Option<&str>, links: &HashSet<String>, on: bool) -> Vec<Left> {
    let keys = safety::run_keys();
    let mut names: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for (key, _) in keys.iter().filter(|(_, approved)| !approved) {
        let Some(k) = Key::open(key) else { continue };
        for v in k.values() {
            let Some(data) = k.string(&v) else { continue };
            let gone = dead(&data);
            let hit = anchor.is_some_and(|a| points(&data, a)) || (gone && (want.top.contains(&norm(&v)) || (!app.location.is_empty() && points(&data, &app.location))));
            if hit && safety::value_ok(key, &v) {
                names.insert(v.to_lowercase());
                out.push(Left { value: v, ..Left::new(Kind::Value, key.clone(), on) });
            }
        }
    }
    for (key, _) in keys.iter().filter(|(_, approved)| *approved) {
        let Some(k) = Key::open(key) else { continue };
        let folder = key.to_ascii_lowercase().ends_with("startupfolder");
        for v in k.values() {
            let l = v.to_lowercase();
            if (if folder { links.contains(&l) } else { names.contains(&l) }) && safety::value_ok(key, &v) {
                out.push(Left { value: v, ..Left::new(Kind::Value, key.clone(), on) });
            }
        }
    }
    out
}

/// Çapanın içinden çalışan hizmetler ve zamanlanmış görevler.
fn services_and_tasks(anchor: &str, on: bool) -> Vec<Left> {
    let mut out = Vec::new();
    for (name, exe) in system::services() {
        if safety::overlaps(&exe, anchor) && safety::service_ok(&name) {
            out.push(Left::new(Kind::Service, name, on));
        }
    }
    for (task, actions) in system::tasks() {
        if points(&actions, anchor) && safety::task_ok(&task) {
            out.push(Left::new(Kind::Task, task, on));
        }
    }
    out
}

/// `gone`: uygulama kaldırılmış; değilse hiçbir öğe seçili gelmez.
pub fn scan(app: &App, others: &[App], gone: bool) -> Vec<Left> {
    let guard = Guard::system();
    let taken: HashSet<String> = others.iter().flat_map(|o| Names::of(o).top).collect();
    let want = Names::of(app).without(&taken);
    let anchor = anchor(app, others, &guard);
    let env = |n: &str| std::env::var_os(n).map(PathBuf::from);

    let mut paths = Vec::new();
    if !app.location.is_empty() && Path::new(&app.location).exists() {
        paths.push(PathBuf::from(&app.location));
    }
    let local = env("LOCALAPPDATA");
    let roots = [env("APPDATA"), local.clone(), local.as_ref().map(|l| l.join("Programs")), local.as_ref().map(|l| l.with_file_name("LocalLow")), env("ProgramData"), env("ProgramFiles"), env("ProgramFiles(x86)"), env("TEMP")];
    for r in roots.iter().flatten() {
        data_folders(r, &want, &mut paths);
    }
    // Kullanıcı klasöründeki ayar klasörleri: `~\.foo`.
    if let Some(home) = env("USERPROFILE") {
        paths.extend(entries(&home).into_iter().filter(|(n, _, d)| *d && n.starts_with('.') && want.top.contains(&norm(n))).map(|(_, p, _)| p));
    }
    // Mağaza uygulamasının kullanıcı verisi.
    if let (Some(l), Some(f)) = (&local, family(&app.package)) {
        let p = l.join("Packages").join(&f);
        if p.is_dir() && !others.iter().any(|o| family(&o.package).as_deref() == Some(f.as_str())) {
            paths.push(p);
        }
    }
    for m in safety::menus() {
        menu_items(&m, &want, &mut paths);
    }
    for d in safety::desktops() {
        paths.extend(entries(&d).into_iter().filter(|(n, _, dir)| !*dir && is_link(n).is_some_and(|s| want.top.contains(&norm(s)))).map(|(_, p, _)| p));
    }
    shortcuts(app, &want, anchor.as_deref(), &mut paths);

    let mut list: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    list.retain(|p| guard.path_ok(p) && !claimed(p, others) && !linked(Path::new(p)));
    let doubtful = !own_folder(app);
    let list = outermost(list);
    let link_names: HashSet<String> = list.iter().filter_map(|p| is_link(names::leaf(p)).map(|_| names::leaf(p).to_lowercase())).collect();
    let mut out: Vec<Left> = list
        .into_iter()
        .map(|path| {
            let p = Path::new(&path);
            let (kind, size) = if p.is_dir() { (Kind::Folder, size(p)) } else { (Kind::File, p.metadata().map_or(0, |m| m.len())) };
            let on = gone && !(doubtful && path.eq_ignore_ascii_case(&app.location));
            Left { size, ..Left::new(kind, path, on) }
        })
        .collect();

    let mut ks = Vec::new();
    for root in [r"HKCU\Software", r"HKLM\SOFTWARE", r"HKLM\SOFTWARE\WOW6432Node"] {
        keys(root, &want, &mut ks);
    }
    if let Some(a) = &anchor {
        registrations(a, &mut ks);
    }
    if !app.key.is_empty() && reg::exists(&app.key) {
        ks.push(app.key.clone());
    }
    ks.retain(|k| safety::key_ok(k));
    ks.sort_by_key(|k| k.to_lowercase());
    ks.dedup_by_key(|k| k.to_lowercase());
    out.extend(ks.into_iter().map(|path| Left::new(Kind::Key, path, gone)));
    out.extend(startup_values(app, &want, anchor.as_deref(), &link_names, gone));
    if let Some(a) = &anchor {
        out.extend(services_and_tasks(a, gone));
    }
    carlui_core::info!("kaldırıcı: {} için {} artık bulundu (çapa: {})", base(&app.name), out.len(), anchor.as_deref().unwrap_or("-"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_results_collapse() {
        let v = outermost(vec![r"C:\A\B".into(), r"C:\A".into(), r"c:\a".into(), r"C:\AB".into()]);
        assert_eq!(v.len(), 2);
        assert!(v.iter().any(|p| p.eq_ignore_ascii_case(r"C:\A")) && v.iter().any(|p| p == r"C:\AB"));
    }

    #[test]
    fn links_only() {
        assert_eq!(is_link("Foo App.lnk"), Some("Foo App"));
        assert_eq!(is_link("Foo.LNK"), Some("Foo"));
        assert_eq!(is_link("Foo.exe"), None);
    }

    #[test]
    fn other_apps_keep_their_folders() {
        let others = [App { location: r"C:\Games\Foo".into(), ..Default::default() }];
        assert!(claimed(r"C:\Games", &others));
        assert!(claimed(r"C:\Games\Foo\Bin", &others));
        assert!(!claimed(r"C:\Games\Bar", &others));
    }

    #[test]
    fn shared_install_folder_is_not_preselected() {
        let a = |name: &str, location: &str| App { name: name.into(), location: location.into(), ..Default::default() };
        assert!(own_folder(&a("Foo Editor 2.1", r"C:\Program Files\Foo Editor")));
        assert!(own_folder(&a("Google Chrome", r"C:\Program Files\Google\Chrome")));
        assert!(!own_folder(&a("Foo Editor", r"D:\Oyunlar")));
    }

    #[test]
    fn commands_point_into_folder() {
        assert!(points(r#""C:\Program Files\Foo\foo.exe" --tray"#, r"C:\Program Files\Foo"));
        assert!(points(r"c:/program files/foo/foo.exe", r"C:\Program Files\Foo\"));
        assert!(points(r"C:\Program Files\Foo", r"C:\Program Files\Foo"));
        assert!(!points(r"C:\Program Files\FooBar\foo.exe", r"C:\Program Files\Foo"));
        assert!(!points(r"C:\x.exe", ""));
    }

    #[test]
    fn store_family() {
        assert_eq!(family("Foo.Bar_1.2.3.4_x64__8wekyb3d8bbwe").as_deref(), Some("Foo.Bar_8wekyb3d8bbwe"));
        assert_eq!(family(""), None);
        assert_eq!(family("Foo_1.0"), None);
    }
}
