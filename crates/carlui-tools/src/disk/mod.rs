//! Bu modül "diskim neden doldu?" sorununu çözüyor: disk analizörü. Bir
//! sürücü arka planda taranır, sonuç ağaç haritası (kutunun alanı = boyut)
//! ve en büyük öğeler listesiyle gösterilir. Kutuya tıklayınca o klasöre
//! inilir, üstteki yol ile geri çıkılır. Sağ tık: aç, Gezgin'de göster, yolu
//! kopyala, Geri Dönüşüm Kutusu'na taşı (kalıcı silme yok; Windows kendi
//! onayını sorar). Kısayol: `shortcuts.disk` (varsayılan boş) ya da Ayarlar.

pub mod scan;
pub mod treemap;

use crate::view::{self, Ctx, Opts, View};
use carlui_ui::{
    glassmenu::{self, Icon, Row},
    paint::{mix, Align, Painter, R, TEXT},
};
use scan::{human, Kind, Node, Progress};
use std::{
    sync::{atomic::Ordering, Arc, Mutex},
    time::Instant,
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        UI::{
            Shell::{SHFileOperationW, ShellExecuteW, FOF_ALLOWUNDO, FOF_WANTNUKEWARNING, FO_DELETE, SHFILEOPSTRUCTW},
            WindowsAndMessaging::{GetCursorPos, SW_SHOWNORMAL},
        },
    },
};

const PALETTE: [u32; 8] = [0x4C9AFF, 0x36B37E, 0xFFAB00, 0xE5646E, 0x9B7BF7, 0x2BC4C4, 0xF78C40, 0xC77DBA];
const FILE: u32 = 0x8A8F98;
const LIST_W: f32 = 340.0;
const ROW: f32 = 30.0;

#[derive(Clone, Debug)]
enum A {
    Drive(usize),
    Rescan,
    Stop,
    Up,
    Crumb(usize),
    /// Geçerli klasörün alt öğesi; isteğe bağlı torunu.
    Item(usize, Option<usize>),
    Nothing,
}

struct Scan {
    prog: Arc<Progress>,
    started: Instant,
    used: u64,
    gen: u64,
}

struct Disk {
    drives: Vec<(String, Option<(u64, u64)>)>,
    root: String,
    tree: Option<Node>,
    /// Kökten geçerli klasöre alt öğe sıraları.
    path: Vec<usize>,
    scanning: Option<Scan>,
    result: Arc<Mutex<Option<(u64, Node)>>>,
    gen: u64,
    took: f32,
    list_scroll: usize,
    status: String,
}

fn count(n: u64) -> String {
    let s = n.to_string();
    let sep = if carlui_core::lang::is_tr() { '.' } else { ',' };
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(sep);
        }
        out.push(c);
    }
    out
}

