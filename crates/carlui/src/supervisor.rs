//! Bu modül "bir modül çökerse diğerleri çalışsın, çöken geri gelsin" sorununu
//! çözüyor. Her modül ayrı bir süreçtir; burada her biri için bir bekçi iş
//! parçacığı süreci başlatır ve bitmesini bekler. Hata koduyla biterse kısa bir
//! beklemeyle yeniden başlatır; 60 saniyede 5 kez çökerse vazgeçip log'a yazar.
//!
//! Modüller bir Job Object'e konur: ana süreç ne şekilde ölürse ölsün Windows
//! modülleri de kapatır, ortada sahipsiz pencere kalmaz. Modüllerin açtığı
//! kullanıcı uygulamaları ise job dışında kalır (sessiz ayrılma). Koruyucu süreç ise
//! bilerek job dışında tutulur; ana süreç öldüğünde görev çubuğunu o geri getirir.

use carlui_core::{error, info, paths, warn};
use std::{
    collections::VecDeque,
    os::windows::io::AsRawHandle,
    process::{Child, Command},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use windows::Win32::{Foundation::*, System::JobObjects::*};

pub struct Supervisor {
    job: isize,
    pub shutting_down: Arc<AtomicBool>,
    children: Arc<Mutex<Vec<(String, u32)>>>,
}

impl Supervisor {
    pub fn new() -> Self {
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
        Self { job, shutting_down: Arc::new(AtomicBool::new(false)), children: Arc::default() }
    }

    /// `exe` adlı modülü başlatır ve çöktükçe yeniden başlatır.
    pub fn run_module(&self, name: &'static str, exe: &'static str) {
        let path = paths::sibling_exe(exe);
        let job = self.job;
        let shutting = self.shutting_down.clone();
        let children = self.children.clone();
        std::thread::Builder::new()
            .name(format!("mod-{name}"))
            .spawn(move || {
                let mut crashes: VecDeque<Instant> = VecDeque::new();
                while !shutting.load(Ordering::SeqCst) {
                    let mut child: Child = match Command::new(&path).spawn() {
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
                    children.lock().unwrap_or_else(|e| e.into_inner()).retain(|(_, pid)| *pid != child.id());
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
                        other => warn!("{name} beklenmedik şekilde kapandı ({other:?}), yeniden başlatılıyor"),
                    }
                    let now = Instant::now();
                    crashes.push_back(now);
                    while crashes.front().is_some_and(|t| now.duration_since(*t) > Duration::from_secs(60)) {
                        crashes.pop_front();
                    }
                    if crashes.len() >= 5 {
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
