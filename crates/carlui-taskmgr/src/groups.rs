//! İşlemleri uygulamaya göre gruplar: aynı exe dosyasından çalışan süreçler
//! (Firefox'un 12 süreci, Discord'un 6 süreci …) tek satırda toplanır; CPU,
//! GPU, bellek ve G/Ç toplamdır. Anahtar exe adıdır: aynı uygulamanın farklı
//! konumlardaki kopyaları (ör. Claude masaüstü + Claude Code) tek satırda birleşir.
//! Açılan gruplar sol taraftaki okla genişletilir; açılma/kapanma kısa bir
//! animasyonla olur (`ANIM`).

use crate::{procs::Row, App, SortCol};
use std::time::{Duration, Instant};

pub const ANIM: Duration = Duration::from_millis(220);

pub struct Group<'a> {
    pub key: String,
    pub name: String,
    pub path: String,
    pub rows: Vec<&'a Row>,
    pub cpu: f32,
    pub gpu: f32,
    pub mem: u64,
    pub io: f64,
    pub suspended: bool,
}

pub fn key_of(r: &Row) -> String {
    r.name.to_lowercase()
}

/// Açılma oranı 0..1 (yumuşak geçişli) ve açık mı.
pub fn openness(app: &App, key: &str) -> f32 {
    let Some((open, at)) = app.expanded.get(key) else { return 0.0 };
    let t = (at.elapsed().as_secs_f32() / ANIM.as_secs_f32()).clamp(0.0, 1.0);
    let e = 1.0 - (1.0 - t).powi(3);
    if *open { e } else { 1.0 - e }
}

pub fn animating(app: &App) -> bool {
    app.expanded.values().any(|(_, at)| at.elapsed() < ANIM)
}

pub fn build(app: &App) -> Vec<Group<'_>> {
    let f = app.filter.to_lowercase();
    let mut groups: Vec<Group> = Vec::new();
    let mut index = std::collections::HashMap::new();
    for r in app.procs.rows.iter().filter(|r| !(app.hide_windows && r.windows)) {
        let k = key_of(r);
        let i = *index.entry(k.clone()).or_insert_with(|| {
            groups.push(Group { key: k.clone(), name: r.name.clone(), path: r.path.clone(), rows: Vec::new(), cpu: 0.0, gpu: 0.0, mem: 0, io: 0.0, suspended: true });
            groups.len() - 1
        });
        let g = &mut groups[i];
        if g.path.is_empty() {
            g.path = r.path.clone();
        }
        g.rows.push(r);
        g.cpu += r.cpu;
        g.gpu += r.gpu;
        g.mem += r.mem;
        g.io += r.io;
        g.suspended &= r.suspended;
    }
    groups.retain(|g| f.is_empty() || g.name.to_lowercase().contains(&f) || g.rows.iter().any(|r| r.pid.to_string() == f));
    let (col, desc) = app.sort;
    let signs = &app.signs;
    let ord = |o: std::cmp::Ordering| if desc { o.reverse() } else { o };
    for g in &mut groups {
        g.rows.sort_by(|a, b| {
            ord(match col {
                SortCol::Name | SortCol::Pid | SortCol::Sign => a.pid.cmp(&b.pid),
                SortCol::Cpu => a.cpu.total_cmp(&b.cpu),
                SortCol::Gpu => a.gpu.total_cmp(&b.gpu),
                SortCol::Mem => a.mem.cmp(&b.mem),
                SortCol::Io => a.io.total_cmp(&b.io),
            })
        });
    }
    groups.sort_by(|a, b| {
        let first = if app.unsigned_first { signs.get(&a.path).rank().cmp(&signs.get(&b.path).rank()) } else { std::cmp::Ordering::Equal };
        first.then(ord(match col {
            SortCol::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortCol::Pid => a.rows[0].pid.cmp(&b.rows[0].pid),
            SortCol::Cpu => a.cpu.total_cmp(&b.cpu),
            SortCol::Gpu => a.gpu.total_cmp(&b.gpu),
            SortCol::Mem => a.mem.cmp(&b.mem),
            SortCol::Io => a.io.total_cmp(&b.io),
            SortCol::Sign => signs.get(&a.path).rank().cmp(&signs.get(&b.path).rank()),
        }))
    });
    groups
}

/// Tabloda bir satır: grup başlığı ya da açık grubun bir süreci.
#[derive(Clone, Copy)]
pub enum Line {
    Group(usize),
    Child(usize, usize),
}

pub fn lines(app: &App, groups: &[Group]) -> Vec<(Line, f32)> {
    let mut v = Vec::new();
    for (gi, g) in groups.iter().enumerate() {
        v.push((Line::Group(gi), 1.0));
        if g.rows.len() > 1 {
            let k = openness(app, &g.key);
            if k > 0.0 {
                v.extend((0..g.rows.len()).map(|ci| (Line::Child(gi, ci), k)));
            }
        }
    }
    v
}

/// Ok tıklanınca: yarıda tersine dönerse animasyon kaldığı yerden devam eder.
pub fn toggle(app: &mut App, key: &str) {
    let k = openness(app, key);
    let open = !app.expanded.get(key).map(|e| e.0).unwrap_or(false);
    // openness = e(t) (açılırken) ya da 1 - e(t) (kapanırken), e(t) = 1 - (1-t)^3
    let t = if open { 1.0 - (1.0 - k).cbrt() } else { 1.0 - k.cbrt() };
    let at = Instant::now().checked_sub(ANIM.mul_f32(t)).unwrap_or_else(Instant::now);
    app.expanded.insert(key.to_string(), (open, at));
}
