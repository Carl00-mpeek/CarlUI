//! Dock'un sağ tık menüsü: neyin, hangi sırayla, hangi simgeyle görüneceğini
//! belirler; çizimi `glassmenu` yapar (cam görünüm, vurgu renkli başlıklar).
//! Menü her zaman dock'un üstüne, tıklanan ikonun ortasına hizalı açılır.
//! İki ayrı menü vardır:
//! - Uygulama ikonu: uygulamanın kendi görevleri (Firefox: gizli pencere…,
//!   kendi logolarıyla) → uygulama adı → pencere işlemleri → Programı durdur
//!   → Programı kapat (ikona en yakın).
//! - Dock'un boş yeri: Görev çubuğunu geri getir / CarlUI'den çık (en üstte,
//!   yanlışlıkla tıklanmasın) → Görev yöneticisi → Ayarlar.

use crate::{
    dock::{Entry, Kind},
    iconcache,
    jumplist::Group,
};
use carlui_ui::glassmenu::{self, Icon, Row};
use carlui_core::config::{expand_env, DockConfig};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::LibraryLoader::{GetProcAddress, LoadLibraryW},
        UI::{Shell::SHDefExtractIconW, WindowsAndMessaging::*},
    },
};

const JUMP_BASE: usize = 100;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmd {
    None,
    NewWindow,
    TogglePin,
    CloseWindow,
    CloseAll,
    TaskManager,
    Suspend,
    Resume,
    Kill,
    /// Uygulamanın kendi görevi (jump list), düz sıradaki numarası.
    Jump(usize),
    Settings,
    RestoreTaskbar,
    Quit,
}

const IDS: [(usize, Cmd); 11] = [
    (1, Cmd::NewWindow),
    (2, Cmd::TogglePin),
    (3, Cmd::CloseWindow),
    (4, Cmd::CloseAll),
    (5, Cmd::TaskManager),
    (6, Cmd::Suspend),
    (7, Cmd::Resume),
    (8, Cmd::Kill),
    (10, Cmd::Settings),
    (11, Cmd::RestoreTaskbar),
    (12, Cmd::Quit),
];

// Segoe Fluent Icons glifleri.
const G_BACK: char = '\u{E72B}';
const G_POWER: char = '\u{E7E8}';
const G_NEW_WINDOW: char = '\u{E78B}';
const G_PIN: char = '\u{E718}';
const G_UNPIN: char = '\u{E77A}';
const G_CLOSE: char = '\u{E8BB}';
const G_TASKMGR: char = '\u{E9D9}';
const G_SETTINGS: char = '\u{E713}';
const G_PLAY: char = '\u{E768}';
const G_KILL: char = '\u{E711}';
/// 🚫 — "Programı durdur" (kırmızı daire, çapraz çizgi).
const G_BLOCKED: char = '\u{E733}';
const RED: u32 = 0xFF6B6B;

/// Uygulamanın menülerini Windows'un koyu temasına geçirir (uxtheme'nin
/// belgelenmemiş ama Explorer'ın da kullandığı 135/136 numaralı işlevleri).
pub fn enable_dark_menus() {
    unsafe {
        let Ok(lib) = LoadLibraryW(w!("uxtheme.dll")) else { return };
        if let Some(set_mode) = GetProcAddress(lib, PCSTR(135 as _)) {
            let set_mode: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(set_mode);
            set_mode(2); // ForceDark
        }
        if let Some(flush) = GetProcAddress(lib, PCSTR(136 as _)) {
            let flush: unsafe extern "system" fn() = std::mem::transmute(flush);
            flush();
        }
    }
}

/// Uygulamanın kendi logosu (Store uygulamasında AUMID, değilse exe).
fn app_icon(e: &Entry) -> Icon {
    let src = if !e.app.aumid.is_empty() {
        format!("shell:AppsFolder\\{}", e.app.aumid)
    } else if !e.exe.is_empty() {
        e.exe.clone()
    } else {
        e.app.exe.clone()
    };
    iconcache::shell(&src, 24).map(Icon::Image).unwrap_or(Icon::None)
}

