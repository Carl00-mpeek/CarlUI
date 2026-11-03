//! Bu modül adadaki aramanın "beynini" çözüyor: yazılan metni, imleci, seçimi,
//! kaydırmayı ve eylem listesini tutar; tuşları ve tıklamaları sonuçlara çevirir.
//! Arama mantığının kendisi (`carlui-launcher`) burada çağrılır; çizim
//! `island/spot.rs`'de yapılır. Uygulama/dosya ikonları ayrı bir iş
//! parçacığında yüklenir ki yazarken arayüz hiç takılmasın.

use crate::island::{SpotMode, SpotView, ROWS_MAX};
use carlui_core::config::Config;
use carlui_launcher::{
    actions,
    currency::Rates,
    files::Files,
    index::{self, Index, Running},
    search::{self, Act, Item, Sys},
    usage::Usage,
};
use std::sync::{mpsc, Arc, Mutex};
use windows::Win32::{
    Foundation::*,
    Graphics::Imaging::*,
    System::{
        Com::*,
        DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard},
        Memory::{GlobalLock, GlobalUnlock},
    },
};

const CF_UNICODETEXT: u32 = 13;

/// Yükleyiciden gelen ikon: (yol, istenen px, genişlik, yükseklik, PBGRA pikseller).
pub type IconPx = (String, u32, u32, u32, Vec<u8>);

pub enum Key {
    Esc,
    Enter { ctrl: bool, shift: bool },
    Tab { shift: bool },
    /// Alt tek başına basılıp bırakıldı: eylemler (eski Tab işlevi).
    Alt,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home { ctrl: bool },
    End { ctrl: bool },
    Back { ctrl: bool },
    Delete,
    SelectAll,
    Paste,
    Copy,
}

/// Tuşun sonucu: ne yapılmalı?
#[derive(PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    Redraw,
    /// Kapat; `true` ise önceki pencereye odak geri verilir.
    Close(bool),
    /// Aramayı kapat ve adada zamanlayıcıyı kur/durdur.
    Timer(crate::timing::Cmd),
    /// Aramayı kapat ve adada indir (None: süren indirmeleri iptal et).
    Download(Option<String>),
}

pub struct Launcher {
    pub open: bool,
    query: Vec<char>,
    /// `query`'nin metin hali (çizim için).
    text: String,
    caret: usize,
    all_selected: bool,
    mode: SpotMode,
    items: Vec<Item>,
    tiles: Vec<Item>,
    chips: Vec<Item>,
    sel: usize,
    scroll: usize,
    confirm: Option<usize>,
    all_apps: bool,
    /// Eylemlere geçmeden önceki liste (geri dönmek için).
    saved: Option<(Vec<Item>, usize, usize)>,
    pub prev_fg: HWND,
    index: Index,
    usage: Usage,
    files: Files,
    rates: Rates,
    run: Vec<Running>,
    wake: Arc<dyn Fn() + Send + Sync>,
    icon_tx: mpsc::Sender<(String, u32)>,
    high_surrogate: Option<u16>,
}

fn icon_thread(rx: mpsc::Receiver<(String, u32)>, out: Arc<Mutex<Vec<IconPx>>>, done: impl Fn() + Send + 'static) {
    std::thread::Builder::new()
        .name("spot-icons".into())
        .spawn(move || unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let Ok(wic) = CoCreateInstance::<_, IWICImagingFactory>(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) else { return };
            while let Ok(first) = rx.recv() {
                // Bir seferde gelenleri topla, hepsini yükleyip tek mesajla bildir.
                let mut batch = vec![first];
                batch.extend(rx.try_iter());
                for (path, px) in batch {
                    let px_data = carlui_ui::icons::shell_icon(&wic, &path, px as i32).ok().and_then(|conv| {
                        let (mut w, mut h) = (0, 0);
                        conv.GetSize(&mut w, &mut h).ok()?;
                        let mut buf = vec![0u8; (w * h * 4) as usize];
                        conv.CopyPixels(std::ptr::null(), w * 4, &mut buf).ok()?;
                        Some((w, h, buf))
                    });
                    let (w, h, buf) = px_data.unwrap_or((0, 0, Vec::new()));
                    if let Ok(mut o) = out.lock() {
                        o.push((path, px, w, h, buf));
                    }
                }
                done();
            }
        })
        .ok();
}

