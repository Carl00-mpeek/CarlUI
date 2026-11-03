//! Bu modül "dosya adıyla arama, ama her tuşta diski taramadan" sorununu
//! çözüyor: kullanıcı klasörlerinin (Masaüstü, Belgeler, İndirilenler,
//! Resimler, Videolar, Müzik, OneDrive) dosya adları başlatıcı ilk açıldığında
//! arka planda bir kez listelenir. Sonraki aramalar bu bellek içi listede
//! yapılır. Liste 5 dakikadan eskiyse bir sonraki açılışta tazelenir.
//! Gizli/sistem klasörleri, `node_modules`, `.git` gibi kalabalık klasörler atlanır.

use carlui_core::info;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub struct Entry {
    /// Arama için normalleştirilmiş ad.
    pub key: String,
    pub path: PathBuf,
    pub dir: bool,
}

#[derive(Default)]
struct State {
    list: Arc<Vec<Entry>>,
    built: Option<Instant>,
    building: bool,
}

#[derive(Clone, Default)]
pub struct Files {
    st: Arc<Mutex<State>>,
}

const MAX_ENTRIES: usize = 400_000;
const MAX_DEPTH: usize = 8;
const SKIP: &[&str] = &["node_modules", ".git", ".svn", "target", "__pycache__", ".vs", ".idea", "bin", "obj", "$recycle.bin", "appdata", ".cache", ".gradle", ".npm"];

fn roots() -> Vec<PathBuf> {
    let home = std::env::var("USERPROFILE").map(PathBuf::from).unwrap_or_default();
    let mut v: Vec<PathBuf> = ["Desktop", "Documents", "Downloads", "Pictures", "Videos", "Music"].iter().map(|d| home.join(d)).collect();
    if let Ok(od) = std::env::var("OneDrive") {
        v.push(PathBuf::from(od));
    }
    // Yerelleştirilmiş/taşınmış klasörler için shell'in bildiği yolları da ekle.
    for p in crate::index::places().into_iter().take(6) {
        let pb = PathBuf::from(p.1);
        if !v.contains(&pb) {
            v.push(pb);
        }
    }
    v.retain(|p| p.is_dir());
    v
}

fn walk(dir: &PathBuf, depth: usize, out: &mut Vec<Entry>) {
    use std::os::windows::fs::MetadataExt;
    if depth > MAX_DEPTH || out.len() >= MAX_ENTRIES {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Ok(md) = e.metadata() else { continue };
        let attrs = md.file_attributes();
        // gizli (2), sistem (4), yeniden ayrıştırma noktası (0x400: bağlantılar, çevrim dışı OneDrive yer tutucuları hariç)
        if attrs & 0x6 != 0 {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let path = e.path();
        let dir = md.is_dir();
        if dir && (name.starts_with('.') || SKIP.contains(&name.to_lowercase().as_str()) || attrs & 0x400 != 0) {
            continue;
        }
        out.push(Entry { key: crate::search::norm(&name), path: path.clone(), dir });
        if dir {
            walk(&path, depth + 1, out);
        }
        if out.len() >= MAX_ENTRIES {
            return;
        }
    }
}

impl Files {
    /// Liste yoksa ya da eskiyse arka planda kur; bitince `done`.
    pub fn ensure(&self, done: impl FnOnce() + Send + 'static) {
        {
            let mut s = self.st.lock().unwrap_or_else(|e| e.into_inner());
            let fresh = s.built.is_some_and(|t| t.elapsed() < Duration::from_secs(300));
            if fresh || s.building {
                return;
            }
            s.building = true;
        }
        let st = self.st.clone();
        std::thread::Builder::new()
            .name("file-index".into())
            .spawn(move || {
                let t = Instant::now();
                let mut out = Vec::new();
                for r in roots() {
                    walk(&r, 0, &mut out);
                }
                info!("dosya listesi: {} öğe, {} ms", out.len(), t.elapsed().as_millis());
                {
                    let mut s = st.lock().unwrap_or_else(|e| e.into_inner());
                    s.list = Arc::new(out);
                    s.built = Some(Instant::now());
                    s.building = false;
                }
                done();
            })
            .ok();
    }

    pub fn ready(&self) -> bool {
        self.st.lock().map(|s| s.built.is_some()).unwrap_or(false)
    }

    /// Ada göre en iyi `limit` sonuç: (puan, yol, klasör mü).
    pub fn search(&self, q: &str, limit: usize) -> Vec<(i32, PathBuf, bool)> {
        let list = self.st.lock().map(|s| s.list.clone()).unwrap_or_default();
        let q = crate::search::norm(q);
        if q.chars().count() < 2 {
            return Vec::new();
        }
        let mut hits: Vec<(i32, &Entry)> = list
            .iter()
            .filter_map(|e| {
                let s = crate::search::score(&e.key, &q);
                (s >= 400).then_some((s, e))
            })
            .collect();
        // Aynı puanda kısa yol (daha üst klasör) önce.
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.path.as_os_str().len().cmp(&b.1.path.as_os_str().len())));
        hits.into_iter().take(limit).map(|(s, e)| (s, e.path.clone(), e.dir)).collect()
    }
}
