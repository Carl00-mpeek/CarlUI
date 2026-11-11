//! Bu modül dock'un sağ ucundaki iki "yer"i çözüyor:
//! - **Klasör yığınları (İndirilenler vb.):** tıklayınca klasördeki en yeni
//!   15 dosya/klasör/kısayol kendi simgeleriyle yan yana bir cam ızgarada
//!   açılır; seçilen öğe açılır, üstteki düğme klasörün kendisini açar.
//! - **Geri Dönüşüm Kutusu:** tıklayınca açılır; sağ tıkta "Boşalt" vardır.
//!   Boşaltma Windows'un kendi onay penceresini gösterir (CarlUI sormadan
//!   hiçbir şeyi kalıcı silmez).

use super::*;
use carlui_ui::{
    glassgrid::{self, Cell},
    glassmenu::{self, Icon, Row},
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicI64, Ordering},
        Condvar, Mutex,
    },
};
use windows::Win32::UI::Shell::{SHEmptyRecycleBinW, SHQueryRecycleBinW, SHQUERYRBINFO};

pub const RECYCLE_BIN: &str = "::{645FF040-5081-101B-9F08-00AA002F954E}";
const MAX: usize = 15;
/// Çöp kutusu dolu/boş yoklaması (simge değişsin; Gezgin'den boşaltılsa da).
pub const TIMER_TRASH: usize = 77;

pub fn open(target: &str) {
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        ShellExecuteW(HWND::default(), w!("open"), &HSTRING::from(target), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

/// Yığının dock'ta görünen adı.
pub fn stack_label(name: &str, dir: &std::path::Path) -> String {
    match name {
        "desktop" => t!("Masaüstü", "Desktop").into(),
        "documents" => t!("Belgeler", "Documents").into(),
        "pictures" => t!("Resimler", "Pictures").into(),
        "music" => t!("Müzik", "Music").into(),
        "videos" => t!("Videolar", "Videos").into(),
        super::quick::KEY => t!("Hızlı Erişim", "Quick Access").into(),
        _ => dir.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| dir.display().to_string()),
    }
}

/// Klasördeki en yeni dosyalar (yarım indirmeler hariç). İndirilenler'de
/// oluşturulma, diğer klasörlerde değiştirilme zamanına göre.
fn recent_files(dir: &std::path::Path, by_created: bool) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<(std::time::SystemTime, PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let name = p.file_name()?.to_string_lossy().to_lowercase();
            if name.ends_with(".crdownload") || name.ends_with(".part") || name.ends_with(".tmp") || name == "desktop.ini" {
                return None;
            }
            let md = e.metadata().ok()?;
            let t = if by_created { md.created().or_else(|_| md.modified()) } else { md.modified() };
            Some((t.ok()?, p))
        })
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.into_iter().take(MAX).map(|(_, p)| p).collect()
}

/// Çöp kutusunda öğe sayısı. Doğrudan çağrılmaz: Windows bunu her sürücünün
/// çöp klasörünü tarayarak bulur; dolu kutuda, uyuyan diskte ya da ağ
/// sürücüsünde saniyeler sürebiliyor ve dock her 3 sn'de bir donuyordu.
fn trash_count() -> i64 {
    let mut info = SHQUERYRBINFO { cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32, ..Default::default() };
    unsafe { SHQueryRecycleBinW(PCWSTR::null(), &mut info) }.map(|_| info.i64NumItems).unwrap_or(0)
}

/// Arka planda yoklanan son sayı (-1: henüz bilinmiyor).
static TRASH_N: AtomicI64 = AtomicI64::new(-1);
static TRASH_WAKE: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());
static TRASH_STARTED: std::sync::Once = std::sync::Once::new();

/// Yoklayıcıyı (bir kez) başlatır; sayı değişince dock'a TIMER_TRASH
/// gönderilir ki simge beklemeden güncellensin.
fn trash_watch(hwnd: HWND) {
    let raw = hwnd.0 as isize;
    TRASH_STARTED.call_once(|| {
        let _ = std::thread::Builder::new().name("trash".into()).spawn(move || loop {
            let n = trash_count();
            if TRASH_N.swap(n, Ordering::SeqCst) != n {
                unsafe {
                    let _ = PostMessageW(HWND(raw as _), WM_TIMER, WPARAM(TIMER_TRASH), LPARAM(0));
                }
            }
            let (m, cv) = &TRASH_WAKE;
            let Ok(mut woke) = m.lock() else { return };
            if !*woke {
                woke = cv.wait_timeout(woke, std::time::Duration::from_secs(3)).map(|r| r.0).unwrap_or_else(|e| e.into_inner().0);
            }
            *woke = false;
        });
    });
}

