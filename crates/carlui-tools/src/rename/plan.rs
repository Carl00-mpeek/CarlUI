//! Bu modül "toplu yeniden adlandırmada hangi dosya ne olacak, çakışma var
//! mı, yarıda kalırsa ne olur?" sorununu çözüyor. Saf kısım: ayardan yeni ad
//! hesaplanır (bul/değiştir, şablon, harf), geçersiz adlar ve çakışmalar
//! işaretlenir. Uygulama iki aşamalıdır: önce hepsi geçici adlara, sonra
//! hedeflere taşınır; böylece "a→b, b→a" gibi yer değiştirmeler ve yalnızca
//! harf büyüklüğü değişen adlar da çalışır. Bir adım başarısız olursa yapılan
//! her şey geri alınır.

use carlui_core::config::{CaseKind, RenameConfig, RenameMode};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Ad değişmiyor.
    Same,
    Ok,
    /// Windows'ta geçersiz ad (boş, yasak karakter, ayrılmış ad…).
    Invalid,
    /// Aynı ada giden başka bir öğe var.
    Duplicate,
    /// Klasörde bu adda başka (seçilmemiş) bir dosya var.
    Exists,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub from: PathBuf,
    /// Yeni dosya adı (yalnızca ad, klasör değil).
    pub to: String,
    pub status: Status,
}

impl Entry {
    pub fn name(&self) -> String {
        self.from.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
    }
    pub fn target(&self) -> PathBuf {
        self.from.with_file_name(&self.to)
    }
}

