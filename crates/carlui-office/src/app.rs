//! Bu modül "tek pencerede dört belge türü (tablo, metin, sunu, PDF) aynı
//! menü, kaydetme ve kapatma akışıyla çalışsın" sorununu çözüyor. Her tür
//! `View` arayüzünü uygular; pencere olayları (tuş, fare, çizim) etkin
//! görünüme iletilir. Belge yokken ana ekran (yeni / aç / son dosyalar) çizilir.
//! Her belge kendi sürecinde açılır: ikinci dosya yeni pencere demektir,
//! pencere kapanınca süreç biter, arka planda hiçbir şey kalmaz.

use crate::{dialog, recent};
use carlui_core::t;
use carlui_ui::paint::{Painter, R};
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    time::Instant,
};
use windows::{
    core::HSTRING,
    Win32::{Foundation::HWND, UI::WindowsAndMessaging::*},
};

/// Araç çubuğu ve menü komutları (tüm görünümler için ortak; ilgisiz olanı görünüm yok sayar).
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    Bold,
    Italic,
    Underline,
    Strike,
    Align(crate::rich::PAlign),
    /// Punto artır (+1) / azalt (-1).
    Size(i8),
    /// 0 = normal, 1.. = başlık düzeyi.
    Heading(u8),
    Color(u32),
    ColorMenu,
    Undo,
    Redo,
    Zoom(i8),
    /// Sayfa / slayt / sekme seç.
    Pick(usize),
    Add,
    Remove,
    Duplicate,
    Rotate,
    /// PDF'ye not ekleme kipini aç/kapat.
    NoteTool,
    Merge,
    Extract,
    Present,
    /// Slayta metin kutusu ekle.
    TextBox,
    /// Sayı biçimi: 0 genel, 1 sayı, 2 para, 3 yüzde, 4 tarih.
    NumFmt(u8),
    /// Görünüme özel menü (sağ tık / "daha fazla").
    More,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    NewSheet,
    NewDoc,
    Open,
    Recent(usize),
    Save,
    SaveAs,
    FileMenu,
    View(Cmd),
    /// Belge alanı: fare olayı görünüme gider.
    Content,
    Scroll,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mouse {
    Down,
    Double,
    Move,
    Up,
    Right,
}

pub struct Keys {
    pub ctrl: bool,
    pub shift: bool,
}

/// Her belge türünün görünümü.
pub trait View {
    /// Araç çubuğunun görünüme ait kısmı (`r` içinde, soldan sağa).
    fn toolbar(&mut self, p: &mut Painter<Act>, r: R);
    /// Belge alanı.
    fn paint(&mut self, p: &mut Painter<Act>, area: R);
    /// Alt durum çubuğundaki metin.
    fn status(&self) -> String {
        String::new()
    }
    fn cmd(&mut self, c: Cmd);
    /// Tuş; işlendiyse true.
    fn key(&mut self, vk: u16, k: &Keys) -> bool;
    fn char(&mut self, ch: char) -> bool;
    fn mouse(&mut self, m: Mouse, pt: (f32, f32), k: &Keys);
    fn wheel(&mut self, d: f32, k: &Keys);
    fn dirty(&self) -> bool;
    /// Kaydedilecek dosya uzantısı (farklı kaydet önerisi).
    fn ext(&self) -> &'static str;
    /// Bu uzantılara kaydedebilir mi.
    fn can_save(&self, ext: &str) -> bool {
        ext == self.ext()
    }
    fn save(&mut self, path: &Path) -> Result<(), String>;
    /// Çizim hedefi yeniden kuruldu: bitmap/düzen önbelleklerini boşalt.
    fn reset_device(&mut self) {}
    /// İmleç yanıp sönmeli mi / arka planda iş var mı (zamanlayıcı için).
    fn ticking(&self) -> bool {
        false
    }
    /// Yazı imleci gösteriliyor mu.
    fn editing(&self) -> bool {
        false
    }
    /// Tam ekran (sunu) kipi: çerçeve ve araç çubukları gizlenir.
    fn fullscreen(&self) -> bool {
        false
    }
}

