//! Bu modül "görev çubuğu gizlensin, yer kaplamasın ama ne olursa olsun eski
//! haline dönsün" sorununu şöyle çözüyor:
//! - Windows 11'de gizli çubuk da ekranın altında yer ayırmaya devam eder ve
//!   explorer çalışma alanını kısa sürede geri yazar. Bu yüzden çubuk geçici
//!   olarak "otomatik gizle" moduna alınır (alan serbest kalır), ardından
//!   pencereleri (`Shell_TrayWnd`, `Shell_SecondaryTrayWnd`) tamamen gizlenir.
//! - Kullanıcının özgün "otomatik gizle" ayarı değiştirilmeden önce bir durum
//!   dosyasına yazılır. Dosya "sistemi değiştirdik, henüz geri almadık"
//!   anlamına gelir: normal kapanış, koruyucu süreç ve bir sonraki açılış bu
//!   dosyaya bakarak kullanıcının ayarını aynen geri yükler.

use carlui_core::{info, paths, warn, win};
use serde::{Deserialize, Serialize};
use windows::Win32::{
    Foundation::*,
    UI::{Shell::*, WindowsAndMessaging::*},
};

pub const CLASSES: [&str; 2] = ["Shell_TrayWnd", "Shell_SecondaryTrayWnd"];
const ABS_AUTOHIDE: usize = 1;

#[derive(Serialize, Deserialize)]
struct Saved {
    /// `ABM_GETSTATE` ile okunan özgün durum (bit 0 = otomatik gizle).
    original_state: usize,
}

/// Yalnızca explorer'a ait çubuk pencereleri. (Tray modülü de uygulamaların
/// tepsi ikonlarını alabilmek için `Shell_TrayWnd` sınıfında bir pencere açar;
/// o pencere burada sayılmamalı.)
pub fn windows() -> Vec<HWND> {
    CLASSES.iter().flat_map(|c| win::top_windows_of_class(c)).filter(|&h| win::is_explorer_window(h)).collect()
}

pub fn is_taskbar(hwnd: HWND) -> bool {
    let class = win::class_name(hwnd);
    CLASSES.contains(&class.as_str()) && win::is_explorer_window(hwnd)
}

/// Bu oturum (veya çökmüş bir önceki oturum) sistemi değiştirmiş ve geri almamış mı?
pub fn hidden_by_us() -> bool {
    paths::state_file().exists()
}

fn appbar_data() -> APPBARDATA {
    APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        hWnd: win::explorer_tray().unwrap_or_default(),
        ..Default::default()
    }
}

fn get_state() -> usize {
    let mut d = appbar_data();
    unsafe { SHAppBarMessage(ABM_GETSTATE, &mut d) }
}

fn set_state(state: usize) {
    let mut d = appbar_data();
    d.lParam = LPARAM(state as isize);
    unsafe {
        SHAppBarMessage(ABM_SETSTATE, &mut d);
    }
}

fn set_visible(visible: bool) {
    for h in windows() {
        unsafe {
            let _ = ShowWindow(h, if visible { SW_SHOWNA } else { SW_HIDE });
        }
    }
}

pub fn hide() {
    // Explorer henüz çubuğu açmamışsa `ABM_GETSTATE` 0 döner ve kullanıcının
    // "otomatik gizle" ayarı yanlış kaydedilirdi. Ertele: explorer çubuğu
    // oluşturunca (TaskbarCreated) `hide` yeniden çağrılır.
    if win::explorer_tray().is_none() {
        info!("explorer görev çubuğu henüz yok, gizleme erteleniyor");
        return;
    }
    if !hidden_by_us() {
        let saved = Saved { original_state: get_state() };
        let path = paths::state_file();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(&path, serde_json::to_vec_pretty(&saved).unwrap_or_default()) {
            // Geri alma bilgisini kaydedemiyorsak sistemi değiştirmeyelim.
            warn!("durum dosyası yazılamadı, görev çubuğu gizlenmiyor: {e}");
            return;
        }
        info!("özgün görev çubuğu durumu kaydedildi: {}", saved.original_state);
    }
    let state = get_state();
    if state & ABS_AUTOHIDE == 0 {
        set_state(state | ABS_AUTOHIDE);
    }
    set_visible(false);
}

/// Explorer'ın kendiliğinden geri gösterdiği çubuğu yeniden gizler (ayar değiştirmeden).
pub fn rehide() {
    set_visible(false);
}

pub fn restore() {
    let saved: Option<Saved> =
        std::fs::read(paths::state_file()).ok().and_then(|b| serde_json::from_slice(&b).ok());
    if let Some(saved) = &saved {
        set_state(saved.original_state);
    }
    set_visible(true);
    let _ = std::fs::remove_file(paths::state_file());
    info!("görev çubuğu geri getirildi (özgün durum: {:?})", saved.map(|s| s.original_state));
}