fn open(path: &str) {
    unsafe {
        ShellExecuteW(HWND::default(), w!("open"), &HSTRING::from(path), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

fn reveal(path: &str) {
    unsafe {
        ShellExecuteW(HWND::default(), w!("open"), w!("explorer.exe"), &HSTRING::from(format!("/select,\"{path}\"")), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

/// Geri Dönüşüm Kutusu'na gönderir; Windows kendi onayını gösterir.
fn recycle(owner: HWND, path: &str) -> bool {
    let from: Vec<u16> = path.encode_utf16().chain([0, 0]).collect();
    let mut op = SHFILEOPSTRUCTW { hwnd: owner, wFunc: FO_DELETE, pFrom: PCWSTR(from.as_ptr()), fFlags: (FOF_ALLOWUNDO | FOF_WANTNUKEWARNING).0 as u16, ..Default::default() };
    let r = unsafe { SHFileOperationW(&mut op) };
    let ok = r == 0 && !op.fAnyOperationsAborted.as_bool() && !std::path::Path::new(path).exists();
    carlui_core::info!("disk: geri dönüşüme {path}: {}", if ok { "tamam" } else { "vazgeçildi/başarısız" });
    ok
}

impl Disk {
    fn new(target: Option<String>) -> Self {
        let drives: Vec<(String, Option<(u64, u64)>)> = scan::drives().into_iter().map(|d| {
            let u = scan::usage(&d);
            (d, u)
        }).collect();
        let sys = std::env::var("SystemDrive").map(|s| format!("{s}\\")).unwrap_or_else(|_| "C:\\".into());
        let root = target.filter(|t| std::path::Path::new(t).is_dir()).unwrap_or_else(|| if drives.iter().any(|d| d.0.eq_ignore_ascii_case(&sys)) { sys } else { drives.first().map(|d| d.0.clone()).unwrap_or(sys) });
        let mut d = Self { drives, root: String::new(), tree: None, path: Vec::new(), scanning: None, result: Arc::new(Mutex::new(None)), gen: 0, took: 0.0, list_scroll: 0, status: String::new() };
        d.start(root);
        d
    }

    fn start(&mut self, root: String) {
        if let Some(s) = &self.scanning {
            s.prog.cancel.store(true, Ordering::Relaxed);
        }
        self.gen += 1;
        let prog = Arc::new(Progress::default());
        let used = scan::usage(&root).map(|u| u.0).filter(|_| root.len() <= 3).unwrap_or(0);
        self.scanning = Some(Scan { prog: prog.clone(), started: Instant::now(), used, gen: self.gen });
        self.tree = None;
        self.path.clear();
        self.list_scroll = 0;
        self.root = root.clone();
        let (res, gen) = (self.result.clone(), self.gen);
        carlui_core::info!("disk: {root} taranıyor");
        std::thread::spawn(move || {
            let n = scan::scan(&root, prog);
            *res.lock().unwrap_or_else(|e| e.into_inner()) = Some((gen, n));
        });
    }

    /// Tarama bittiyse sonucu al.
    fn poll(&mut self) {
        let Some(s) = &self.scanning else { return };
        let done = self.result.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some((gen, n)) = done {
            if gen == s.gen {
                self.took = s.started.elapsed().as_secs_f32();
                carlui_core::info!("disk: {} öğe, {} ({:.1} sn)", n.count, human(n.size), self.took);
                self.tree = Some(n);
                self.scanning = None;
            }
        }
    }

    fn current(&self) -> Option<&Node> {
        let mut n = self.tree.as_ref()?;
        for &i in &self.path {
            n = n.children.get(i)?;
        }
        Some(n)
    }

    fn full(&self, rel: &[usize]) -> String {
        let mut s = self.root.trim_end_matches('\\').to_string();
        let Some(mut n) = self.tree.as_ref() else { return s };
        for &i in self.path.iter().chain(rel) {
            let Some(c) = n.children.get(i) else { break };
            if c.kind == Kind::Rest {
                break;
            }
            s.push('\\');
            s.push_str(&c.name);
            n = c;
        }
        if s.len() == 2 {
            s.push('\\');
        }
        s
    }

    fn paint_scanning(&self, p: &mut Painter<A>, area: R) {
        let Some(s) = &self.scanning else { return };
        let files = s.prog.files.load(Ordering::Relaxed);
        let bytes = s.prog.bytes.load(Ordering::Relaxed);
        let cy = area.y + area.h / 2.0 - 70.0;
        let cx = area.x + area.w / 2.0;
        p.text(&t!(format!("{} taranıyor…", self.root), format!("Scanning {}…", self.root)), R::new(area.x, cy, area.w, 30.0), 18.0, true, TEXT, 0.95, Align::Center);
        let bar = R::new(cx - 220.0, cy + 44.0, 440.0, 8.0);
        if s.used > 0 {
            p.bar(bar, bytes as f32 / s.used as f32, p.accent);
        } else {
            // Klasör taraması: toplam bilinmiyor, kayan çubuk.
            let t = s.started.elapsed().as_secs_f32();
            let x = (t * 0.6).fract();
            p.round(bar, 4.0, 0xFFFFFF, 0.08);
            p.round(R::new(bar.x + bar.w * x * 0.75, bar.y, bar.w * 0.25, bar.h), 4.0, p.accent, 0.9);
        }
        let line = if s.used > 0 { format!("{} {} · {} / {}", count(files), t!("dosya", "files"), human(bytes), human(s.used)) } else { format!("{} {} · {}", count(files), t!("dosya", "files"), human(bytes)) };
        p.text(&line, R::new(area.x, cy + 62.0, area.w, 22.0), 13.0, false, TEXT, 0.75, Align::Center);
        let cur = s.prog.current.lock().map(|c| c.clone()).unwrap_or_default();
        p.text(&cur, R::new(area.x + 40.0, cy + 86.0, area.w - 80.0, 20.0), 11.5, false, TEXT, 0.4, Align::Center);
        p.button(R::new(cx - 60.0, cy + 120.0, 120.0, 32.0), Some('\u{E71A}'), t!("Durdur", "Stop"), A::Stop, true, false);
    }

    fn paint_map(&mut self, p: &mut Painter<A>, area: R) {
        let Some(node) = self.current() else { return };
        let sizes: Vec<u64> = node.children.iter().map(|c| c.size).collect();
        let rects = treemap::layout(&sizes, area);
        let mut hover: Option<String> = None;
        for (i, (c, q)) in node.children.iter().zip(&rects).enumerate() {
            if q.w < 1.5 || q.h < 1.5 {
                continue;
            }
            let base = if c.is_dir() { PALETTE[i % PALETTE.len()] } else { FILE };
            let q = q.inset(1.0);
            let hot = p.hover(q);
            p.round(q, 3.0, base, if hot { 0.42 } else { 0.30 });
            p.outline(q, 3.0, base, 0.65);
            p.hit(q, A::Item(i, None));
            let label = q.w > 60.0 && q.h > 22.0;
            if c.is_dir() && q.w > 44.0 && q.h > 44.0 && !c.children.is_empty() {
                let head = 20.0;
                p.text(&format!("{}  {}", c.name, human(c.size)), R::new(q.x + 6.0, q.y + 1.0, q.w - 12.0, head), 11.5, true, TEXT, 0.92, Align::Left);
                let inner = R::new(q.x + 3.0, q.y + head, q.w - 6.0, q.h - head - 3.0);
                let sub: Vec<u64> = c.children.iter().map(|g| g.size).collect();
                for (j, (g, gq)) in c.children.iter().zip(treemap::layout(&sub, inner)).enumerate() {
                    if gq.w < 2.0 || gq.h < 2.0 {
                        continue;
                    }
                    let gq = gq.inset(0.5);
                    let col = if g.is_dir() { mix(base, 0xFFFFFF, 0.15) } else { mix(base, FILE, 0.5) };
                    let ghot = p.hover(gq);
                    p.fill(gq, col, if ghot { 0.55 } else { 0.34 });
                    if gq.w > 70.0 && gq.h > 18.0 {
                        p.text(&g.name, R::new(gq.x + 4.0, gq.y, gq.w - 8.0, 18.0), 10.5, false, TEXT, 0.8, Align::Left);
                    }
                    p.hit(gq, A::Item(i, Some(j)));
                    if ghot {
                        hover = Some(format!("{}   {}", self.full(&[i, j]), human(g.size)));
                    }
                }
            } else if label {
                p.text(&c.name, R::new(q.x + 6.0, q.y + 2.0, q.w - 12.0, 18.0), 11.5, true, TEXT, 0.9, Align::Left);
                if q.h > 40.0 {
                    p.text(&human(c.size), R::new(q.x + 6.0, q.y + 18.0, q.w - 12.0, 18.0), 11.0, false, TEXT, 0.6, Align::Left);
                }
            }
            if hot && hover.is_none() {
                let what = if c.kind == Kind::Rest { format!("{} · {}", self.full(&[]), c.name) } else { self.full(&[i]) };
                hover = Some(format!("{what}   {}", human(c.size)));
            }
        }
        if let Some(h) = hover {
            self.status = h;
        }
    }

    fn paint_list(&mut self, p: &mut Painter<A>, area: R) {
        let rows = ((area.h - 46.0) / ROW).floor().max(1.0) as usize;
        let len = self.current().map_or(0, |n| n.children.len());
        self.list_scroll = self.list_scroll.min(len.saturating_sub(rows));
        let Some(node) = self.current() else { return };
        p.card(area);
        p.text(t!("En büyük öğeler", "Largest items"), R::new(area.x + 14.0, area.y + 8.0, area.w - 28.0, 24.0), 13.0, true, TEXT, 0.85, Align::Left);
        let top = area.y + 38.0;
        let max = node.children.first().map(|c| c.size).unwrap_or(1).max(1);
        let mut hover = None;
        for (k, c) in node.children.iter().enumerate().skip(self.list_scroll).take(rows) {
            let r = R::new(area.x + 6.0, top + (k - self.list_scroll) as f32 * ROW, area.w - 12.0, ROW - 2.0);
            if p.hover(r) {
                p.round(r, 5.0, 0xFFFFFF, 0.06);
                hover = Some(k);
            }
            let (g, col) = match c.kind {
                Kind::Dir => ('\u{E8B7}', PALETTE[k % PALETTE.len()]),
                Kind::File => ('\u{E8A5}', FILE),
                Kind::Rest => ('\u{E8FD}', FILE),
            };
            p.glyph(g, R::new(r.x + 4.0, r.y, 22.0, r.h - 6.0), 13.0, col, 0.95);
            p.text(&c.name, R::new(r.x + 32.0, r.y, r.w - 120.0, r.h - 6.0), 12.5, false, TEXT, 0.9, Align::Left);
            p.text(&human(c.size), R::new(r.right() - 86.0, r.y, 80.0, r.h - 6.0), 12.0, false, TEXT, 0.7, Align::Right);
            p.round(R::new(r.x + 32.0, r.bottom() - 6.0, (r.w - 40.0) * (c.size as f32 / max as f32), 3.0), 1.5, col, 0.75);
            p.hit(r, A::Item(k, None));
        }
        if let Some(k) = hover {
            let c = &node.children[k];
            let what = if c.kind == Kind::Rest { format!("{} · {}", self.full(&[]), c.name) } else { self.full(&[k]) };
            self.status = format!("{what}   {}", human(c.size));
        }
    }

    fn menu(&mut self, rel: Vec<usize>, ctx: &mut Ctx) {
        let Some(node) = self.current() else { return };
        let Some(c) = rel.iter().try_fold(node, |n, &i| n.children.get(i)) else { return };
        let path = self.full(&rel);
        let rest = c.kind == Kind::Rest;
        let mut rows = vec![Row::Label(format!("{}  ·  {}", c.name, human(c.size)), Icon::Glyph(if c.is_dir() { '\u{E8B7}' } else { '\u{E8A5}' }, None))];
        if !rest {
            rows.push(Row::Item { id: 0, text: t!("Aç", "Open").into(), icon: Icon::Glyph('\u{E8E5}', None) });
            rows.push(Row::Item { id: 1, text: t!("Gezgin'de göster", "Show in Explorer").into(), icon: Icon::Glyph('\u{EC50}', None) });
            rows.push(Row::Item { id: 2, text: t!("Yolu kopyala", "Copy path").into(), icon: Icon::Glyph('\u{E8C8}', None) });
            rows.push(Row::Sep);
            rows.push(Row::Item { id: 3, text: t!("Geri Dönüşüm Kutusu'na taşı", "Move to Recycle Bin").into(), icon: Icon::Glyph('\u{E74D}', Some(0xFF6B6B)) });
        } else {
            rows.push(Row::Item { id: 0, text: t!("Klasörü aç", "Open folder").into(), icon: Icon::Glyph('\u{E8DA}', None) });
        }
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        let Some(id) = glassmenu::run(ctx.hwnd, rows, None, (pt.x, pt.y)) else { return };
        match (id, rest) {
            (0, _) => open(&path),
            (1, _) => reveal(&path),
            (2, _) => {
                carlui_clip::write::set(ctx.hwnd, carlui_clip::write::Data::Text(&path));
            }
            (3, _) => {
                if recycle(ctx.hwnd, &path) {
                    let full: Vec<usize> = self.path.iter().chain(&rel).copied().collect();
                    if let Some(t) = self.tree.as_mut() {
                        t.remove(&full);
                    }
                    self.status = t!(format!("Geri Dönüşüm Kutusu'na taşındı: {path}"), format!("Moved to Recycle Bin: {path}"));
                }
            }
            _ => {}
        }
    }
}

impl View for Disk {
    type A = A;

    fn paint(&mut self, p: &mut Painter<A>, w: f32, h: f32) {
        self.poll();
        let pad = 16.0;
        p.hit(R::new(0.0, 0.0, w, h), A::Nothing);
        // Sürücüler.
        let mut x = pad;
        for (i, (d, u)) in self.drives.iter().enumerate() {
            let label = match u {
                Some((used, total)) => format!("{}  {} / {}", d.trim_end_matches('\\'), human(*used), human(*total)),
                None => d.trim_end_matches('\\').to_string(),
            };
            let bw = p.fonts.width(&label, 13.0, false) + 44.0;
            let r = R::new(x, 12.0, bw, 34.0);
            let sel = self.root.eq_ignore_ascii_case(d);
            if sel {
                p.round(r, 6.0, p.accent, 0.22);
                p.outline(r, 6.0, p.accent, 0.8);
                p.glyph('\u{EDA2}', R::new(r.x + 4.0, r.y, 24.0, r.h), 13.0, TEXT, 0.9);
                p.text(&label, R::new(r.x + 30.0, r.y, bw - 34.0, r.h), 13.0, false, TEXT, 0.95, Align::Left);
                p.hit(r, A::Drive(i));
            } else {
                p.button(r, Some('\u{EDA2}'), &label, A::Drive(i), true, false);
            }
            if let Some((used, total)) = u {
                p.round(R::new(r.x + 8.0, r.bottom() - 5.0, (r.w - 16.0) * (*used as f32 / (*total).max(1) as f32), 2.0), 1.0, p.accent, 0.8);
            }
            x += bw + 8.0;
        }
        p.button(R::new(w - pad - 128.0, 12.0, 128.0, 34.0), Some('\u{E72C}'), t!("Yeniden tara", "Rescan"), A::Rescan, self.scanning.is_none(), false);

        let body = R::new(pad, 98.0, w - pad * 2.0, h - 98.0 - 38.0);
        if self.scanning.is_some() {
            self.paint_scanning(p, body);
            return;
        }
        // Yol: kök › klasör › klasör.
        let mut x = pad;
        let y = 58.0;
        if !self.path.is_empty() {
            p.button(R::new(x, y, 32.0, 30.0), Some('\u{E74A}'), "", A::Up, true, false);
            x += 40.0;
        }
        let mut names = vec![self.root.trim_end_matches('\\').to_string()];
        if let Some(mut n) = self.tree.as_ref() {
            for &i in &self.path {
                let Some(c) = n.children.get(i) else { break };
                names.push(c.name.to_string());
                n = c;
            }
        }
        for (k, name) in names.iter().enumerate() {
            let last = k + 1 == names.len();
            let tw = p.fonts.width(name, 13.0, last).min(220.0);
            let r = R::new(x, y, tw + 12.0, 30.0);
            if !last && p.hover(r) {
                p.round(r, 5.0, 0xFFFFFF, 0.08);
            }
            p.text(name, R::new(x + 6.0, y, tw, 30.0), 13.0, last, TEXT, if last { 0.95 } else { 0.65 }, Align::Left);
            if !last {
                p.hit(r, A::Crumb(k));
                p.glyph('\u{E76C}', R::new(r.right(), y, 18.0, 30.0), 9.0, TEXT, 0.4);
            }
            x += tw + 12.0 + 18.0;
        }
        if let Some(n) = self.current() {
            let info = format!("{} · {} {}", human(n.size), count(n.count), t!("dosya", "files"));
            p.text(&info, R::new(w - pad - 400.0, y, 400.0, 30.0), 12.5, false, TEXT, 0.6, Align::Right);
        }
        self.status.clear();
        let map = R::new(body.x, body.y, body.w - LIST_W - 12.0, body.h);
        p.round(map, 6.0, 0x000000, 0.2);
        self.paint_map(p, map.inset(2.0));
        self.paint_list(p, R::new(map.right() + 12.0, body.y, LIST_W, body.h));
        let status = if self.status.is_empty() { t!(format!("Tarama {:.1} sn sürdü · Bir kutuya tıklayıp içine inin, sağ tıkla işlem yapın", self.took), format!("Scan took {:.1} s · Click a box to open it, right-click for actions", self.took)) } else { self.status.clone() };
        p.text(&status, R::new(pad, h - 34.0, w - pad * 2.0, 28.0), 12.0, false, TEXT, 0.6, Align::Left);
    }

    fn click(&mut self, a: A, _ctx: &mut Ctx) {
        match a {
            A::Drive(i) => {
                if let Some(d) = self.drives.get(i).map(|d| d.0.clone()) {
                    self.drives[i].1 = scan::usage(&d);
                    self.start(d);
                }
            }
            A::Rescan => self.start(self.root.clone()),
            A::Stop => {
                if let Some(s) = &self.scanning {
                    s.prog.cancel.store(true, Ordering::Relaxed);
                }
            }
            A::Up => {
                self.path.pop();
                self.list_scroll = 0;
            }
            A::Crumb(k) => {
                self.path.truncate(k);
                self.list_scroll = 0;
            }
            A::Item(i, _) => {
                if self.current().and_then(|n| n.children.get(i)).is_some_and(|c| c.is_dir() && !c.children.is_empty()) {
                    self.path.push(i);
                    self.list_scroll = 0;
                }
            }
            A::Nothing => {}
        }
    }

    fn right_click(&mut self, a: A, ctx: &mut Ctx) {
        if let A::Item(i, j) = a {
            let rel: Vec<usize> = std::iter::once(i).chain(j).collect();
            self.menu(rel, ctx);
        }
    }

    fn key(&mut self, vk: u16, _ctx: &mut Ctx) -> bool {
        match vk {
            0x08 | 0x1B if !self.path.is_empty() => {
                self.path.pop();
                self.list_scroll = 0;
                true
            }
            0x74 if self.scanning.is_none() => {
                self.start(self.root.clone());
                true
            }
            _ => false,
        }
    }

    fn wheel(&mut self, delta: f32, _at: (f32, f32), _ctx: &mut Ctx) {
        let n = (delta.abs().ceil() as usize).max(1) * 3;
        self.list_scroll = if delta > 0.0 { self.list_scroll.saturating_sub(n) } else { self.list_scroll + n };
    }

    fn animating(&self) -> bool {
        self.scanning.is_some()
    }
}

pub fn run(target: Option<String>) -> Result<()> {
    let d = view::run(Disk::new(target), Opts { title: t!("CarlUI Disk analizörü", "CarlUI Disk analyzer").into(), w: 1180.0, h: 760.0, at: None, resizable: true, popup: false })?;
    if let Some(s) = &d.scanning {
        s.prog.cancel.store(true, Ordering::Relaxed);
    }
    Ok(())
}
