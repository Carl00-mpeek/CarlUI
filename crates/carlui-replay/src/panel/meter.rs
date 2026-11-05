//! Bu modül "kayıt kapalıyken mikrofonumun ve bilgisayar sesinin düzeyini
//! nasıl görürüm?" sorununu çözüyor: paneldeki "Test" düğmesi açıkken iki
//! kaynağı ayrı bir iş parçacığında dinler ve tepe değerlerini (ayarlanan
//! düzeyle çarpılmış) karıştırıcının kullandığı göstergelere yazar. Kayda
//! hiçbir şey yazılmaz; panel kapanınca ya da test bitince aygıtlar bırakılır.

use crate::{audio::Loopback, mixer};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

static RUN: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn on() -> bool {
    RUN.load(Ordering::SeqCst)
}

pub fn start() {
    RUN.store(true, Ordering::SeqCst);
    if ACTIVE.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = std::thread::Builder::new().name("replay-meter".into()).spawn(|| unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        let mut src: [(bool, Option<Loopback>, Instant); 2] = [(false, None, Instant::now() - Duration::from_secs(5)), (true, None, Instant::now() - Duration::from_secs(5))];
        while RUN.load(Ordering::SeqCst) {
            for (mic, cap, retry) in src.iter_mut() {
                if cap.is_none() && retry.elapsed() >= Duration::from_secs(2) {
                    *retry = Instant::now();
                    *cap = Loopback::open(*mic).ok();
                }
                let (gain, level) = if *mic { (&mixer::GAIN_MIC, &mixer::LEVEL_MIC) } else { (&mixer::GAIN_SYS, &mixer::LEVEL_SYS) };
                if let Some(c) = cap.as_mut() {
                    let g = mixer::gain(gain);
                    if c.read(|pcm, _| {
                        level.fetch_max(mixer::peak(pcm, g), Ordering::Relaxed);
                    })
                    .is_err()
                    {
                        *cap = None;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        ACTIVE.store(false, Ordering::SeqCst);
    });
}

pub fn stop() {
    RUN.store(false, Ordering::SeqCst);
}
