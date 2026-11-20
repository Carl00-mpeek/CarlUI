//! Bu modül "bir modül çökerse diğerleri çalışsın, çöken geri gelsin" sorununu
//! çözüyor. Her modül ayrı bir süreçtir; burada her biri için bir bekçi iş
//! parçacığı süreci başlatır ve bitmesini bekler. Hata koduyla biterse kısa bir
//! beklemeyle yeniden başlatır; 60 saniyede 5 kez çökerse vazgeçip log'a yazar.
//!
//! Modüller bir Job Object'e konur: ana süreç ne şekilde ölürse ölsün Windows
//! modülleri de kapatır, ortada sahipsiz pencere kalmaz. Modüllerin açtığı
//! kullanıcı uygulamaları ise job dışında kalır (sessiz ayrılma). Koruyucu süreç ise
//! bilerek job dışında tutulur; ana süreç öldüğünde görev çubuğunu o geri getirir.

use carlui_core::{crash, error, info, paths, warn};
use std::{
    collections::VecDeque,
    os::windows::io::AsRawHandle,
    path::PathBuf,
    process::{Child, Command},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use windows::Win32::{Foundation::*, System::JobObjects::*};

/// Bir modül çöktü: (modül adı, rapor dosyası, yeniden başlatma durduruldu mu).
pub type CrashFn = Arc<dyn Fn(&'static str, PathBuf, bool) + Send + Sync>;

/// Aynı modül için çökme bildirimi en sık bu aralıkla gösterilir; rapor her seferinde yazılır.
const NOTE_EVERY: Duration = Duration::from_secs(10 * 60);

/// Modülün `log::init`'e verdiği ad (çoğunda modül adıyla aynı).
fn log_name(module: &str) -> &str {
    match module {
        "assist" => "convert",
        m => m,
    }
}

pub struct Supervisor {
    job: isize,
    pub shutting_down: Arc<AtomicBool>,
    children: Arc<Mutex<Vec<(String, u32)>>>,
    on_crash: CrashFn,
}

impl Supervisor {
    pub fn new(on_crash: CrashFn) -> Self {
        let job = unsafe {
            let job = CreateJobObjectW(None, None).unwrap_or_default();
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            // SILENT_BREAKAWAY: modüllerin açtığı uygulamalar (dock, Win menüsü,
            // arama) job'a girmez; CarlUI yeniden başlayınca onlar kapanmaz.
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK;
            let _ = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            job.0 as isize
        };
        Self { job, shutting_down: Arc::new(AtomicBool::new(false)), children: Arc::default(), on_crash }
    }

    /// `exe` adlı modülü başlatır ve çöktükçe yeniden başlatır.
    pub fn run_module(&self, name: &'static str, exe: &'static str) {
        self.run_module_with(name, exe, &[]);
    }

    /// `run_module` gibi, süreci verilen argümanlarla başlatır.
    pub fn run_module_with(&self, name: &'static str, exe: &'static str, args: &'static [&'static str]) {
        let path = paths::sibling_exe(exe);
        let job = self.job;
        let shutting = self.shutting_down.clone();
        let children = self.children.clone();
        let on_crash = self.on_crash.clone();
        std::thread::Builder::new()
            .name(format!("mod-{name}"))
            .spawn(move || {
                let mut crashes: VecDeque<Instant> = VecDeque::new();
                let mut noted: Option<Instant> = None;
                while !shutting.load(Ordering::SeqCst) {
                    let mut child: Child = match Command::new(&path).args(args).spawn() {
                        Ok(c) => c,
                        Err(e) => {
                            error!("{name} başlatılamadı ({}): {e}", path.display());
                            return;
                        }
                    };
                    unsafe {
                        let _ = AssignProcessToJobObject(HANDLE(job as _), HANDLE(child.as_raw_handle()));
                    }
                    children.lock().unwrap_or_else(|e| e.into_inner()).push((name.to_string(), child.id()));
                    info!("{name} başlatıldı (pid {})", child.id());

                    let status = child.wait();
                    let pid = child.id();
                    children.lock().unwrap_or_else(|e| e.into_inner()).retain(|(_, p)| *p != pid);
                    if shutting.load(Ordering::SeqCst) {
                        return;
                    }
                    match status {
                        Ok(s) if s.code() == Some(carlui_core::RELOAD_EXIT) => {
                            info!("{name} yeniden yükleniyor");
                            continue;
                        }
                        Ok(s) if s.success() => {
                            info!("{name} kendi isteğiyle kapandı");
                            return;
                        }
                        ref other => warn!("{name} beklenmedik şekilde kapandı ({other:?}), yeniden başlatılıyor"),
                    }
                    let now = Instant::now();
                    crashes.push_back(now);
                    while crashes.front().is_some_and(|t| now.duration_since(*t) > Duration::from_secs(60)) {
                        crashes.pop_front();
                    }
                    let gave_up = crashes.len() >= 5;
                    let code = status.as_ref().ok().and_then(|s| s.code());
                    if crash::is_crash(code) {
                        match crash::write(name, log_name(name), pid, code) {
                            Some(path) => {
                                info!("{name} çökme raporu: {}", path.display());
                                // Durdurma bildirimi her zaman; öbürü seyrek.
                                if gave_up || noted.is_none_or(|t| now.duration_since(t) >= NOTE_EVERY) {
                                    noted = Some(now);
                                    let cb = on_crash.clone();
                                    // Çöken ada olabilir: bildirim, yeniden başladıktan sonra gitsin.
                                    std::thread::spawn(move || {
                                        std::thread::sleep(Duration::from_secs(4));
                                        cb(name, path, gave_up);
                                    });
                                }
                            }
                            None => warn!("{name} çökme raporu yazılamadı"),
                        }
                    }
                    if gave_up {
                        error!("{name} 60 saniyede 5 kez çöktü; yeniden başlatma durduruldu");
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(400 * crashes.len() as u64));
                }
            })
            .ok();
    }

    /// Koruyucu süreci başlatır ve öldürülürse yeniden başlatır.
    pub fn run_guard(&self) {
        let shutting = self.shutting_down.clone();
        let me = std::env::current_exe().unwrap_or_default();
        let pid = std::process::id().to_string();
        std::thread::Builder::new()
            .name("guard-watch".into())
            .spawn(move || {
                while !shutting.load(Ordering::SeqCst) {
                    match Command::new(&me).args(["--guard", &pid]).spawn() {
                        Ok(mut g) => {
                            let _ = g.wait();
                            if !shutting.load(Ordering::SeqCst) {
                                warn!("koruyucu süreç kapandı, yeniden başlatılıyor");
                            }
                        }
                        Err(e) => error!("koruyucu başlatılamadı: {e}"),
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
            })
            .ok();
    }

    /// Modüllerin kapanması için kısa süre bekler; kalanları job ile kapatır.
    pub fn shutdown(&self, grace: Duration) {
        self.shutting_down.store(true, Ordering::SeqCst);
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline && !self.children.lock().unwrap_or_else(|e| e.into_inner()).is_empty() {
            std::thread::sleep(Duration::from_millis(50));
        }
        unsafe {
            let _ = TerminateJobObject(HANDLE(self.job as _), 0);
        }
    }
}
