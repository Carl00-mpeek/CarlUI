//! Bu modül dock'un sağ ucundaki iki "yer"i çözüyor:
//! - **İndirilenler yığını:** tıklayınca son indirilen 12 dosya (en yeni
//!   altta, ikona en yakın) kendi simgeleriyle cam menüde açılır; seçilen
//!   dosya açılır, en üstteki satır klasörün kendisini açar.
//! - **Geri Dönüşüm Kutusu:** tıklayınca açılır; sağ tıkta "Boşalt" vardır.
//!   Boşaltma Windows'un kendi onay penceresini gösterir (CarlUI sormadan
//!   hiçbir şeyi kalıcı silmez).

use super::*;
use carlui_ui::glassmenu::{self, Icon, Row};
use std::path::PathBuf;
use windows::Win32::{
    Graphics::Imaging::{CLSID_WICImagingFactory, IWICImagingFactory},
    System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
    UI::Shell::{SHEmptyRecycleBinW, SHQueryRecycleBinW, SHQUERYRBINFO},
};

pub const RECYCLE_BIN: &str = "::{645FF040-5081-101B-9F08-00AA002F954E}";
const MAX: usize = 12;
/// Çöp kutusu dolu/boş yoklaması (simge değişsin; Gezgin'den boşaltılsa da).
pub const TIMER_TRASH: usize = 77;

pub fn open(target: &str) {
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        ShellExecuteW(HWND::default(), w!("open"), &HSTRING::from(target), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

/// İndirilenler'deki en yeni dosyalar (yarım indirmeler hariç).
fn recent_downloads(dir: &std::path::Path) -> Vec<PathBuf> {
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
            Some((md.created().or_else(|_| md.modified()).ok()?, p))
        })
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.into_iter().take(MAX).map(|(_, p)| p).collect()
}

/// Çöp kutusunda öğe sayısı.
fn trash_count() -> i64 {
    let mut info = SHQUERYRBINFO { cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32, ..Default::default() };
    unsafe { SHQueryRecycleBinW(PCWSTR::null(), &mut info) }.map(|_| info.i64NumItems).unwrap_or(0)
}

impl Dock {
    /// Çöp kutusu doluluğu değiştiyse simgeyi yeniden yükler ve küçük bir
    /// "zıplama" ile gösterir. `force`: değişmese de yenile.
    pub fn trash_tick(&mut self, gfx: &Gfx, force: bool) {
        if !self.cfg.dock.recycle_bin {
            return;
        }
        let full = trash_count() > 0;
        if !force && self.trash_full == Some(full) {
            return;
        }
        let first = self.trash_full.is_none();
        self.trash_full = Some(full);
        let Some(e) = self.entries.iter().find(|e| e.kind == Kind::Trash && e.leaving.is_none()) else { return };
        let px = self.m.icon;
        let Ok(src) = icons::shell_icon(&gfx.wic, RECYCLE_BIN, px as i32) else { return };
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
        let at = self.anchor(i);
        let at = (at.0, at.1 - (6.0 * self.m.s) as i32);
        self.menu_open = true;
        let id = glassmenu::run(self.hwnd, rows, self.cfg.dock.menu_accent_color(glassmenu::accent), at).unwrap_or(0);
        self.menu_open = false;
        unsafe {
            let _ = PostMessageW(self.hwnd, WM_NULL, WPARAM(0), LPARAM(0));
        }
        id
    }

    /// İndirilenler yığını: son dosyalar menüsü.
    pub fn stack_menu(&mut self, i: usize) {
        let Some(dir) = win::downloads_dir() else { return };
        let files = recent_downloads(&dir);
        let wic: Option<IWICImagingFactory> = unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok() };
        let icon = |p: &str| wic.as_ref().and_then(|w| icons::shell_icon(w, p, 24).ok()).map(Icon::Image).unwrap_or(Icon::None);
        let mut rows = vec![Row::Item { id: 1, text: "İndirilenler klasörünü aç".into(), icon: Icon::Glyph('\u{E838}', None) }];
        if files.is_empty() {
            rows.push(Row::Sep);
            rows.push(Row::Label("Henüz indirilen dosya yok".into(), Icon::None));
        } else {
            rows.push(Row::Header("Son indirilenler".into()));
            // En yeni en altta: ikona en yakın.
            for (n, p) in files.iter().enumerate().rev() {
                let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                rows.push(Row::Item { id: 10 + n, text: name, icon: icon(&p.display().to_string()) });
            }
        }
        match self.menu(i, rows) {
            1 => open(&dir.display().to_string()),
            id if id >= 10 => {
                if let Some(p) = files.get(id - 10) {
                    open(&p.display().to_string());
                }
            }
            _ => {}
        }
    }

    /// Çöp kutusu sağ tık menüsü: aç / boşalt.
    pub fn trash_menu(&mut self, i: usize) {
        let n = trash_count();
        let mut rows = vec![Row::Item { id: 1, text: "Aç".into(), icon: Icon::Glyph('\u{E8E5}', None) }];
        if n > 0 {
            rows.push(Row::Item { id: 2, text: format!("Boşalt ({n} öğe)"), icon: Icon::Glyph('\u{E74D}', Some(0xFF6B6B)) });
        } else {
            rows.push(Row::Label("Geri Dönüşüm Kutusu boş".into(), Icon::None));
        }
        match self.menu(i, rows) {
            1 => open(RECYCLE_BIN),
            // Onay penceresi Windows'tan gelir; kullanıcı vazgeçebilir.
            2 => unsafe {
                if let Err(e) = SHEmptyRecycleBinW(self.hwnd, PCWSTR::null(), 0) {
                    carlui_core::info!("çöp kutusu boşaltılmadı: {e}");
                }
                // Simge hemen boşa dönsün.
                SetTimer(self.hwnd, TIMER_TRASH, 200, None);
            },
            _ => {}
        }
    }
}
