//! Kaldırmadan sonra geride kalanları bulur. Hiçbir şey silmez; yalnızca
//! kullanıcıya gösterilecek listeyi çıkarır.

use crate::{
    apps::App,
    names::{base, leaf, norm, Names},
    reg::{self, Key},
    safety::{self, Guard},
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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Left {
    pub kind: Kind,
    pub path: String,
    pub size: u64,
    pub on: bool,
}

fn entries(dir: &Path) -> Vec<(String, PathBuf, bool)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    rd.flatten().filter_map(|e| Some((e.file_name().to_string_lossy().into_owned(), e.path(), e.file_type().ok()?.is_dir()))).collect()
}

/// Bağlantılar izlenmez: içi başka yeri gösteren klasör hesaba katılmaz.
fn linked(p: &Path) -> bool {
    std::fs::symlink_metadata(p).map_or(true, |m| m.file_attributes() & REPARSE != 0)
}

fn size(root: &Path) -> u64 {
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
    let (l, b) = (norm(leaf(&app.location)), norm(&base(&app.name)));
    !l.is_empty() && !b.is_empty() && (l.contains(&b) || b.contains(&l))
}

/// `gone`: uygulama kaldırılmış; değilse hiçbir öğe seçili gelmez.
pub fn scan(app: &App, others: &[App], gone: bool) -> Vec<Left> {
    let guard = Guard::system();
    let taken: HashSet<String> = others.iter().flat_map(|o| Names::of(o).top).collect();
    let want = Names::of(app).without(&taken);
    let env = |n: &str| std::env::var_os(n).map(PathBuf::from);

    let mut paths = Vec::new();
    if !app.location.is_empty() && Path::new(&app.location).exists() {
        paths.push(PathBuf::from(&app.location));
    }
    let local = env("LOCALAPPDATA");
    let roots = [env("APPDATA"), local.clone(), local.as_ref().map(|l| l.join("Programs")), local.as_ref().map(|l| l.with_file_name("LocalLow")), env("ProgramData"), env("ProgramFiles"), env("ProgramFiles(x86)")];
    for r in roots.iter().flatten() {
        data_folders(r, &want, &mut paths);
    }
    for m in safety::menus() {
        menu_items(&m, &want, &mut paths);
    }
    for d in safety::desktops() {
        paths.extend(entries(&d).into_iter().filter(|(n, _, dir)| !*dir && is_link(n).is_some_and(|s| want.top.contains(&norm(s)))).map(|(_, p, _)| p));
    }

    let mut list: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    list.retain(|p| guard.path_ok(p) && !claimed(p, others) && !linked(Path::new(p)));
    let doubtful = !own_folder(app);
    let mut out: Vec<Left> = outermost(list)
        .into_iter()
        .map(|path| {
            let p = Path::new(&path);
            let (kind, size) = if p.is_dir() { (Kind::Folder, size(p)) } else { (Kind::File, p.metadata().map_or(0, |m| m.len())) };
            let on = gone && !(doubtful && path.eq_ignore_ascii_case(&app.location));
            Left { kind, size, on, path }
        })
        .collect();

    let mut ks = Vec::new();
    for root in [r"HKCU\Software", r"HKLM\SOFTWARE", r"HKLM\SOFTWARE\WOW6432Node"] {
        keys(root, &want, &mut ks);
    }
    if !app.key.is_empty() && reg::exists(&app.key) {
        ks.push(app.key.clone());
    }
    ks.retain(|k| safety::key_ok(k));
    ks.sort();
    ks.dedup();
    out.extend(ks.into_iter().map(|path| Left { kind: Kind::Key, path, size: 0, on: gone }));
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
}