thread_local! {
    static NOTICE: RefCell<Option<(String, Instant)>> = const { RefCell::new(None) };
    static HWND_: Cell<isize> = const { Cell::new(0) };
    static CARET: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// Alttan kayan kısa bilgi.
pub fn toast(m: impl Into<String>) {
    NOTICE.with(|n| *n.borrow_mut() = Some((m.into(), Instant::now())));
}

pub fn notice() -> Option<(String, Instant)> {
    NOTICE.with(|n| n.borrow().clone())
}

pub fn hwnd() -> HWND {
    HWND(HWND_.with(|h| h.get()) as _)
}

/// İmleç yeniden görünür olsun (tuşa basınca yanıp sönme baştan başlar).
pub fn caret_reset() {
    CARET.with(|c| c.set(Some(Instant::now())));
}

/// İmleç şu an görünür mü (530 ms yanar / söner).
pub fn caret_on() -> bool {
    let t = CARET.with(|c| c.get()).map(|t| t.elapsed().as_millis()).unwrap_or(0);
    (t / 530).is_multiple_of(2)
}

pub fn ask(text: &str, buttons: MESSAGEBOX_STYLE) -> MESSAGEBOX_RESULT {
    unsafe { MessageBoxW(hwnd(), &HSTRING::from(text), &HSTRING::from("Carl Office"), buttons) }
}

pub fn error(text: &str) {
    ask(text, MB_OK | MB_ICONWARNING);
}

/// Dosyayı önce yanına geçici adla yazar, sonra yerine taşır: yazma yarıda
/// kalırsa (disk dolu, elektrik) özgün dosya bozulmaz.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".carl-tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

pub struct App {
    pub view: Option<Box<dyn View>>,
    pub path: Option<PathBuf>,
    pub recent: Vec<PathBuf>,
    pub title: String,
}

impl App {
    pub fn new(hwnd: HWND) -> Self {
        HWND_.with(|h| h.set(hwnd.0 as isize));
        App { view: None, path: None, recent: recent::load(), title: String::new() }
    }

    pub fn name(&self) -> String {
        match &self.path {
            Some(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            None => match self.view.as_ref().map(|v| v.ext()) {
                Some("xlsx") => t!("Yeni tablo", "New spreadsheet").to_string(),
                Some("docx") => t!("Yeni belge", "New document").to_string(),
                Some(_) => t!("Adsız", "Untitled").to_string(),
                None => String::new(),
            },
        }
    }

    pub fn dirty(&self) -> bool {
        self.view.as_ref().is_some_and(|v| v.dirty())
    }

    /// Pencere başlığı değiştiyse günceller.
    pub fn sync_title(&mut self) {
        let t = match &self.view {
            None => "Carl Office".to_string(),
            Some(_) => format!("{}{} — Carl Office", if self.dirty() { "• " } else { "" }, self.name()),
        };
        if t != self.title {
            unsafe {
                let _ = SetWindowTextW(hwnd(), &HSTRING::from(t.as_str()));
            }
            self.title = t;
        }
    }

    /// Dosyayı açar. Bu pencerede zaten belge varsa yeni süreçte açar.
    pub fn open(&mut self, path: &Path) {
        if self.view.is_some() {
            if let Ok(exe) = std::env::current_exe() {
                let _ = std::process::Command::new(exe).arg(path).spawn();
            }
            return;
        }
        match crate::load(path) {
            Ok(v) => {
                self.view = Some(v);
                self.path = Some(path.to_path_buf());
                self.recent = recent::add(path);
                carlui_core::info!("açıldı: {}", path.display());
            }
            Err(e) => {
                carlui_core::warn!("açılamadı {}: {e}", path.display());
                error(&format!("{}\n\n{}\n{e}", t!("Dosya açılamadı.", "Couldn't open the file."), path.display()));
            }
        }
    }

    fn new_view(&mut self, v: Box<dyn View>, arg: &str) {
        if self.view.is_some() {
            if let Ok(exe) = std::env::current_exe() {
                let _ = std::process::Command::new(exe).arg(arg).spawn();
            }
            return;
        }
        self.view = Some(v);
        self.path = None;
    }

    pub fn act(&mut self, a: Act) {
        match a {
            Act::NewSheet => self.new_view(Box::new(crate::sheet::view::SheetView::new(crate::sheet::Book::new_blank())), "--new-sheet"),
            Act::NewDoc => self.new_view(Box::new(crate::doc::view::DocView::new(crate::doc::Document::blank())), "--new-doc"),
            Act::Open => {
                if let Some(p) = dialog::open(hwnd()) {
                    self.open(&p);
                }
            }
            Act::Recent(i) => {
                if let Some(p) = self.recent.get(i).cloned() {
                    self.open(&p);
                }
            }
            Act::Save => {
                self.save(false);
            }
            Act::SaveAs => {
                self.save(true);
            }
            Act::FileMenu => self.file_menu(),
            Act::View(c) => {
                if let Some(v) = self.view.as_mut() {
                    v.cmd(c);
                }
            }
            Act::Content | Act::Scroll => {}
        }
    }

    fn file_menu(&mut self) {
        use carlui_ui::glassmenu::{run, Icon, Row};
        let item = |id, text: &str, g| Row::Item { id, text: text.to_string(), icon: Icon::Glyph(g, None) };
        let mut rows = vec![
            item(1, t!("Yeni tablo", "New spreadsheet"), '\u{E80A}'),
            item(2, t!("Yeni belge", "New document"), '\u{E8A5}'),
            item(3, t!("Aç…  (Ctrl+O)", "Open…  (Ctrl+O)"), '\u{E8E5}'),
            Row::Sep,
        ];
        if self.view.is_some() {
            rows.push(item(4, t!("Kaydet  (Ctrl+S)", "Save  (Ctrl+S)"), '\u{E74E}'));
            rows.push(item(5, t!("Farklı kaydet…", "Save as…"), '\u{E792}'));
        }
        if let Some(p) = &self.path {
            if p.parent().is_some() {
                rows.push(item(6, t!("Klasörde göster", "Show in folder"), '\u{E838}'));
            }
        }
        let mut pt = windows::Win32::Foundation::POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        match run(hwnd(), rows, None, (pt.x, pt.y)) {
            Some(1) => self.act(Act::NewSheet),
            Some(2) => self.act(Act::NewDoc),
            Some(3) => self.act(Act::Open),
            Some(4) => self.act(Act::Save),
            Some(5) => self.act(Act::SaveAs),
            Some(6) => {
                if let Some(p) = &self.path {
                    let _ = std::process::Command::new("explorer.exe").arg(format!("/select,{}", p.display())).spawn();
                }
            }
            _ => {}
        }
    }

    /// Kaydeder; başarılıysa true. `as_new`: her zaman yer sor.
    pub fn save(&mut self, as_new: bool) -> bool {
        let Some(v) = self.view.as_mut() else { return false };
        let cur_ext = self.path.as_ref().and_then(|p| p.extension()).map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        // Biçim yazılamıyorsa (.xls) farklı kaydet'e düş.
        let target = match &self.path {
            Some(p) if !as_new && v.can_save(&cur_ext) => p.clone(),
            _ => {
                let stem = self.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| self_name(v.ext()));
                let Some(p) = dialog::save(hwnd(), &format!("{stem}.{}", v.ext()), v.ext()) else { return false };
                p
            }
        };
        match v.save(&target) {
            Ok(()) => {
                self.path = Some(target.clone());
                self.recent = recent::add(&target);
                toast(t!("Kaydedildi", "Saved"));
                carlui_core::info!("kaydedildi: {}", target.display());
                true
            }
            Err(e) => {
                carlui_core::warn!("kaydedilemedi {}: {e}", target.display());
                error(&format!("{}\n\n{e}", t!("Kaydedilemedi.", "Couldn't save.")));
                false
            }
        }
    }

    /// Kapatmadan önce: değişiklik varsa sor. Kapanabilirse true.
    pub fn confirm_close(&mut self) -> bool {
        if !self.dirty() {
            return true;
        }
        let q = format!("{}\n\n{}", t!("Değişiklikler kaydedilsin mi?", "Save your changes?"), self.name());
        match ask(&q, MB_YESNOCANCEL | MB_ICONQUESTION) {
            IDYES => self.save(false),
            IDNO => true,
            _ => false,
        }
    }
}

fn self_name(ext: &str) -> String {
    match ext {
        "xlsx" => t!("Tablo", "Spreadsheet").to_string(),
        "docx" => t!("Belge", "Document").to_string(),
        "pptx" => t!("Sunu", "Presentation").to_string(),
        _ => t!("Belge", "Document").to_string(),
    }
}
