//! Bu modül "diskte yer kaplayan ne?" sorusunun veri kısmını çözüyor: bir
//! sürücü ya da klasör arka planda, birkaç iş parçacığıyla taranır ve boyuta
//! göre sıralı bir ağaç çıkar. `FindFirstFileExW` (temel bilgi + büyük
//! getirme) ile her klasör tek çağrıda okunur; bağlantı noktaları (junction,
//! symlink) izlenmez ki aynı yer iki kez sayılmasın. Bellek sınırlı kalsın
//! diye bir klasördeki en büyük `KEEP` dosya ayrı tutulur, kalanı tek bir
//! "küçük dosyalar" düğümünde toplanır.

use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use windows::{
    core::*,
    Win32::Storage::FileSystem::*,
};

const KEEP: usize = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File,
    Dir,
    /// Toplanmış küçük dosyalar (`count` adet).
    Rest,
}

#[derive(Debug)]
pub struct Node {
    pub name: Box<str>,
    pub size: u64,
    pub kind: Kind,
    /// Dosya sayısı (klasörde tüm alt öğeler).
    pub count: u64,
    /// Boyuta göre büyükten küçüğe.
    pub children: Vec<Node>,
}

impl Node {
    pub fn is_dir(&self) -> bool {
        self.kind == Kind::Dir
    }

    fn sort(&mut self) {
        self.children.sort_unstable_by(|a, b| b.size.cmp(&a.size));
    }

    /// `path` ile gösterilen alt öğeyi kaldırır, üst düzeylerin boyut ve
    /// sayılarını düzeltir; kaldırılanın (boyut, dosya sayısı) değeri döner.
    pub fn remove(&mut self, path: &[usize]) -> Option<(u64, u64)> {
        let (&first, rest) = path.split_first()?;
        let gone = if rest.is_empty() {
            if first >= self.children.len() {
                return None;
            }
            let g = self.children.remove(first);
            (g.size, if g.kind == Kind::File { 1 } else { g.count })
        } else {
            self.children.get_mut(first)?.remove(rest)?
        };
        self.size = self.size.saturating_sub(gone.0);
        self.count = self.count.saturating_sub(gone.1);
        Some(gone)
    }
}

#[derive(Default)]
pub struct Progress {
    pub bytes: AtomicU64,
    pub files: AtomicU64,
    pub cancel: AtomicBool,
    pub current: Mutex<String>,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn name_of(d: &WIN32_FIND_DATAW) -> String {
    let n = d.cFileName.iter().position(|&c| c == 0).unwrap_or(d.cFileName.len());
    String::from_utf16_lossy(&d.cFileName[..n])
}

/// Bir klasörün doğrudan içeriği: (dosyalar, alt klasör adları).
fn list(dir: &str) -> (Vec<(String, u64)>, Vec<String>) {
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    let pattern = wide(&format!(r"\\?\{}\*", dir.trim_end_matches('\\')));
    let mut d = WIN32_FIND_DATAW::default();
    let Ok(h) = (unsafe { FindFirstFileExW(PCWSTR(pattern.as_ptr()), FindExInfoBasic, &mut d as *mut _ as *mut _, FindExSearchNameMatch, None, FIND_FIRST_EX_LARGE_FETCH) }) else {
        return (files, dirs);
    };
    loop {
        let name = name_of(&d);
        if name != "." && name != ".." {
            let attr = d.dwFileAttributes;
            if attr & FILE_ATTRIBUTE_DIRECTORY.0 != 0 {
                if attr & FILE_ATTRIBUTE_REPARSE_POINT.0 == 0 {
                    dirs.push(name);
                }
            } else {
                files.push((name, (d.nFileSizeHigh as u64) << 32 | d.nFileSizeLow as u64));
            }
        }
        if unsafe { FindNextFileW(h, &mut d) }.is_err() {
            break;
        }
    }
    unsafe {
        let _ = FindClose(h);
    }
    (files, dirs)
}

fn build(name: &str, path: &str, prog: &Progress) -> Node {
    if prog.cancel.load(Ordering::Relaxed) {
        return Node { name: name.into(), size: 0, kind: Kind::Dir, count: 0, children: Vec::new() };
    }
    let (files, dirs) = list(path);
    if let Ok(mut c) = prog.current.try_lock() {
        c.clear();
        c.push_str(path);
    }
    let mut children: Vec<Node> = dirs.iter().map(|d| build(d, &format!(r"{}\{d}", path.trim_end_matches('\\')), prog)).collect();
    prog.files.fetch_add(files.len() as u64, Ordering::Relaxed);
    prog.bytes.fetch_add(files.iter().map(|f| f.1).sum::<u64>(), Ordering::Relaxed);
    children.extend(collapse(files));
    finish(name, children)
}

fn collapse(mut files: Vec<(String, u64)>) -> Vec<Node> {
    files.sort_unstable_by(|a, b| b.1.cmp(&a.1));
    let mut out: Vec<Node> = Vec::new();
    let rest = if files.len() > KEEP { files.split_off(KEEP) } else { Vec::new() };
    out.extend(files.into_iter().map(|(n, s)| Node { name: n.into(), size: s, kind: Kind::File, count: 1, children: Vec::new() }));
    if !rest.is_empty() {
        let size = rest.iter().map(|f| f.1).sum();
        out.push(Node { name: t!(format!("{} küçük dosya", rest.len()), format!("{} smaller files", rest.len())).into(), size, kind: Kind::Rest, count: rest.len() as u64, children: Vec::new() });
    }
    out
}

fn finish(name: &str, children: Vec<Node>) -> Node {
    let size = children.iter().map(|c| c.size).sum();
    let count = children.iter().map(|c| if c.kind == Kind::Dir { c.count } else { c.count.max(1) }).sum();
    let mut n = Node { name: name.into(), size, kind: Kind::Dir, count, children };
    n.sort();
    n
}

/// Kökü tarar. Üst düzey klasörler iş parçacıklarına dağıtılır.
pub fn scan(root: &str, prog: Arc<Progress>) -> Node {
    let (files, dirs) = list(root);
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 8);
    let queue = Mutex::new(dirs.into_iter().enumerate().collect::<Vec<_>>());
    let done = Mutex::new(Vec::<(usize, Node)>::new());
    let base = root.trim_end_matches('\\').to_string();
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let Some((i, d)) = queue.lock().unwrap_or_else(|e| e.into_inner()).pop() else { break };
                let n = build(&d, &format!(r"{base}\{d}"), &prog);
                done.lock().unwrap_or_else(|e| e.into_inner()).push((i, n));
            });
        }
    });
    prog.files.fetch_add(files.len() as u64, Ordering::Relaxed);
    prog.bytes.fetch_add(files.iter().map(|f| f.1).sum::<u64>(), Ordering::Relaxed);
    let mut children: Vec<Node> = done.into_inner().unwrap_or_default().into_iter().map(|(_, n)| n).collect();
    children.extend(collapse(files));
    finish(root, children)
}

