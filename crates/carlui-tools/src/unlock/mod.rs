//! Bu modül "dosya kullanımda, silinemiyor ama kim kullanıyor?" sorununu
//! çözüyor. Gezgin'de seçilen öğeler (kısayol `shortcuts.unlock`), komut
//! satırından verilen yollar (Tam Kaldırıcı'nın silemedikleri) ya da pencereden
//! seçilen bir dosya/klasör için onu tutan uygulamaları listeler. "Kapat"
//! uygulamaya kibarca kapanmasını söyler; kapanmazsa "Zorla" onayla çalışır.
//! Liste üç saniyede bir yenilenir; dosya serbest kalınca pencere bunu söyler.

pub mod holders;

use crate::view::{self, Ctx, Opts, View, WM_APP_TICK};
use carlui_ui::paint::{Align, Painter, GREEN, ORANGE, R, TEXT};
use holders::{Holder, KillError, Kind};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::Com::*,
        UI::{Shell::*, WindowsAndMessaging::*},
    },
};

const PAD: f32 = 18.0;
const ROW: f32 = 58.0;
const ROWS: usize = 4;
const REFRESH: Duration = Duration::from_secs(3);

#[derive(Clone, Debug)]
enum A {
    Nothing,
    PickFile,
    PickFolder,
    Refresh,
    Close(usize),
    Force(usize),
    Yes(usize),
    No,
    Elevate,
    Exit,
}

struct Scan {
    holders: std::result::Result<Vec<Holder>, String>,
    files: usize,
    full: bool,
}

struct Unlock {
    paths: Vec<PathBuf>,
    /// Son tarama; ilk sonuç gelene kadar None.
    scan: Option<Scan>,
    busy: bool,
    last: Instant,
    /// Zorla kapatma onayı bekleyen süreç.
    confirm: Option<u32>,
    message: Option<String>,
    denied: bool,
    scroll: usize,
    inbox: Arc<Mutex<Option<Scan>>>,
    stop: Arc<AtomicBool>,
    hwnd: isize,
}

/// Uzun yolu ortasından kısaltır: dosya adı görünür kalsın.
fn shorten(p: &Painter<A>, s: &str, size: f32, max: f32) -> String {
    if p.fonts.width(s, size, false) <= max {
        return s.to_string();
    }
    let c: Vec<char> = s.chars().collect();
    let mut keep = c.len();
    while keep > 8 {
        keep -= 1;
        let head = keep / 3;
        let tail = keep - head;
        let t: String = c[..head].iter().chain(['…'].iter()).chain(c[c.len() - tail..].iter()).collect();
        if p.fonts.width(&t, size, false) <= max {
            return t;
        }
    }
    "…".into()
}

/// Dosya ya da klasör seçtirir (Windows'un kendi penceresi).
fn pick(folder: bool, owner: HWND) -> Option<PathBuf> {
    unsafe {
        let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let o = FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | if folder { FOS_PICKFOLDERS } else { FOS_FILEMUSTEXIST };
        let _ = dlg.SetOptions(o);
        dlg.Show(owner).ok()?;
        let p = dlg.GetResult().ok()?.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        s.map(PathBuf::from)
    }
}

impl Unlock {
    fn new(paths: Vec<PathBuf>) -> Self {
        Self { paths, scan: None, busy: false, last: Instant::now(), confirm: None, message: None, denied: false, scroll: 0, inbox: Arc::new(Mutex::new(None)), stop: Arc::new(AtomicBool::new(false)), hwnd: 0 }
    }

    fn rescan(&mut self) {
        if self.busy || self.paths.is_empty() || self.hwnd == 0 {
            return;
        }
        self.busy = true;
        self.last = Instant::now();
        let (paths, inbox, stop, hwnd) = (self.paths.clone(), self.inbox.clone(), self.stop.clone(), self.hwnd);
        std::thread::spawn(move || {
            let (files, full) = holders::expand(&paths, &stop);
            let holders = holders::find(&files);
            *inbox.lock().unwrap_or_else(|e| e.into_inner()) = Some(Scan { holders, files: files.len(), full });
            unsafe {
                let _ = PostMessageW(HWND(hwnd as _), WM_APP_TICK, WPARAM(0), LPARAM(0));
            }
        });
    }