/// Adı gövde + uzantıya ayırır (".gitignore" gibi baştaki nokta uzantı sayılmaz).
fn split(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

fn replace(s: &str, find: &str, with: &str, match_case: bool) -> String {
    if find.is_empty() {
        return s.to_string();
    }
    if match_case {
        return s.replace(find, with);
    }
    // Harf duyarsız: karakter karakter karşılaştır, özgün yazıyı koru.
    let hay: Vec<char> = s.chars().collect();
    let needle: Vec<char> = find.chars().collect();
    let eq = |a: char, b: char| a == b || a.to_lowercase().eq(b.to_lowercase());
    let mut out = String::new();
    let mut i = 0;
    while i < hay.len() {
        if i + needle.len() <= hay.len() && hay[i..i + needle.len()].iter().zip(&needle).all(|(a, b)| eq(*a, *b)) {
            out.push_str(with);
            i += needle.len();
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    out
}

fn title(s: &str) -> String {
    let mut out = String::new();
    let mut start = true;
    for c in s.chars() {
        if c.is_alphanumeric() {
            if start {
                out.extend(c.to_uppercase());
            } else {
                out.extend(c.to_lowercase());
            }
            start = false;
        } else {
            out.push(c);
            start = true;
        }
    }
    out
}

/// Tek bir adın yenisi. `index`: seçimdeki sıra (0'dan), `total`: öğe sayısı.
pub fn new_name(name: &str, is_dir: bool, index: usize, total: usize, cfg: &RenameConfig, date: &str) -> String {
    let (stem, ext) = if cfg.keep_ext && !is_dir { split(name) } else { (name, "") };
    let stem = match cfg.mode {
        RenameMode::Replace => replace(stem, &cfg.find, &cfg.replace, cfg.match_case),
        RenameMode::Template => {
            let n = cfg.start as usize + index;
            let last = cfg.start as usize + total.saturating_sub(1);
            let width = last.to_string().len();
            let num = format!("{n:0width$}");
            cfg.template.replace("{ad}", stem).replace("{name}", stem).replace("{n}", &num).replace("{tarih}", date).replace("{date}", date)
        }
        RenameMode::Case => match cfg.case {
            CaseKind::Lower => stem.to_lowercase(),
            CaseKind::Upper => stem.to_uppercase(),
            CaseKind::Title => title(stem),
        },
    };
    format!("{stem}{ext}")
}

pub fn valid(name: &str) -> bool {
    const RESERVED: [&str; 22] = ["CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9"];
    if name.trim().is_empty() || name.ends_with('.') || name.ends_with(' ') || name.chars().count() > 255 {
        return false;
    }
    if name.chars().any(|c| c.is_control() || "<>:\"/\\|?*".contains(c)) {
        return false;
    }
    let base = name.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    !RESERVED.contains(&base.as_str())
}

/// Seçimin planı. `exists`: diskte bu yolda bir şey var mı (testte sahte).
pub fn plan(paths: &[PathBuf], cfg: &RenameConfig, date: &str, exists: impl Fn(&Path) -> bool) -> Vec<Entry> {
    let total = paths.len();
    let mut out: Vec<Entry> = paths
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let to = new_name(&name, p.is_dir(), i, total, cfg, date);
            let status = if to == name {
                Status::Same
            } else if !valid(&to) {
                Status::Invalid
            } else {
                Status::Ok
            };
            Entry { from: p.clone(), to, status }
        })
        .collect();
    // Windows adlarda harf duyarsızdır: karşılaştırma küçük harfle.
    let key = |p: &Path| p.to_string_lossy().to_lowercase();
    let sources: std::collections::HashSet<String> = paths.iter().map(|p| key(p)).collect();
    let mut seen = std::collections::HashMap::<String, usize>::new();
    for e in &out {
        *seen.entry(key(&e.target())).or_default() += 1;
    }
    for e in &mut out {
        if e.status != Status::Ok {
            continue;
        }
        let t = e.target();
        if seen.get(&key(&t)).copied().unwrap_or(0) > 1 {
            e.status = Status::Duplicate;
        } else if !sources.contains(&key(&t)) && exists(&t) {
            e.status = Status::Exists;
        }
    }
    out
}

pub fn changes(plan: &[Entry]) -> usize {
    plan.iter().filter(|e| e.status == Status::Ok).count()
}

pub fn blocked(plan: &[Entry]) -> usize {
    plan.iter().filter(|e| matches!(e.status, Status::Invalid | Status::Duplicate | Status::Exists)).count()
}

/// Yolları iki aşamada taşır; başarılıysa yapılan (eski, yeni) çiftleri.
/// Hata olursa o ana kadar yapılanlar geri alınır.
pub fn apply(moves: &[(PathBuf, PathBuf)]) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let pid = std::process::id();
    let temps: Vec<PathBuf> = moves.iter().enumerate().map(|(i, (from, _))| from.with_file_name(format!(".carlui-ad-{pid}-{i}.tmp"))).collect();
    // 1. aşama: geçici adlar.
    for (i, (from, _)) in moves.iter().enumerate() {
        if let Err(e) = std::fs::rename(from, &temps[i]) {
            for j in (0..i).rev() {
                let _ = std::fs::rename(&temps[j], &moves[j].0);
            }
            return Err(format!("{}: {e}", from.display()));
        }
    }
    // 2. aşama: hedefler.
    for (i, (_, to)) in moves.iter().enumerate() {
        if let Err(e) = std::fs::rename(&temps[i], to) {
            for j in (0..i).rev() {
                let _ = std::fs::rename(&moves[j].1, &temps[j]);
            }
            for j in 0..moves.len() {
                let _ = std::fs::rename(&temps[j], &moves[j].0);
            }
            return Err(format!("{}: {e}", to.display()));
        }
    }
    Ok(moves.to_vec())
}

pub fn moves(plan: &[Entry]) -> Vec<(PathBuf, PathBuf)> {
    plan.iter().filter(|e| e.status == Status::Ok).map(|e| (e.from.clone(), e.target())).collect()
}