/// Sürücünün (kullanılan, toplam) baytı.
pub fn usage(root: &str) -> Option<(u64, u64)> {
    let (mut free, mut total) = (0u64, 0u64);
    let w = wide(root);
    unsafe { GetDiskFreeSpaceExW(PCWSTR(w.as_ptr()), None, Some(&mut total), Some(&mut free)).ok()? };
    Some((total.saturating_sub(free), total))
}

/// Sabit ve çıkarılabilir sürücüler ("C:\").
pub fn drives() -> Vec<String> {
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| format!("{}:\\", (b'A' + i) as char))
        .filter(|d| {
            let t = unsafe { GetDriveTypeW(&HSTRING::from(d.as_str())) };
            // 2 = çıkarılabilir, 3 = sabit disk.
            t == 2 || t == 3
        })
        .collect()
}

pub fn human(b: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    let s = if i == 0 { format!("{b} B") } else if v >= 100.0 { format!("{v:.0} {}", U[i]) } else { format!("{v:.1} {}", U[i]) };
    // Türkçede ondalık ayırıcı virgül.
    if carlui_core::lang::is_tr() {
        s.replace('.', ",")
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_tree() {
        let dir = std::env::temp_dir().join(format!("carlui-disk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        std::fs::write(dir.join("a/b/x.bin"), vec![0u8; 3000]).unwrap();
        std::fs::write(dir.join("a/y.bin"), vec![0u8; 1000]).unwrap();
        for i in 0..(KEEP + 5) {
            std::fs::write(dir.join(format!("k{i}.txt")), "x").unwrap();
        }
        let prog = Arc::new(Progress::default());
        let mut root = scan(&dir.to_string_lossy(), prog.clone());
        assert_eq!(root.size, 4000 + KEEP as u64 + 5);
        assert_eq!(root.count, 2 + KEEP as u64 + 5);
        assert_eq!(&*root.children[0].name, "a");
        assert_eq!(root.children[0].size, 4000);
        assert!(root.children.iter().any(|c| c.kind == Kind::Rest && c.count == 5));
        assert_eq!(prog.files.load(Ordering::Relaxed), 2 + KEEP as u64 + 5);
        // a/b kaldırılınca üst düzeyler küçülür.
        let b = root.children[0].children.iter().position(|c| &*c.name == "b").unwrap();
        assert_eq!(root.remove(&[0, b]), Some((3000, 1)));
        assert_eq!(root.children[0].size, 1000);
        assert_eq!(root.size, 1000 + KEEP as u64 + 5);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sizes() {
        assert_eq!(human(512), "512 B");
        assert!(human(1536).starts_with("1"));
        assert!(human(5 * 1024 * 1024 * 1024).ends_with("GB"));
    }
}