/// Dolu/boş simgesinin kaynağı (ör. `imageres.dll,-54`). Kabuğun simge
/// önbelleğine sorulmaz: o önbellek bayat kalabiliyor ve dock'taki kutu
/// Gezgin'dekiyle ters (biri dolu biri boş) görünüyordu. Kullanıcının
/// özel simgesi (HKCU) varsa o, yoksa sistemin varsayılanı.
fn trash_icon_source(full: bool) -> Option<(String, i32)> {
    use windows::Win32::System::Registry::*;
    let name = if full { w!("Full") } else { w!("Empty") };
    let keys = [
        (HKEY_CURRENT_USER, w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\CLSID\\{645FF040-5081-101B-9F08-00AA002F954E}\\DefaultIcon")),
        (HKEY_CLASSES_ROOT, w!("CLSID\\{645FF040-5081-101B-9F08-00AA002F954E}\\DefaultIcon")),
    ];
    for (root, key) in keys {
        let mut buf = [0u16; 520];
        let mut size = (buf.len() * 2) as u32;
        let ok = unsafe { RegGetValueW(root, key, name, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as _), Some(&mut size)) };
        if ok.is_err() {
            continue;
        }
        // Genişletilmiş değerde bildirilen boy güvenilmez; ilk boş karaktere kadar oku.
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let s = String::from_utf16_lossy(&buf[..end]);
        let s = s.trim();
        let (path, idx) = match s.rsplit_once(',') {
            Some((p, i)) => (p, i.trim().parse().unwrap_or(0)),
            None => (s, 0),
        };
        let path = path.trim().trim_matches('"');
        if !path.is_empty() {
            return Some((path.to_string(), idx));
        }
    }
    None
}

fn trash_icon(gfx: &Gfx, full: bool, px: i32) -> Option<windows::Win32::Graphics::Imaging::IWICFormatConverter> {
    use windows::Win32::UI::{Shell::SHDefExtractIconW, WindowsAndMessaging::DestroyIcon};
    let (path, idx) = trash_icon_source(full)?;
    let req = (px * 2).clamp(64, 256) as u32;
    let mut icon = HICON::default();
    unsafe {
        SHDefExtractIconW(&HSTRING::from(path), idx, 0, Some(&mut icon), None, req).ok().ok()?;
        let r = icons::hicon_bitmap(&gfx.wic, icon).ok();
        let _ = DestroyIcon(icon);
        r
    }
}

/// Yoklayıcıya "hemen yeniden say" der (boşaltmadan sonra).
fn trash_poke() {
    let (m, cv) = &TRASH_WAKE;
    if let Ok(mut w) = m.lock() {
        *w = true;
        cv.notify_one();
    }
}

impl Dock {
    /// Çöp kutusu doluluğu değiştiyse simgeyi yeniden yükler ve küçük bir
    /// "zıplama" ile gösterir. `force`: değişmese de yenile.
    pub fn trash_tick(&mut self, gfx: &Gfx, force: bool) {
        if !self.cfg.dock.recycle_bin {
            return;
        }
        // Sayım arka planda; burada yalnızca son sonuç okunur.
        trash_watch(self.hwnd);
        let n = TRASH_N.load(Ordering::SeqCst);
        if n < 0 {
            return;
        }
        let full = n > 0;
        if !force && self.trash_full == Some(full) {
            return;
        }
        let first = self.trash_full.is_none();
        self.trash_full = Some(full);
        let Some(e) = self.entries.iter().find(|e| e.kind == Kind::Trash && e.leaving.is_none()) else { return };
        let px = self.m.icon;
        let Some(src) = trash_icon(gfx, full, px as i32).or_else(|| icons::shell_icon(&gfx.wic, RECYCLE_BIN, px as i32).ok()) else { return };
        let Ok(surface) = gfx.surface(px, px, |dc| icons::draw_bitmap(dc, &src, px)) else { return };
        if let Ok(b) = self.comp.CreateSurfaceBrushWithSurface(&surface) {
            let _ = b.SetStretch(CompositionStretch::Fill);
            let _ = e.icon.SetBrush(&b);
        }
        if !first {
            let _ = e.icon.SetScale(Vector3 { X: 0.82, Y: 0.82, Z: 1.0 });
            let _ = self.motion.scale(&e.icon, 1.0, 0.45, 45, 0);
        }
    }

    fn menu(&mut self, i: usize, rows: Vec<Row>) -> usize {
        let (at, side) = self.menu_at(i);
        self.menu_open = true;
        let id = glassmenu::run_side(self.hwnd, rows, self.cfg.dock.menu_accent_color(glassmenu::accent), at, side).unwrap_or(0);
        self.menu_open = false;
        unsafe {
            let _ = PostMessageW(self.hwnd, WM_NULL, WPARAM(0), LPARAM(0));
        }
        id
    }