/// Yapılanı tersine çevirir.
pub fn undo(done: &[(PathBuf, PathBuf)]) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let back: Vec<(PathBuf, PathBuf)> = done.iter().map(|(a, b)| (b.clone(), a.clone())).collect();
    apply(&back)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(mode: RenameMode) -> RenameConfig {
        RenameConfig { mode, ..Default::default() }
    }

    #[test]
    fn names() {
        let mut c = cfg(RenameMode::Replace);
        c.find = "IMG".into();
        c.replace = "Tatil".into();
        assert_eq!(new_name("img_001.jpg", false, 0, 1, &c, ""), "Tatil_001.jpg");
        c.match_case = true;
        assert_eq!(new_name("img_001.jpg", false, 0, 1, &c, ""), "img_001.jpg");
        c.find = "jpg".into();
        c.match_case = false;
        assert_eq!(new_name("a.jpg", false, 0, 1, &c, ""), "a.jpg", "uzantı korunur");
        c.keep_ext = false;
        assert_eq!(new_name("a.jpg", false, 0, 1, &c, ""), "a.Tatil");

        let mut t = cfg(RenameMode::Template);
        t.template = "Tatil {n} {tarih}".into();
        assert_eq!(new_name("x.png", false, 0, 12, &t, "2026-11-05"), "Tatil 01 2026-11-05.png");
        t.template = "{ad}-{n}".into();
        t.start = 98;
        assert_eq!(new_name("x.png", false, 2, 3, &t, ""), "x-100.png");
        assert_eq!(new_name("x.png", false, 0, 3, &t, ""), "x-098.png");

        let mut k = cfg(RenameMode::Case);
        k.case = CaseKind::Title;
        assert_eq!(new_name("benim_tatil fotoğrafım.JPG", false, 0, 1, &k, ""), "Benim_Tatil Fotoğrafım.JPG");
        k.case = CaseKind::Upper;
        assert_eq!(new_name("klasör.v2", true, 0, 1, &k, ""), "KLASÖR.V2", "klasörün uzantısı yoktur");
    }

    #[test]
    fn validity() {
        assert!(valid("rapor.pdf"));
        assert!(!valid("a:b"));
        assert!(!valid("con.txt"));
        assert!(!valid("ad."));
        assert!(!valid(" "));
    }

    #[test]
    fn conflicts() {
        let dir = PathBuf::from(r"C:\x");
        let paths = vec![dir.join("a.txt"), dir.join("b.txt"), dir.join("c.txt")];
        let mut c = cfg(RenameMode::Template);
        c.template = "z".into();
        let p = plan(&paths, &c, "", |_| false);
        assert!(p.iter().all(|e| e.status == Status::Duplicate));
        // Seçimin içindeki bir ada gitmek çakışma değildir (yer değiştirme).
        c.template = "{ad}".into();
        c.mode = RenameMode::Replace;
        c.find = "a".into();
        c.replace = "b".into();
        let p = plan(&paths[..1], &c, "", |_| true);
        assert_eq!(p[0].status, Status::Exists);
        let p = plan(&paths[..2], &c, "", |_| true);
        assert_eq!(p[0].status, Status::Duplicate, "b.txt ikisinin de hedefi");
    }

    #[test]
    fn swap_and_undo() {
        let dir = std::env::temp_dir().join(format!("carlui-ren-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "A").unwrap();
        std::fs::write(dir.join("b.txt"), "B").unwrap();
        let moves = vec![(dir.join("a.txt"), dir.join("b.txt")), (dir.join("b.txt"), dir.join("a.txt"))];
        let done = apply(&moves).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "B");
        undo(&done).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "A");
        // Yalnızca harf büyüklüğü değişimi.
        apply(&[(dir.join("a.txt"), dir.join("A.txt"))]).unwrap();
        let names: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
        assert!(names.contains(&"A.txt".to_string()));
        // Başarısız adım her şeyi geri alır.
        assert!(apply(&[(dir.join("b.txt"), dir.join("c.txt")), (dir.join("yok.txt"), dir.join("d.txt"))]).is_err());
        assert!(dir.join("b.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