/// Görevin kendi simgesi: kısayolda yazan simge dosyası, yoksa hedefinki.
/// Bir kez çıkarılıp saklanır; yavaş sürücülerdekiler atlanır (logo kullanılır).
fn jump_icon(icon: &(String, i32), target: &str) -> Option<Icon> {
    let path = expand_env(&icon.0);
    if !path.is_empty() && !path.starts_with("ms-appx") && !iconcache::slow_path(&path) {
        let img = iconcache::get(&format!("x{}|{path}", icon.1), |wic| unsafe {
            let mut h = HICON::default();
            if SHDefExtractIconW(&HSTRING::from(path.as_str()), icon.1, 0, Some(&mut h), None, 32).is_err() || h.is_invalid() {
                return None;
            }
            let img = carlui_ui::icons::hicon_bitmap(wic, h).ok();
            let _ = DestroyIcon(h);
            img
        });
        if let Some(img) = img {
            return Some(Icon::Image(img));
        }
    }
    iconcache::shell(target, 24).map(Icon::Image)
}

/// `suspended`: uygulamanın çalışan süreci varsa durdurulmuş mu (yoksa `None`).
/// `jump`: uygulamanın kendi görevleri (Firefox: gizli pencere vb.).
/// `cfg`: başlık rengi ayarı (`dock.menu_accent`).
pub fn show(hwnd: HWND, cfg: &DockConfig, entry: Option<&Entry>, suspended: Option<bool>, jump: &[Group], at: (i32, i32), side: i32) -> Cmd {
    let item =|id: usize, text: &str, icon: Icon| Row::Item { id, text: text.to_string(), icon };
    let glyph = |c: char| Icon::Glyph(c, None);
    let mut rows = Vec::new();
    let app = entry.filter(|e| e.kind == Kind::App);
    if app.is_none() {
        // Dock'un boş yeri: yalnızca CarlUI'nin kendi seçenekleri; çıkışlar
        // en üstte (fareden en uzak, yanlışlıkla tıklanmasın).
        rows.push(item(11, t!("Görev çubuğunu geri getir ve çık", "Restore taskbar and quit"), glyph(G_BACK)));
        rows.push(item(12, t!("CarlUI'den çık", "Quit CarlUI"), glyph(G_POWER)));
        rows.push(Row::Sep);
        rows.push(item(5, t!("Görev yöneticisi", "Task manager"), glyph(G_TASKMGR)));
        rows.push(item(10, t!("Ayarlar", "Settings"), glyph(G_SETTINGS)));
    }
    if let Some(e) = app {
        // Logo bir kez okunur (eskiden simgesiz her görev satırı için yeniden).
        let logo = app_icon(e);
        let mut n = 0;
        for g in jump {
            rows.push(Row::Header(if g.title.is_empty() { t!("Görevler", "Tasks").into() } else { g.title.clone() }));
            for it in &g.items {
                let icon = jump_icon(&it.icon, &it.target).unwrap_or_else(|| logo.clone());
                rows.push(item(JUMP_BASE + n, &it.title, icon));
                n += 1;
            }
            rows.push(Row::Sep);
        }
        rows.push(Row::Label(e.name.clone(), logo));
        rows.push(item(1, t!("Yeni pencere", "New window"), glyph(G_NEW_WINDOW)));
        // Açık Explorer'a "sabitle" önerilmez; ama sabitlenmişse kaldırılabilmeli.
        if e.pin.is_some() {
            rows.push(item(2, t!("Görev çubuğundan kaldır", "Unpin from dock"), glyph(G_UNPIN)));
        } else if !e.is_explorer() {
            rows.push(item(2, t!("Görev çubuğuna sabitle", "Pin to dock"), glyph(G_PIN)));
        }
        match e.windows.len() {
            0 => {}
            1 => rows.push(item(3, t!("Pencereyi kapat", "Close window"), glyph(G_CLOSE))),
            n => rows.push(item(4, &t!(format!("Tüm pencereleri kapat ({n})"), format!("Close all windows ({n})")), glyph(G_CLOSE))),
        }
        // Durdur/kapat en altta, ikonun hemen üstünde.
        if let (Some(s), false) = (suspended, e.is_explorer()) {
            rows.push(Row::Sep);
            rows.push(if s { item(7, t!("Programı sürdür", "Resume program"), glyph(G_PLAY)) } else { item(6, t!("Programı durdur", "Suspend program"), Icon::Glyph(G_BLOCKED, Some(RED))) });
            rows.push(item(8, t!("Programı kapat", "Kill program"), Icon::Glyph(G_KILL, Some(RED))));
        }
    }

    let id = glassmenu::run_side(hwnd, rows, cfg.menu_accent_color(glassmenu::accent), at, side).unwrap_or(0);
    unsafe {
        // Menü kapandı; odağı alttaki pencereye geri bırak.
        let _ = PostMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));
    }
    if id >= JUMP_BASE {
        return Cmd::Jump(id - JUMP_BASE);
    }
    IDS.iter().find(|(i, _)| *i == id).map(|(_, c)| *c).unwrap_or(Cmd::None)
}