    fn holders(&self) -> &[Holder] {
        match &self.scan {
            Some(Scan { holders: Ok(h), .. }) => h,
            _ => &[],
        }
    }

    fn choose(&mut self, path: PathBuf) {
        self.paths = vec![path];
        self.scan = None;
        self.message = None;
        self.denied = false;
        self.confirm = None;
        self.scroll = 0;
        self.rescan();
    }

    /// Aynı öğelerle yönetici olarak yeniden açar; onayı Windows sorar.
    fn elevate(&self, ctx: &mut Ctx) {
        let Ok(exe) = std::env::current_exe() else { return };
        let mut args = String::from("--unlock");
        for p in &self.paths {
            args.push_str(&format!(" \"{}\"", p.display()));
        }
        let r = unsafe { ShellExecuteW(HWND::default(), w!("runas"), &HSTRING::from(exe.as_os_str()), &HSTRING::from(args), PCWSTR::null(), SW_SHOWNORMAL) };
        if r.0 as isize > 32 {
            ctx.close = true;
        }
    }

    fn title(&self) -> String {
        match self.paths.as_slice() {
            [] => t!("Dosya ya da klasör seçin", "Choose a file or folder").to_string(),
            [one] => one.display().to_string(),
            many => t!(format!("{} öğe seçili", many.len()), format!("{} items selected", many.len())),
        }
    }

    fn paint_row(&self, p: &mut Painter<A>, i: usize, h: &Holder, r: R) {
        p.card(r);
        let glyph = match h.kind {
            Kind::App => '\u{ECAA}',
            Kind::Explorer => '\u{E8B7}',
            Kind::Service | Kind::Critical => '\u{E912}',
        };
        p.glyph(glyph, R::new(r.x + 8.0, r.y, 34.0, r.h), 16.0, TEXT, 0.8);
        let confirm = self.confirm == Some(h.pid);
        let right = if confirm { 214.0 } else { 176.0 };
        let tw = r.w - 48.0 - right;
        let name = shorten(p, &h.name, 13.5, tw);
        p.text(&name, R::new(r.x + 48.0, r.y + 9.0, tw, 20.0), 13.5, true, TEXT, 0.95, Align::Left);
        let sub = if h.exe.is_empty() { format!("PID {}", h.pid) } else { format!("PID {} · {}", h.pid, h.exe) };
        let sub = shorten(p, &sub, 11.5, tw);
        p.text(&sub, R::new(r.x + 48.0, r.y + 30.0, tw, 18.0), 11.5, false, TEXT, 0.6, Align::Left);
        let y = r.y + (r.h - 32.0) / 2.0;
        let end = r.right() - 12.0;
        if confirm {
            p.text(t!("Kaydedilmemiş iş gider", "Unsaved work is lost"), R::new(end - 200.0, r.y + 4.0, 200.0, 16.0), 11.0, false, ORANGE, 1.0, Align::Right);
            p.button(R::new(end - 196.0, r.y + 24.0, 112.0, 28.0), None, t!("Zorla kapat", "Force close"), A::Yes(i), true, true);
            p.button(R::new(end - 78.0, r.y + 24.0, 78.0, 28.0), None, t!("Vazgeç", "Cancel"), A::No, true, false);
        } else if h.kind == Kind::Explorer {
            p.button(R::new(end - 148.0, y, 148.0, 32.0), None, t!("Klasörleri kapat", "Close folders"), A::Close(i), true, false);
        } else if h.killable() {
            p.button(R::new(end - 164.0, y, 84.0, 32.0), None, t!("Kapat", "Close"), A::Close(i), true, false);
            p.button(R::new(end - 74.0, y, 74.0, 32.0), None, t!("Zorla", "Force"), A::Force(i), true, true);
        } else {
            let s = if h.kind == Kind::Service { t!("Windows hizmeti", "Windows service") } else { t!("Sistem süreci", "System process") };
            p.text(s, R::new(end - 150.0, r.y, 150.0, r.h), 12.0, false, TEXT, 0.6, Align::Right);
        }
    }
}

