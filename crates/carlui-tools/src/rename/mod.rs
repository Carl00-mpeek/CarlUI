//! Bu modül "çok sayıda dosyayı tek tek yeniden adlandırmak" sorununu
//! çözüyor: Gezgin'de ya da masaüstünde dosyalar seçilip kısayola
//! (`shortcuts.rename`, varsayılan Ctrl+Shift+R) basılınca imlecin yanında
//! küçük bir pencere açılır. Üç kip vardır: bul/değiştir, şablon
//! (`{ad}`, `{n}`, `{tarih}`) ve harf büyüklüğü. Altta canlı önizleme ve
//! çakışma uyarısı görünür; çakışma varken Uygula kapalıdır. Uygulandıktan
//! sonra "Geri al" ile eski adlar döner. Son seçenekler ayara yazılır.

pub mod plan;

use crate::{
    field::Field,
    note,
    view::{self, Ctx, Opts, View},
};
use carlui_core::config::{self, CaseKind, RenameConfig, RenameMode};
use carlui_ui::paint::{Align, Painter, GREEN, ORANGE, R, RED, TEXT};
use plan::{Entry, Status};
use std::path::PathBuf;

#[derive(Clone, Debug)]
enum A {
    Mode(RenameMode),
    Case(CaseKind),
    Focus(usize),
    KeepExt,
    MatchCase,
    Start(i32),
    Apply,
    Undo,
    Close,
    Nothing,
}

struct Rename {
    paths: Vec<PathBuf>,
    cfg: RenameConfig,
    /// Kipe göre görünen alanlar: [bul, değiştir] ya da [şablon].
    fields: [Field; 3],
    focus: usize,
    plan: Vec<Entry>,
    date: String,
    scroll: usize,
    done: Option<Vec<(PathBuf, PathBuf)>>,
    error: Option<String>,
}

const ROW: f32 = 24.0;
const ROWS: usize = 8;

fn today() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay)
}

impl Rename {
    fn new(paths: Vec<PathBuf>, cfg: RenameConfig) -> Self {
        let fields = [Field::new(&cfg.find), Field::new(&cfg.replace), Field::new(&cfg.template)];
        let mut r = Self { paths, cfg, fields, focus: 0, plan: Vec::new(), date: today(), scroll: 0, done: None, error: None };
        r.focus = r.first_field();
        r.refresh();
        r
    }

    fn first_field(&self) -> usize {
        match self.cfg.mode {
            RenameMode::Replace => 0,
            RenameMode::Template => 2,
            RenameMode::Case => usize::MAX,
        }
    }

    fn visible_fields(&self) -> &'static [usize] {
        match self.cfg.mode {
            RenameMode::Replace => &[0, 1],
            RenameMode::Template => &[2],
            RenameMode::Case => &[],
        }
    }

    fn refresh(&mut self) {
        self.cfg.find = self.fields[0].text.clone();
        self.cfg.replace = self.fields[1].text.clone();
        self.cfg.template = self.fields[2].text.clone();
        self.plan = plan::plan(&self.paths, &self.cfg, &self.date, |p| p.exists());
        self.error = None;
    }

    fn apply(&mut self, ctx: &mut Ctx) {
        if self.done.is_some() || plan::blocked(&self.plan) > 0 {
            return;
        }
        let moves = plan::moves(&self.plan);
        if moves.is_empty() {
            return;
        }
        match plan::apply(&moves) {
            Ok(done) => {
                let n = done.len();
                carlui_core::info!("yeniden adlandırma: {n} öğe");
                let mut c = config::load();
                c.tools.rename = self.cfg.clone();
                config::save(&c);
                // Önizleme yeni adları göstersin.
                self.paths = self.paths.iter().map(|p| done.iter().find(|(a, _)| a == p).map(|(_, b)| b.clone()).unwrap_or_else(|| p.clone())).collect();
                self.done = Some(done);
                note::island(t!("Yeniden adlandır", "Rename"), &t!(format!("{n} öğe yeniden adlandırıldı"), format!("{n} items renamed")), "");
                let _ = ctx;
            }
            Err(e) => {
                carlui_core::warn!("yeniden adlandırma başarısız: {e}");
                self.error = Some(e);
            }
        }
    }

    fn undo(&mut self) {
        let Some(done) = self.done.take() else { return };
        match plan::undo(&done) {
            Ok(_) => {
                self.paths = self.paths.iter().map(|p| done.iter().find(|(_, b)| b == p).map(|(a, _)| a.clone()).unwrap_or_else(|| p.clone())).collect();
                self.refresh();
            }
            Err(e) => {
                self.error = Some(e);
                self.done = Some(done);
            }
        }
    }

    fn label_for(&self, i: usize) -> &'static str {
        match i {
            0 => t!("Bul", "Find"),
            1 => t!("Şununla değiştir", "Replace with"),
            _ => t!("Şablon", "Template"),
        }
    }
}

