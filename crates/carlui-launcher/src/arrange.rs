//! Bu modül "Discord ve Steam'i 1. ekranda böl" sorununu çözüyor. Kapalı uygulamalar
//! açılır, ana pencereleri gelince seçilen ekranın çalışma alanı eşit sütunlara
//! bölünür. Ekran 1 ana ekrandır, öbürleri soldan sağa sayılır; ekran 0 "söylenmedi"
//! demektir ve öndeki pencerenin ekranı kullanılır. Pencereler odak çalmadan yerleşir.

use crate::{
    actions,
    index::{self, App},
    search::Act,
};
use carlui_core::{
    layouts::{self, Spot},
    win::{self, Monitor},
};
use std::time::{Duration, Instant};
use windows::Win32::{
    Foundation::*,
    Graphics::{
        Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS},
        Gdi::{MonitorFromWindow, MONITOR_DEFAULTTOPRIMARY},
    },
    UI::WindowsAndMessaging::*,
};

/// Discord ve Steam açılışta güncelleme yapabilir.
const WAIT_MAX: Duration = Duration::from_secs(25);
const WAIT_STEP: Duration = Duration::from_millis(300);
/// Bundan küçük pencere açılış ekranıdır (Discord güncelleyicisi, Steam logosu).
const MIN_W: i32 = 420;
const MIN_H: i32 = 300;

fn numbered() -> Vec<Monitor> {
    let mut list = win::monitors();
    list.sort_by_key(|m| (!m.primary, m.work.left, m.work.top));
    list
}

/// Söylenen numarada ekran var mı?
pub fn has_screen(n: u8) -> bool {
    n as usize <= numbered().len()
}

fn work_of(n: u8) -> Option<RECT> {
    let list = numbered();
    if n > 0 {
        return list.get(n as usize - 1).map(|m| m.work);
    }
    let front = unsafe { MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTOPRIMARY) };
    list.iter().find(|m| m.handle == front).or(list.first()).map(|m| m.work)
}

/// Alanı eşit sütunlara böler; kalan pikseller son sütuna gider.
fn columns(r: RECT, n: usize) -> Vec<[i32; 4]> {
    let w = (r.right - r.left) / n.max(1) as i32;
    (0..n as i32).map(|i| [r.left + i * w, r.top, if i + 1 == n as i32 { r.right } else { r.left + (i + 1) * w }, r.bottom]).collect()
}

fn main_window(app: &App) -> Option<isize> {
    let big = |h: isize| unsafe {
        let mut r = RECT::default();
        IsIconic(HWND(h as _)).as_bool() || (GetWindowRect(HWND(h as _), &mut r).is_ok() && r.right - r.left >= MIN_W && r.bottom - r.top >= MIN_H)
    };
    index::running().iter().filter(|w| index::owns(app, &w.exe, &w.aumid)).map(|w| w.hwnd).find(|h| big(*h))
}

fn launch(app: &App) {
    actions::run(&Act::App { launch: app.launch_path(), hwnd: None });
}

/// Uygulamaların ana pencereleri, sırayla. Süresi dolanın yeri `None` kalır.
fn windows_of(apps: &[App]) -> Vec<Option<isize>> {
    let mut found: Vec<Option<isize>> = apps.iter().map(main_window).collect();
    for (a, _) in apps.iter().zip(&found).filter(|(_, w)| w.is_none()) {
        launch(a);
    }
    let t0 = Instant::now();
    while found.iter().any(Option::is_none) && t0.elapsed() < WAIT_MAX {
        std::thread::sleep(WAIT_STEP);
        for (a, w) in apps.iter().zip(found.iter_mut()).filter(|(_, w)| w.is_none()) {
            *w = main_window(a);
        }
    }
    found
}

/// Pencereyi görünen kenarları `r` olacak şekilde koyar. Windows dikdörtgeni
/// görünmeyen gölge payını da sayar; o pay düşülmezse sütunlar arasında boşluk kalır.
fn put(hwnd: isize, r: [i32; 4]) {
    let spot = Spot { rect: r, show: SW_SHOWNORMAL.0 as u32, flags: 0, min: [-1, -1], max: [-1, -1] };
    if !layouts::place(hwnd, &spot) {
        return;
    }
    let h = HWND(hwnd as _);
    // Ekranlar arası geçişte DPI değişip boyut yeniden hesaplanır; ikinci tur oturtur.
    for _ in 0..2 {
        let (mut all, mut seen) = (RECT::default(), RECT::default());
        unsafe {
            if GetWindowRect(h, &mut all).is_err() || DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, &mut seen as *mut _ as _, std::mem::size_of::<RECT>() as u32).is_err() {
                return;
            }
            let (l, t) = (seen.left - all.left, seen.top - all.top);
            let (rt, b) = (all.right - seen.right, all.bottom - seen.bottom);
            let _ = SetWindowPos(h, HWND_TOP, r[0] - l, r[1] - t, r[2] - r[0] + l + rt, r[3] - r[1] + t + b, SWP_NOACTIVATE);
        }
    }
}

/// Uygulamaları ekranda yan yana dizer; açılamayanın sütunu boş kalır.
pub fn split(apps: &[App], screen: u8) {
    let Some(work) = work_of(screen) else { return };
    for ((w, r), app) in windows_of(apps).into_iter().zip(columns(work, apps.len())).zip(apps) {
        match w {
            Some(w) => put(w, r),
            None => carlui_core::warn!("asistan: {} penceresi gelmedi", app.name),
        }
    }
}

/// Pencereyi boyutuyla ekranın ortasına taşır; ekranı kaplıyorsa orada da kaplar.
pub fn move_to(app: &App, screen: u8) {
    let (Some(work), Some(Some(w))) = (work_of(screen), windows_of(std::slice::from_ref(app)).pop()) else { return };
    let h = HWND(w as _);
    let full = unsafe { IsZoomed(h) }.as_bool();
    let (aw, ah) = (work.right - work.left, work.bottom - work.top);
    let mut now = RECT::default();
    let _ = unsafe { GetWindowRect(h, &mut now) };
    let (mw, mh) = ((now.right - now.left).clamp(MIN_W, aw), (now.bottom - now.top).clamp(MIN_H, ah));
    let (x, y) = (work.left + (aw - mw) / 2, work.top + (ah - mh) / 2);
    put(w, [x, y, x + mw, y + mh]);
    if full {
        let _ = unsafe { ShowWindow(h, SW_SHOWMAXIMIZED) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_evenly() {
        let r = RECT { left: 1920, top: 0, right: 3840, bottom: 1040 };
        assert_eq!(columns(r, 2), vec![[1920, 0, 2880, 1040], [2880, 0, 3840, 1040]]);
        let c = columns(RECT { left: 0, top: 0, right: 1000, bottom: 10 }, 3);
        assert_eq!(c, vec![[0, 0, 333, 10], [333, 0, 666, 10], [666, 0, 1000, 10]]);
    }
}