impl View for Unlock {
    type A = A;

    fn opened(&mut self, hwnd: HWND) {
        self.hwnd = hwnd.0 as isize;
        self.rescan();
    }

    fn paint(&mut self, p: &mut Painter<A>, w: f32, h: f32) {
        let x = PAD;
        let cw = w - PAD * 2.0;
        p.hit(R::new(0.0, 0.0, w, h), A::Nothing);
        p.glyph('\u{E785}', R::new(x, 14.0, 26.0, 30.0), 18.0, p.accent, 1.0);
        p.text(t!("Dosya kilidi açıcı", "File unlocker"), R::new(x + 32.0, 14.0, cw - 32.0, 30.0), 17.0, true, TEXT, 0.95, Align::Left);
        let title = shorten(p, &self.title(), 12.5, cw);
        p.text(&title, R::new(x, 48.0, cw, 20.0), 12.5, false, TEXT, 0.7, Align::Left);

        let body = R::new(x, 78.0, cw, ROW * ROWS as f32 + 6.0 * (ROWS as f32 - 1.0));
        let mid = R::new(body.x, body.y + body.h / 2.0 - 50.0, body.w, 34.0);
        let under = R::new(body.x, mid.y + 40.0, body.w, 24.0);
        match &self.scan {
            _ if self.paths.is_empty() => {
                p.glyph('\u{E8E5}', mid, 26.0, TEXT, 0.5);
                p.text(t!("Aşağıdan dosya ya da klasör seçin.", "Choose a file or folder below."), under, 13.5, false, TEXT, 0.75, Align::Center);
            }
            None => p.text(t!("Bakılıyor…", "Checking…"), under, 13.5, false, TEXT, 0.75, Align::Center),
            Some(Scan { holders: Err(e), .. }) => {
                p.glyph('\u{E7BA}', mid, 26.0, ORANGE, 1.0);
                let s = t!(format!("Windows yanıt vermedi: {e}"), format!("Windows didn't answer: {e}"));
                p.text(&s, under, 13.0, false, TEXT, 0.8, Align::Center);
            }
            Some(Scan { holders: Ok(list), files, .. }) if list.is_empty() => {
                p.glyph('\u{E73E}', mid, 26.0, GREEN, 1.0);
                let s = if *files > 1 { t!(format!("{files} dosyayı tutan uygulama yok"), format!("Nothing is holding these {files} files")) } else { t!("Bu dosyayı tutan uygulama yok", "Nothing is holding this file").to_string() };
                p.text(&s, under, 14.0, true, TEXT, 0.9, Align::Center);
            }
            Some(_) => {
                let list = self.holders().to_vec();
                self.scroll = self.scroll.min(list.len().saturating_sub(ROWS));
                for (k, (i, hd)) in list.iter().enumerate().skip(self.scroll).take(ROWS).enumerate() {
                    self.paint_row(p, i, hd, R::new(body.x, body.y + k as f32 * (ROW + 6.0), body.w, ROW));
                }
            }
        }

        let note = match (&self.message, &self.scan) {
            (Some(m), _) => Some((m.clone(), ORANGE)),
            (None, Some(Scan { full: true, .. })) => Some((t!(format!("Klasör çok büyük: ilk {} dosyaya bakıldı.", holders::MAX_FILES), format!("Folder is huge: checked the first {} files.", holders::MAX_FILES)), ORANGE)),
            (None, Some(Scan { holders: Ok(l), .. })) if l.len() > ROWS => Some((t!(format!("{} uygulama · tekerlekle kaydırın", l.len()), format!("{} apps · scroll with the wheel", l.len())), TEXT)),
            _ => None,
        };
        if let Some((m, c)) = note {
            p.text(&m, R::new(x, h - 88.0, cw, 20.0), 12.0, false, c, 0.95, Align::Left);
        }
        let b = h - 58.0;
        p.button(R::new(x, b, 112.0, 38.0), Some('\u{E8A5}'), t!("Dosya…", "File…"), A::PickFile, true, false);
        p.button(R::new(x + 120.0, b, 112.0, 38.0), Some('\u{E8B7}'), t!("Klasör…", "Folder…"), A::PickFolder, true, false);
        p.button(R::new(x + 240.0, b, 44.0, 38.0), Some('\u{E72C}'), "", A::Refresh, !self.paths.is_empty(), false);
        if self.denied {
            p.button(R::new(x + cw - 92.0 - 8.0 - 156.0, b, 156.0, 38.0), Some('\u{EA18}'), t!("Yönetici olarak", "As administrator"), A::Elevate, true, false);
        }
        p.button(R::new(x + cw - 92.0, b, 92.0, 38.0), None, t!("Kapat", "Close"), A::Exit, true, false);
    }