impl View for Rename {
    type A = A;

    fn paint(&mut self, p: &mut Painter<A>, w: f32, h: f32) {
        let pad = 18.0;
        let iw = w - pad * 2.0;
        p.hit(R::new(0.0, 0.0, w, h), A::Nothing);
        let title = t!(format!("Yeniden adlandır · {} öğe", self.paths.len()), format!("Rename · {} items", self.paths.len()));
        p.glyph('\u{E8AC}', R::new(pad, 14.0, 20.0, 24.0), 15.0, p.accent, 1.0);
        p.text(&title, R::new(pad + 28.0, 14.0, iw - 60.0, 24.0), 15.0, true, TEXT, 0.95, Align::Left);
        p.button(R::new(w - pad - 28.0, 12.0, 28.0, 28.0), Some('\u{E711}'), "", A::Close, true, false);

        let mut y = 50.0;
        let editing = self.done.is_none();
        let modes = [(t!("Bul / değiştir", "Find / replace"), A::Mode(RenameMode::Replace)), (t!("Şablon", "Template"), A::Mode(RenameMode::Template)), (t!("Harf", "Case"), A::Mode(RenameMode::Case))];
        let sel = match self.cfg.mode {
            RenameMode::Replace => 0,
            RenameMode::Template => 1,
            RenameMode::Case => 2,
        };
        p.segments(R::new(pad, y, iw, 32.0), &modes, sel);
        y += 44.0;

        for &i in self.visible_fields() {
            p.text(self.label_for(i), R::new(pad, y, 130.0, 32.0), 13.0, false, TEXT, 0.7, Align::Left);
            let hint = if i == 2 { "{ad} {n} {tarih}" } else { "" };
            self.fields[i].paint(p, R::new(pad + 130.0, y, iw - 130.0, 32.0), self.focus == i && editing, hint, A::Focus(i));
            y += 40.0;
        }
        match self.cfg.mode {
            RenameMode::Template => {
                p.text(t!("Başlangıç no.", "Start number"), R::new(pad, y, 130.0, 30.0), 13.0, false, TEXT, 0.7, Align::Left);
                p.button(R::new(pad + 130.0, y, 30.0, 30.0), Some('\u{E738}'), "", A::Start(-1), self.cfg.start > 0, false);
                p.text(&self.cfg.start.to_string(), R::new(pad + 164.0, y, 50.0, 30.0), 14.0, true, TEXT, 0.95, Align::Center);
                p.button(R::new(pad + 218.0, y, 30.0, 30.0), Some('\u{E710}'), "", A::Start(1), true, false);
                p.text(t!("{ad}: eski ad · {n}: sıra · {tarih}: bugün", "{name}: old name · {n}: number · {date}: today"), R::new(pad + 262.0, y, iw - 262.0, 30.0), 11.5, false, TEXT, 0.5, Align::Left);
                y += 40.0;
            }
            RenameMode::Case => {
                let cases = [(t!("küçük harf", "lowercase"), A::Case(CaseKind::Lower)), (t!("BÜYÜK HARF", "UPPERCASE"), A::Case(CaseKind::Upper)), (t!("Baş Harfler", "Title Case"), A::Case(CaseKind::Title))];
                let sel = match self.cfg.case {
                    CaseKind::Lower => 0,
                    CaseKind::Upper => 1,
                    CaseKind::Title => 2,
                };
                p.segments(R::new(pad, y, iw, 30.0), &cases, sel);
                y += 40.0;
            }
            RenameMode::Replace => {}
        }
        let mut x = pad;
        x += p.toggle(x, y, t!("Uzantıyı koru", "Keep extension"), self.cfg.keep_ext, A::KeepExt) + 22.0;
        if self.cfg.mode == RenameMode::Replace {
            p.toggle(x, y, t!("Büyük/küçük harf duyarlı", "Match case"), self.cfg.match_case, A::MatchCase);
        }
        y += 38.0;

        // Önizleme.
        let list = R::new(pad, y, iw, ROW * ROWS as f32 + 8.0);
        p.round(list, 8.0, 0x000000, 0.18);
        let max_scroll = self.plan.len().saturating_sub(ROWS);
        self.scroll = self.scroll.min(max_scroll);
        let half = (iw - 40.0) / 2.0;
        for (k, e) in self.plan.iter().skip(self.scroll).take(ROWS).enumerate() {
            let ry = y + 4.0 + k as f32 * ROW;
            let (c, a) = match e.status {
                Status::Same => (TEXT, 0.45),
                Status::Ok => (TEXT, 0.92),
                _ => (RED, 0.95),
            };
            p.text(&e.name(), R::new(pad + 10.0, ry, half - 10.0, ROW), 12.5, false, TEXT, 0.6, Align::Left);
            p.glyph('\u{E72A}', R::new(pad + half + 4.0, ry, 24.0, ROW), 10.0, TEXT, 0.4);
            let new = if e.status == Status::Same { "—".to_string() } else { e.to.clone() };
            p.text(&new, R::new(pad + half + 34.0, ry, half - 10.0, ROW), 12.5, e.status == Status::Ok, c, a, Align::Left);
        }
        if self.plan.len() > ROWS {
            let frac = ROWS as f32 / self.plan.len() as f32;
            let th = (list.h - 8.0) * frac;
            let ty = list.y + 4.0 + (list.h - 8.0 - th) * (self.scroll as f32 / max_scroll.max(1) as f32);
            p.round(R::new(list.right() - 6.0, ty, 3.0, th), 1.5, TEXT, 0.3);
        }
        y += list.h + 12.0;

        // Alt satır: özet + düğmeler.
        let blocked = plan::blocked(&self.plan);
        let changes = plan::changes(&self.plan);
        let (msg, c) = if let Some(e) = &self.error {
            (e.clone(), RED)
        } else if let Some(d) = &self.done {
            (t!(format!("{} öğe yeniden adlandırıldı", d.len()), format!("{} items renamed", d.len())), GREEN)
        } else if blocked > 0 {
            let first = self.plan.iter().find(|e| matches!(e.status, Status::Invalid | Status::Duplicate | Status::Exists)).map(|e| e.status);
            let why = match first {
                Some(Status::Invalid) => t!("geçersiz ad", "invalid name"),
                Some(Status::Duplicate) => t!("aynı ada giden öğeler", "duplicate names"),
                _ => t!("bu adda dosya zaten var", "file already exists"),
            };
            (t!(format!("{blocked} çakışma: {why}"), format!("{blocked} conflicts: {why}")), ORANGE)
        } else if changes == 0 {
            (t!("Değişecek ad yok", "Nothing to rename").to_string(), TEXT)
        } else {
            (t!(format!("{changes} öğe değişecek"), format!("{changes} items will change")), TEXT)
        };
        p.text(&msg, R::new(pad, y, iw - 230.0, 34.0), 12.5, false, c, 0.85, Align::Left);
        let bw = 108.0;
        if self.done.is_some() {
            p.button(R::new(w - pad - bw * 2.0 - 8.0, y, bw, 34.0), Some('\u{E7A7}'), t!("Geri al", "Undo"), A::Undo, true, false);
            p.button(R::new(w - pad - bw, y, bw, 34.0), None, t!("Tamam", "Done"), A::Close, true, false);
        } else {
            p.button(R::new(w - pad - bw * 2.0 - 8.0, y, bw, 34.0), None, t!("Vazgeç", "Cancel"), A::Close, true, false);
            let ok = blocked == 0 && changes > 0;
            let r = R::new(w - pad - bw, y, bw, 34.0);
            if ok {
                p.round(r, 6.0, p.accent, 0.9);
                p.text(t!("Uygula", "Apply"), r, 13.0, true, 0x000000, 0.9, Align::Center);
                p.hit(r, A::Apply);
            } else {
                p.button(r, None, t!("Uygula", "Apply"), A::Nothing, false, false);
            }
        }
    }

