//! Ekran yenilemesine bağlı kare saati. Kar topu oyunu ve küreme aracı gibi
//! sürekli hareketlerde `SetTimer` yetmez: en az ~16 ms'lik kaba adımları ve
//! titremesi yüzünden kareler düzensiz gelir, hareket takılır. Bu saat ayrı
//! bir iş parçacığında `DwmFlush` ile her ekran karesini bekler ve dock
//! penceresine `WM_APP_FRAME` gönderir. Kuyrukta bekleyen kare varken yenisi
//! gönderilmez (UI iş parçacığı gecikirse mesajlar birikmez).

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};
use windows::Win32::{
    Foundation::*,
    Graphics::Dwm::DwmFlush,
    UI::WindowsAndMessaging::{PostMessageW, WM_APP},
};

/// Ekran karesi geldi (dock penceresine).
pub const WM_APP_FRAME: u32 = WM_APP + 57;

struct Shared {
    run: AtomicBool,
    queued: AtomicBool,
    quit: AtomicBool,
}

pub struct Pacer {
    shared: Arc<Shared>,
    thread: thread::Thread,
}

impl Pacer {
    pub fn new(hwnd: HWND) -> Self {
        let shared = Arc::new(Shared { run: AtomicBool::new(false), queued: AtomicBool::new(false), quit: AtomicBool::new(false) });
        let sh = shared.clone();
        let raw = hwnd.0 as isize;
        let handle = thread::Builder::new()
            .name("carlui-live-pacer".into())
            .spawn(move || {
                let hwnd = HWND(raw as _);
                loop {
                    if sh.quit.load(Ordering::Acquire) {
                        return;
                    }
                    if !sh.run.load(Ordering::Acquire) {
                        thread::park();
                        continue;
                    }
                    // Kompozisyon kapalıysa (olmamalı) düz bekleme.
                    if unsafe { DwmFlush() }.is_err() {
                        thread::sleep(Duration::from_millis(16));
                    }
                    if sh.run.load(Ordering::Acquire) && !sh.queued.swap(true, Ordering::AcqRel) {
                        unsafe {
                            if PostMessageW(hwnd, WM_APP_FRAME, WPARAM(0), LPARAM(0)).is_err() {
                                sh.queued.store(false, Ordering::Release);
                            }
                        }
                    }
                }
            })
            .expect("kare saati iş parçacığı");
        Self { shared, thread: handle.thread().clone() }
    }

    /// Kareler istenir/istenmez.
    pub fn set(&self, on: bool) {
        if self.shared.run.swap(on, Ordering::AcqRel) != on && on {
            self.thread.unpark();
        }
    }

    pub fn running(&self) -> bool {
        self.shared.run.load(Ordering::Acquire)
    }

    /// `WM_APP_FRAME` işlendi: bir sonraki gönderilebilir.
    pub fn consumed(&self) {
        self.shared.queued.store(false, Ordering::Release);
    }
}

impl Drop for Pacer {
    fn drop(&mut self) {
        self.shared.run.store(false, Ordering::Release);
        self.shared.quit.store(true, Ordering::Release);
        self.thread.unpark();
    }
}
