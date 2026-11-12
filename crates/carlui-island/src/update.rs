//! Bu modül "yeni sürüm çıktığını nereden bileceğim, nasıl kuracağım?"
//! sorununu çözüyor: açılıştan bir dakika sonra ve sonra günde bir kez
//! GitHub'daki son sürüm (`releases/latest`) sorulur. Kurulu sürümden yeniyse
//! adada Güncelle / Sonra düğmeli bildirim çıkar (aynı sürüm için bir kez).
//! Güncelle'ye basınca `CarlUI-Setup.exe` indirilir, SHA-256'sı GitHub'ın
//! bildirdiğiyle karşılaştırılır ve kurucu `--update` ile açılır: penceresinde
//! ilerleme görünür, CarlUI'yi kapatır, dosyaları yeniler, ayarlara dokunmadan
//! CarlUI'yi yeniden açar ve sonunda Yenilikler'i gösterir.
//! Kendiliğinden hiçbir şey kurulmaz. Ayarlar → Güncelleme'den denetim
//! kapatılabilir ya da elle yapılabilir.
//!
//! Ağ, indirme ve SHA-256 doğrulaması `carlui_core::update`'tedir (ayarlardaki
//! Güncelleme sayfası da aynısını kullanır); burada yalnızca bildirim akışı var.

use crate::notify::Note;
use carlui_core::update as core;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::PostMessageW,
};

pub use core::Release;

fn seen_file() -> std::path::PathBuf {
    carlui_core::paths::data_dir().join("update-seen.txt")
}

fn push(hwnd: isize, msg: u32, notes: &Mutex<VecDeque<Note>>, n: Note) {
    if let Ok(mut q) = notes.lock() {
        q.push_back(n);
    }
    unsafe {
        let _ = PostMessageW(HWND(hwnd as _), msg, WPARAM(0), LPARAM(0));
    }
}

fn check(hwnd: isize, msg: u32, notes: &Mutex<VecDeque<Note>>) {
    if !carlui_core::config::load().general.check_updates {
        return;
    }
    let r = match core::latest(core::API) {
        Ok(r) => r,
        Err(e) => {
            carlui_core::info!("güncelleme denetimi: yanıt alınamadı ({e})");
            return;
        }
    };
    core::record_check();
    let current = core::current();
    if !core::newer(&r.tag, &current) || std::fs::read_to_string(seen_file()).is_ok_and(|s| s.trim() == r.tag) {
        return;
    }
    carlui_core::info!("yeni sürüm: {} (kurulu {current})", r.tag);
    let _ = std::fs::write(seen_file(), &r.tag);
    let ver = r.version().to_string();
    // Kurucu ya da özeti yoksa kurulamaz: eskisi gibi sayfayı aç.
    let ready = r.installable();
    push(
        hwnd,
        msg,
        notes,
        Note {
            app: "CarlUI".into(),
            title: t!(format!("CarlUI {ver} çıktı"), format!("CarlUI {ver} is out")),
            body: if ready { t!("Yeni sürüm kurulmaya hazır", "A new version is ready to install") } else { t!("İndirme sayfasını açmak için tıkla", "Click to open the download page") }.into(),
            file: (!ready).then(|| r.page.clone()),
            update: ready.then_some(r),
            ..Default::default()
        },
    );
}

/// Denetimi arka planda başlatır; yeni sürüm `notes`'a eklenip `msg` gönderilir.
pub fn start(hwnd: isize, msg: u32, notes: Arc<Mutex<VecDeque<Note>>>) {
    std::thread::Builder::new()
        .name("update".into())
        .spawn(move || {
            let _ = unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED) };
            core::cleanup_old();
            std::thread::sleep(Duration::from_secs(60));
            loop {
                check(hwnd, msg, &notes);
                std::thread::sleep(Duration::from_secs(24 * 3600));
            }
        })
        .ok();
}

/// Güncelle'ye basınca: arka planda indirip kurucuyu güncelleme kipinde açar.
pub fn apply(hwnd: isize, msg: u32, notes: Arc<Mutex<VecDeque<Note>>>, r: Release) {
    let ver = r.version().to_string();
    push(hwnd, msg, &notes, Note { app: "CarlUI".into(), title: t!(format!("CarlUI {ver} indiriliyor"), format!("Downloading CarlUI {ver}")), body: t!("Kurulunca CarlUI yeniden açılacak", "CarlUI will restart when it's installed").into(), ..Default::default() });
    std::thread::Builder::new()
        .name("update-apply".into())
        .spawn(move || {
            let _ = unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED) };
            if let Err(e) = core::install(&r, |_| {}) {
                carlui_core::warn!("güncelleme kurulamadı: {e}");
                push(hwnd, msg, &notes, Note { app: "CarlUI".into(), title: t!("Güncelleme kurulamadı", "Update failed").into(), body: format!("{e} · {}", t!("sayfayı açmak için tıkla", "click to open the page")), file: Some(r.page.clone()), ..Default::default() });
            }
        })
        .ok();
}