    fn click(&mut self, a: A, ctx: &mut Ctx) {
        let holder = |s: &Self, i: usize| s.holders().get(i).cloned();
        match a {
            A::Nothing => {}
            A::PickFile | A::PickFolder => {
                if let Some(p) = pick(matches!(a, A::PickFolder), ctx.hwnd) {
                    self.choose(p);
                }
            }
            A::Refresh => {
                self.message = None;
                self.rescan();
            }
            A::Close(i) => {
                let Some(h) = holder(self, i) else { return };
                self.confirm = None;
                self.message = if holders::close(&h) {
                    Some(t!(format!("{} uygulamasına kapanması söylendi.", h.name), format!("Asked {} to close.", h.name)))
                } else {
                    Some(t!("Açık penceresi yok; kapanmıyorsa Zorla'yı kullanın.", "It has no open window; use Force if it stays.").to_string())
                };
                // Uygulama kapanmaya vakit bulsun, liste bir saniye sonra yenilensin.
                self.last = Instant::now() - REFRESH + Duration::from_millis(1000);
            }
            A::Force(i) => self.confirm = holder(self, i).map(|h| h.pid),
            A::No => self.confirm = None,
            A::Yes(i) => {
                let Some(h) = holder(self, i) else { return };
                self.confirm = None;
                match holders::kill(&h) {
                    Ok(()) | Err(KillError::Gone) => self.message = None,
                    Err(KillError::Denied) => {
                        self.denied = true;
                        self.message = Some(t!("Bu uygulamayı kapatmak için yönetici yetkisi gerekiyor.", "Closing this app needs administrator rights.").to_string());
                    }
                    Err(KillError::Failed) => self.message = Some(t!("Windows uygulamayı kapatamadı.", "Windows couldn't close the app.").to_string()),
                }
                self.rescan();
            }
            A::Elevate => self.elevate(ctx),
            A::Exit => ctx.close = true,
        }
    }

    fn key(&mut self, vk: u16, ctx: &mut Ctx) -> bool {
        if vk == 0x1B {
            ctx.close = true;
            return true;
        }
        false
    }

    fn wheel(&mut self, delta: f32, _at: (f32, f32), _ctx: &mut Ctx) {
        let max = self.holders().len().saturating_sub(ROWS);
        self.scroll = if delta > 0.0 { self.scroll.saturating_sub(1) } else { (self.scroll + 1).min(max) };
    }

    fn tick(&mut self, _ctx: &mut Ctx) {
        let got = self.inbox.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(s) = got {
            self.busy = false;
            if matches!(&s.holders, Ok(l) if l.is_empty()) {
                self.message = None;
                self.denied = false;
            }
            if let (Some(pid), Ok(l)) = (self.confirm, &s.holders) {
                if !l.iter().any(|h| h.pid == pid) {
                    self.confirm = None;
                }
            }
            self.scan = Some(s);
        }
        if self.last.elapsed() >= REFRESH {
            self.rescan();
        }
    }
}

impl Drop for Unlock {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub fn run(paths: Vec<PathBuf>) -> Result<()> {
    view::run(Unlock::new(paths), Opts { title: t!("Dosya kilidi açıcı", "File unlocker").into(), w: 640.0, h: 410.0, timer_ms: 1000, ..Default::default() })?;
    Ok(())
}