fn clipboard_text() -> Option<String> {
    unsafe {
        OpenClipboard(None).ok()?;
        let r = (|| {
            let h = GetClipboardData(CF_UNICODETEXT).ok()?;
            let g = windows::Win32::Foundation::HGLOBAL(h.0);
            let p = GlobalLock(g) as *const u16;
            if p.is_null() {
                return None;
            }
            let mut n = 0;
            while *p.add(n) != 0 {
                n += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
            let _ = GlobalUnlock(g);
            Some(s)
        })();
        let _ = CloseClipboard();
        r
    }
}

impl Launcher {
    /// `wake`: arka plan sonucu hazır (uygulama listesi, kur, dosya dizini).
    /// `icons_done`: ikon yüklendi.
    pub fn new(wake: impl Fn() + Send + Sync + 'static, icons_out: Arc<Mutex<Vec<IconPx>>>, icons_done: impl Fn() + Send + 'static) -> Self {
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let w2 = wake.clone();
        let index = Index::start(move || w2());
        let (tx, rx) = mpsc::channel();
        icon_thread(rx, icons_out, icons_done);
        Self {
            open: false,
            query: Vec::new(),
            text: String::new(),
            caret: 0,
            all_selected: false,
            mode: SpotMode::Home,
            items: Vec::new(),
            tiles: Vec::new(),
            chips: Vec::new(),
            sel: 0,
            scroll: 0,
            confirm: None,
            all_apps: false,
            saved: None,
            prev_fg: HWND::default(),
            index,
            usage: Usage::load(),
            files: Files::default(),
            rates: Rates::default(),
            run: Vec::new(),
            wake,
            icon_tx: tx,
            high_surrogate: None,
        }
    }

    pub fn request_icons(&self, list: Vec<(String, u32)>) {
        for r in list {
            let _ = self.icon_tx.send(r);
        }
    }

    pub fn open(&mut self, cfg: &Config, prev_fg: HWND) {
        self.open = true;
        self.prev_fg = prev_fg;
        self.query.clear();
        self.caret = 0;
        self.all_selected = false;
        self.all_apps = false;
        self.saved = None;
        self.run = index::running();
        self.index.refresh_if_stale(self.wake.clone());
        let apps = self.index.snapshot();
        self.usage.import_legacy(&apps);
        if cfg.launcher.file_search {
            let w = self.wake.clone();
            self.files.ensure(move || w());
        }
        self.rebuild(cfg, true);
    }

    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.items.clear();
        self.saved = None;
        self.confirm = None;
    }

    fn query_string(&self) -> String {
        self.query.iter().collect()
    }

    /// Sonuçları yeniden hesaplar. `reset`: seçimi başa al.
    pub fn rebuild(&mut self, cfg: &Config, reset: bool) {
        if self.mode == SpotMode::Actions && !reset {
            return;
        }
        let apps = self.index.snapshot();
        let q = self.query_string();
        self.text = q.clone();
        let keep = (!reset).then(|| self.selected().map(|i| (i.title.clone(), i.tag))).flatten();
        self.saved = None;
        if q.trim().is_empty() && !self.all_apps {
            self.mode = SpotMode::Home;
            let (t, c) = search::home(&apps, &self.run, &self.usage.counts);
            self.tiles = t;
            self.chips = c;
            self.items.clear();
        } else {
            self.mode = SpotMode::List;
            self.items = if q.trim().is_empty() {
                search::all_apps(&apps, &self.run)
            } else {
                self.all_apps = false;
                let cx = search::Ctx { apps: &apps, run: &self.run, usage: &self.usage.counts, files: &self.files, rates: &self.rates, cfg: &cfg.launcher, wake: self.wake.clone() };
                search::build(&q, &cx)
            };
        }
        let count = self.count();
        let kept = keep.and_then(|(t, tag)| self.list().iter().position(|i| i.title == t && i.tag == tag));
        match kept {
            Some(i) => self.sel = i,
            None => {
                self.sel = 0;
                self.scroll = 0;
            }
        }
        if count == 0 {
            self.sel = 0;
        }
        self.confirm = None;
        self.fit_scroll();
    }

    fn list(&self) -> Vec<&Item> {
        match self.mode {
            SpotMode::Home => self.tiles.iter().chain(self.chips.iter()).collect(),
            _ => self.items.iter().collect(),
        }
    }

    fn count(&self) -> usize {
        match self.mode {
            SpotMode::Home => self.tiles.len() + self.chips.len(),
            _ => self.items.len(),
        }
    }

    fn selected(&self) -> Option<&Item> {
        match self.mode {
            SpotMode::Home if self.sel < self.tiles.len() => self.tiles.get(self.sel),
            SpotMode::Home => self.chips.get(self.sel - self.tiles.len()),
            _ => self.items.get(self.sel),
        }
    }

    fn fit_scroll(&mut self) {
        if self.mode == SpotMode::Home {
            self.scroll = 0;
            return;
        }
        let n = self.items.len();
        if self.sel < self.scroll {
            self.scroll = self.sel;
        } else if self.sel >= self.scroll + ROWS_MAX {
            self.scroll = self.sel + 1 - ROWS_MAX;
        }
        self.scroll = self.scroll.min(n.saturating_sub(ROWS_MAX));
    }

    pub fn view(&self) -> SpotView<'_> {
        let hint = match self.mode {
            SpotMode::Home => t!("Ok tuşları ile seç · Enter aç · yt, chatgpt, wp … kısayollar için @ · Esc kapat", "Arrows to select · Enter to open · @ for yt, chatgpt, wp … shortcuts · Esc to close"),
            SpotMode::List => t!("Enter aç · Tab tamamla · Alt eylemler · Ctrl+Enter konumu aç · Shift+Enter yönetici · Esc temizle", "Enter open · Tab complete · Alt actions · Ctrl+Enter open location · Shift+Enter admin · Esc clear"),
            SpotMode::Actions => t!("Enter uygula · Esc ya da Alt geri", "Enter apply · Esc or Alt back"),
        };
        SpotView {
            query: &self.text,
            caret: self.caret,
            all_selected: self.all_selected,
            mode: self.mode,
            items: &self.items,
            tiles: &self.tiles,
            chips: &self.chips,
            sel: self.sel,
            scroll: self.scroll,
            confirm: self.confirm,
            hint,
            placeholder: if self.all_apps { t!("Tüm uygulamalar içinde ara", "Search all apps") } else { t!("Uygulama, dosya veya web'de ara", "Search apps, files or the web") },
        }
    }

    /// Test dökümü (island-state.json).
    pub fn debug(&self) -> serde_json::Value {
        let t = |v: &[Item]| v.iter().take(12).map(|i| format!("[{}] {} — {}", i.tag, i.title, i.sub)).collect::<Vec<_>>();
        serde_json::json!({
            "open": self.open,
            "query": self.text,
            "caret": self.caret,
            "mode": format!("{:?}", self.mode),
            "sel": self.sel,
            "scroll": self.scroll,
            "confirm": self.confirm,
            "count": self.count(),
            "items": t(&self.items),
            "tiles": t(&self.tiles),
            "chips": t(&self.chips),
            "apps": self.index.snapshot().len(),
        })
    }

    // ---- Metin düzenleme ----

    fn edited(&mut self, cfg: &Config) -> Outcome {
        self.all_selected = false;
        self.rebuild(cfg, true);
        Outcome::Redraw
    }

    fn insert(&mut self, s: &str) {
        if self.all_selected {
            self.query.clear();
            self.caret = 0;
        }
        for c in s.chars().filter(|c| !c.is_control()) {
            self.query.insert(self.caret, c);
            self.caret += 1;
        }
    }

    /// WM_CHAR: UTF-16 birimi.
    pub fn char(&mut self, cfg: &Config, unit: u16) -> Outcome {
        if (0xD800..0xDC00).contains(&unit) {
            self.high_surrogate = Some(unit);
            return Outcome::Nothing;
        }
        let s = match self.high_surrogate.take() {
            Some(hi) if (0xDC00..0xE000).contains(&unit) => String::from_utf16_lossy(&[hi, unit]),
            _ => String::from_utf16_lossy(&[unit]),
        };
        if s.chars().all(|c| c.is_control()) {
            return Outcome::Nothing;
        }
        if self.mode == SpotMode::Actions {
            self.mode = SpotMode::List;
        }
        self.insert(&s);
        self.edited(cfg)
    }

    // ---- Tuşlar ----

    pub fn key(&mut self, cfg: &Config, k: Key) -> Outcome {
        let n = self.count();
        let has_text = !self.query.is_empty();
        match k {
            Key::Esc => {
                if self.confirm.take().is_some() {
                    return Outcome::Redraw;
                }
                if self.mode == SpotMode::Actions {
                    self.back();
                    return Outcome::Redraw;
                }
                if has_text || self.all_apps {
                    self.query.clear();
                    self.caret = 0;
                    self.all_apps = false;
                    return self.edited(cfg);
                }
                Outcome::Close(true)
            }
            Key::Enter { ctrl, shift } => self.activate(cfg, self.sel, ctrl, shift),
            Key::Alt | Key::Tab { shift: true } if self.mode == SpotMode::Actions => {
                self.back();
                Outcome::Redraw
            }
            Key::Alt => self.open_actions(self.sel),
            Key::Tab { shift: true } => Outcome::Nothing,
            // Tab: seçili önerinin adıyla yazıyı tamamla ("yt" → "yt ", "chr" → "Google Chrome").
            Key::Tab { .. } => {
                if self.mode == SpotMode::Actions {
                    self.back();
                    return Outcome::Redraw;
                }
                let Some(text) = self.selected().and_then(|i| i.completion()) else { return Outcome::Nothing };
                if text == self.query_string() {
                    return Outcome::Nothing;
                }
                self.query = text.chars().collect();
                self.caret = self.query.len();
                self.all_apps = false;
                self.edited(cfg)
            }
            Key::Up | Key::Down | Key::Left | Key::Right if self.mode == SpotMode::Home => {
                let tiles = self.tiles.len();
                let s = self.sel;
                self.sel = match k {
                    Key::Left => s.saturating_sub(1),
                    Key::Right => (s + 1).min(n.saturating_sub(1)),
                    Key::Down if s < tiles => tiles.min(n.saturating_sub(1)),
                    Key::Up if s >= tiles => s.saturating_sub(tiles).min(tiles.saturating_sub(1)),
                    _ => s,
                };
                self.confirm = None;
                Outcome::Redraw
            }
            Key::Left => {
                self.all_selected = false;
                self.caret = self.caret.saturating_sub(1);
                Outcome::Redraw
            }
            Key::Right => {
                self.all_selected = false;
                self.caret = (self.caret + 1).min(self.query.len());
                Outcome::Redraw
            }
            Key::Up | Key::Down | Key::PageUp | Key::PageDown | Key::Home { ctrl: true } | Key::End { ctrl: true } => {
                if n == 0 {
                    return Outcome::Nothing;
                }
                self.sel = match k {
                    Key::Up => if self.sel == 0 { n - 1 } else { self.sel - 1 },
                    Key::Down => if self.sel + 1 >= n { 0 } else { self.sel + 1 },
                    Key::PageUp => self.sel.saturating_sub(ROWS_MAX),
                    Key::PageDown => (self.sel + ROWS_MAX).min(n - 1),
                    Key::Home { .. } => 0,
                    _ => n - 1,
                };
                self.confirm = None;
                self.fit_scroll();
                Outcome::Redraw
            }
            Key::Home { .. } => {
                self.all_selected = false;
                self.caret = 0;
                Outcome::Redraw
            }
            Key::End { .. } => {
                self.all_selected = false;
                self.caret = self.query.len();
                Outcome::Redraw
            }
            Key::Back { ctrl } => {
                if !has_text {
                    if self.all_apps {
                        self.all_apps = false;
                        return self.edited(cfg);
                    }
                    return Outcome::Nothing;
                }
                if self.all_selected {
                    self.query.clear();
                    self.caret = 0;
                } else if ctrl {
                    // Kelimeyi sil: önce boşlukları, sonra kelimeyi.
                    let mut i = self.caret;
                    while i > 0 && self.query[i - 1] == ' ' {
                        i -= 1;
                    }
                    while i > 0 && self.query[i - 1] != ' ' {
                        i -= 1;
                    }
                    self.query.drain(i..self.caret);
                    self.caret = i;
                } else if self.caret > 0 {
                    self.caret -= 1;
                    self.query.remove(self.caret);
                }
                self.edited(cfg)
            }
            Key::Delete => {
                if self.all_selected {
                    self.query.clear();
                    self.caret = 0;
                } else if self.caret < self.query.len() {
                    self.query.remove(self.caret);
                } else {
                    return Outcome::Nothing;
                }
                self.edited(cfg)
            }
            Key::SelectAll => {
                if !has_text {
                    return Outcome::Nothing;
                }
                self.all_selected = true;
                self.caret = self.query.len();
                Outcome::Redraw
            }
            Key::Paste => {
                let Some(t) = clipboard_text() else { return Outcome::Nothing };
                let line = t.lines().next().unwrap_or("").trim().to_string();
                if line.is_empty() {
                    return Outcome::Nothing;
                }
                self.insert(&line);
                self.edited(cfg)
            }
            Key::Copy => {
                if has_text {
                    actions::copy_text(&self.query_string());
                }
                Outcome::Nothing
            }
        }
    }

    fn back(&mut self) {
        if let Some((items, sel, scroll)) = self.saved.take() {
            self.items = items;
            self.sel = sel;
            self.scroll = scroll;
        }
        self.mode = if self.items.is_empty() && !self.all_apps && self.query.is_empty() { SpotMode::Home } else { SpotMode::List };
        self.confirm = None;
    }

    pub fn open_actions(&mut self, i: usize) -> Outcome {
        if self.mode == SpotMode::Actions {
            return Outcome::Nothing;
        }
        self.sel = i.min(self.count().saturating_sub(1));
        let Some(it) = self.selected().cloned() else { return Outcome::Nothing };
        if matches!(it.act, Act::AllApps | Act::Pending | Act::None | Act::System(_)) {
            return Outcome::Nothing;
        }
        let acts = search::actions(&it);
        let was_home = self.mode == SpotMode::Home;
        let items = std::mem::replace(&mut self.items, acts);
        self.saved = Some((if was_home { Vec::new() } else { items }, self.sel, self.scroll));
        self.mode = SpotMode::Actions;
        self.sel = 0;
        self.scroll = 0;
        self.confirm = None;
        Outcome::Redraw
    }

    /// Fareyle üstüne gelinen satır/kutucuk.
    pub fn hover(&mut self, i: usize) -> bool {
        if i < self.count() && i != self.sel {
            self.sel = i;
            self.confirm = None;
            return true;
        }
        false
    }

    pub fn wheel(&mut self, rows: i32) -> bool {
        if self.mode == SpotMode::Home || self.items.len() <= ROWS_MAX {
            return false;
        }
        let max = self.items.len() - ROWS_MAX;
        let ns = (self.scroll as i32 - rows).clamp(0, max as i32) as usize;
        if ns == self.scroll {
            return false;
        }
        self.scroll = ns;
        self.sel = self.sel.clamp(ns, ns + ROWS_MAX - 1);
        true
    }

    pub fn activate(&mut self, cfg: &Config, i: usize, ctrl: bool, shift: bool) -> Outcome {
        self.sel = i.min(self.count().saturating_sub(1));
        let Some(it) = self.selected().cloned() else { return Outcome::Nothing };
        let act = match (&it.act, ctrl, shift) {
            (_, true, _) if !it.path.is_empty() => Act::Reveal(it.path.clone()),
            (_, _, true) if it.path.to_lowercase().ends_with(".exe") => Act::Admin(it.path.clone()),
            (a, ..) => a.clone(),
        };
        match &act {
            Act::AllApps => {
                self.all_apps = true;
                self.query.clear();
                self.caret = 0;
                self.rebuild(cfg, true);
                return Outcome::Redraw;
            }
            Act::Pending | Act::None => return Outcome::Nothing,
            Act::System(Sys::Restart | Sys::Shutdown) | Act::Restart { .. } if self.confirm != Some(self.sel) => {
                self.confirm = Some(self.sel);
                return Outcome::Redraw;
            }
            _ => {}
        }
        self.usage.bump(&it.key);
        carlui_core::info!("arama: {} ({})", it.title, it.tag);
        match act {
            Act::Timer { secs, label } => return Outcome::Timer(crate::timing::Cmd::Start(secs, label)),
            Act::Stopwatch => return Outcome::Timer(crate::timing::Cmd::Stopwatch),
            Act::TimerStop => return Outcome::Timer(crate::timing::Cmd::Stop),
            Act::Download(url) => return Outcome::Download(Some(url)),
            Act::DownloadCancel => return Outcome::Download(None),
            _ => {}
        }
        let restore = matches!(act, Act::Copy(_));
        actions::run(&act);
        Outcome::Close(restore)
    }
}