    fn click(&mut self, a: A, ctx: &mut Ctx) {
        let editing = self.done.is_none();
        match a {
            A::Close => ctx.close = true,
            A::Apply => self.apply(ctx),
            A::Undo => self.undo(),
            A::Nothing => {}
            _ if !editing => {}
            A::Mode(m) => {
                self.cfg.mode = m;
                self.focus = self.first_field();
                self.refresh();
            }
            A::Case(k) => {
                self.cfg.case = k;
                self.refresh();
            }
            A::Focus(i) => self.focus = i,
            A::KeepExt => {
                self.cfg.keep_ext = !self.cfg.keep_ext;
                self.refresh();
            }
            A::MatchCase => {
                self.cfg.match_case = !self.cfg.match_case;
                self.refresh();
            }
            A::Start(d) => {
                self.cfg.start = (self.cfg.start as i64 + d as i64 * if view::shift() { 10 } else { 1 }).clamp(0, 99_999) as u32;
                self.refresh();
            }
        }
    }

    fn key(&mut self, vk: u16, ctx: &mut Ctx) -> bool {
        match vk {
            0x0D => {
                if self.done.is_some() {
                    ctx.close = true;
                } else {
                    self.apply(ctx);
                }
                true
            }
            0x09 => {
                let vis = self.visible_fields();
                if !vis.is_empty() {
                    let at = vis.iter().position(|&i| i == self.focus).map(|k| (k + 1) % vis.len()).unwrap_or(0);
                    self.focus = vis[at];
                }
                true
            }
            0x5A if view::ctrl() && self.done.is_some() => {
                self.undo();
                true
            }
            _ if self.done.is_none() && self.focus < 3 => {
                let used = self.fields[self.focus].key(vk, view::ctrl());
                if used {
                    self.refresh();
                }
                used
            }
            _ => false,
        }
    }