    /// Klasör yığını (İndirilenler ya da ayarlardan eklenen): klasördeki en
    /// yeni dosya, klasör ve kısayollar yan yana simge ızgarasında.
    pub fn stack_menu(&mut self, i: usize) {
        let Some(e) = self.entries.get(i) else { return };
        let downloads = e.key == "downloads";
        let quick = e.key == super::quick::KEY;
        let dir = if e.exe.is_empty() { win::downloads_dir() } else { Some(PathBuf::from(&e.exe)) };
        let Some(dir) = dir else { return };
        let name = e.name.clone();
        let files = if quick { super::quick::items(&dir) } else { recent_files(&dir, downloads) };
        // Simgeler saklanır; USB/ağdaki İndirilenler pencereyi bekletmez.
        let icon = |p: &str| crate::iconcache::shell(p, 64).map(Icon::Image).unwrap_or(Icon::None);
        let cells: Vec<Cell> = files
            .iter()
            .enumerate()
            .map(|(n, p)| {
                let mut text = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                // Kısayolların uzantısı gösterilmez (Gezgin gibi).
                for ext in [".lnk", ".url"] {
                    if text.to_lowercase().ends_with(ext) {
                        text.truncate(text.len() - ext.len());
                    }
                }
                Cell { id: 10 + n, text, icon: icon(&p.display().to_string()) }
            })
            .collect();
        let empty = if downloads {
            t!("Henüz indirilen dosya yok", "No downloads yet")
        } else if quick {
            t!("Boş: sağ tıkla klasör ya da uygulama ekle", "Empty: right-click to add folders or apps")
        } else {
            t!("Klasör boş", "Folder is empty")
        };
        let button = if quick { t!("Düzenle", "Edit") } else { t!("Klasörü aç", "Open folder") };
        let (at, side) = self.menu_at(i);
        self.menu_open = true;
        let id = glassgrid::run_side(self.hwnd, &name, (1, button.into()), empty, cells, self.cfg.dock.menu_accent_color(glassmenu::accent), at, side).unwrap_or(0);
        self.menu_open = false;
        unsafe {
            let _ = PostMessageW(self.hwnd, WM_NULL, WPARAM(0), LPARAM(0));
        }
        match id {
            1 => open(&dir.display().to_string()),
            id if id >= 10 => {
                if let Some(p) = files.get(id - 10) {
                    open(&p.display().to_string());
                }
            }
            _ => {}
        }
    }

    /// Hızlı Erişim sağ tık menüsü: klasör / uygulama ekle, düzenle.
    pub fn quick_menu(&mut self, i: usize) {
        let Some(e) = self.entries.get(i) else { return };
        let dir = PathBuf::from(&e.exe);
        let rows = vec![
            Row::Item { id: 1, text: t!("Klasör ekle…", "Add folder…").into(), icon: Icon::Glyph('\u{E8F4}', None) },
            Row::Item { id: 2, text: t!("Uygulama ya da dosya ekle…", "Add app or file…").into(), icon: Icon::Glyph('\u{E710}', None) },
            Row::Sep,
            Row::Item { id: 3, text: t!("Düzenle (klasörü aç)", "Edit (open folder)").into(), icon: Icon::Glyph('\u{E70F}', None) },
        ];
        let id = self.menu(i, rows);
        let target = match id {
            1 => super::quick::pick(self.hwnd, true),
            2 => super::quick::pick(self.hwnd, false),
            3 => {
                open(&dir.display().to_string());
                None
            }
            _ => None,
        };
        if let Some(t) = target {
            if !super::quick::add(&t) {
                carlui_core::warn!("hızlı erişime eklenemedi: {t}");
            }
        }
    }

    /// Çöp kutusu sağ tık menüsü: aç / boşalt.
    pub fn trash_menu(&mut self, i: usize) {
        let n = match TRASH_N.load(Ordering::SeqCst) {
            -1 => trash_count(),
            n => n,
        };
        let mut rows = vec![Row::Item { id: 1, text: t!("Aç", "Open").into(), icon: Icon::Glyph('\u{E8E5}', None) }];
        if n > 0 {
            rows.push(Row::Item { id: 2, text: t!(format!("Boşalt ({n} öğe)"), format!("Empty ({n} items)")), icon: Icon::Glyph('\u{E74D}', Some(0xFF6B6B)) });
        } else {
            rows.push(Row::Label(t!("Geri Dönüşüm Kutusu boş", "Recycle Bin is empty").into(), Icon::None));
        }
        match self.menu(i, rows) {
            1 => open(RECYCLE_BIN),
            // Onay penceresi Windows'tan gelir; kullanıcı vazgeçebilir.
            2 => unsafe {
                if let Err(e) = SHEmptyRecycleBinW(self.hwnd, PCWSTR::null(), 0) {
                    carlui_core::info!("çöp kutusu boşaltılmadı: {e}");
                }
                // Simge hemen boşa dönsün: yoklayıcı yeniden sayar, değişince haber verir.
                trash_poke();
            },
            _ => {}
        }
    }
}