    fn char(&mut self, c: char, _ctx: &mut Ctx) {
        if self.done.is_none() && self.focus < 3 && !view::ctrl() {
            self.fields[self.focus].char(c);
            self.refresh();
        }
    }

    fn wheel(&mut self, delta: f32, _at: (f32, f32), _ctx: &mut Ctx) {
        let n = (delta.abs().ceil() as usize).max(1) * 2;
        self.scroll = if delta > 0.0 { self.scroll.saturating_sub(n) } else { self.scroll + n };
    }
}

pub fn run(fg: windows::Win32::Foundation::HWND) -> windows::core::Result<()> {
    let mut files = crate::select::selection(fg);
    files.sort_by_key(|f| f.to_lowercase());
    files.dedup();
    if files.is_empty() {
        note::island(t!("Yeniden adlandır", "Rename"), t!("Önce dosya seçin", "Select files first"), "");
        return Ok(());
    }
    let cfg = config::load().tools.rename;
    let paths: Vec<PathBuf> = files.into_iter().map(PathBuf::from).collect();
    let mut pt = windows::Win32::Foundation::POINT::default();
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut pt);
    }
    view::run(Rename::new(paths, cfg), Opts { title: t!("Yeniden adlandır", "Rename").into(), w: 620.0, h: 482.0, at: Some((pt.x - 60, pt.y - 40)), resizable: false, popup: true, ..Default::default() })?;
    Ok(())
}
