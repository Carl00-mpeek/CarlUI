//! Ekran yenilemesine bağlı kare saati. Kar topu oyunu ve küreme aracı gibi
//! sürekli hareketlerde `SetTimer` yetmez: en az ~16 ms'lik kaba adımları ve
//! titremesi yüzünden kareler düzensiz gelir, hareket takılır. Bu saat ayrı
//! bir iş parçacığında `DwmFlush` ile her ekran karesini bekler ve dock
//! penceresine `WM_APP_FRAME` gönderir. Kuyrukta bekleyen kare varken yenisi
//! gönderilmez (UI iş parçacığı gecikirse mesajlar birikmez).
//!
//! Gönderilen kare işlenmeden kaybolabilir: dock menüsü açıkken (modal döngü)
//! uygulama meşguldür, `WM_APP_FRAME` canlı katmana ulaşmaz ve `consumed`
//! çağrılmaz. Eskiden bu durumda saat sonsuza dek "kuyrukta kare var" sanıp
//! durur, kar efektlerinin hepsi (yağış, birikinti, toplar, küreme aracı)
//! donup kalırdı. Şimdi bekleyen kare `STALE` süresince işlenmezse kayıp
//! sayılır ve yeniden gönderilir.

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::*,
    Graphics::Dwm::DwmFlush,
    UI::WindowsAndMessaging::{PostMessageW, WM_APP},
};

/// Ekran karesi geldi (dock penceresine).
pub const WM_APP_FRAME: u32 = WM_APP + 57;

/// Bu süre içinde işlenmeyen kare mesajı kayıp sayılır.
const STALE: Duration = Duration::from_millis(250);

/// Yeni kare gönderilmeli mi? Kuyruk boşsa evet; doluysa yalnızca son
/// gönderim `STALE`den eskiyse (mesaj işlenmeden kaybolmuş).
fn should_post(queued: bool, posted: Option<Instant>, now: Instant) -> bool {
    !queued || posted.is_none_or(|t| now.saturating_duration_since(t) >= STALE)
}

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
                let mut posted: Option<Instant> = None;
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
                    let now = Instant::now();
                    if sh.run.load(Ordering::Acquire) && should_post(sh.queued.load(Ordering::Acquire), posted, now) {
                        sh.queued.store(true, Ordering::Release);
                        posted = Some(now);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bos_kuyrukta_kare_gonderilir() {
        let now = Instant::now();
        assert!(should_post(false, None, now));
        assert!(should_post(false, Some(now), now));
    }

    #[test]
    fn islenmeyi_bekleyen_kare_varken_yenisi_gonderilmez() {
        let now = Instant::now();
        assert!(!should_post(true, Some(now), now));
        assert!(!should_post(true, Some(now), now + STALE / 2));
    }

    #[test]
    fn menu_acikken_kaybolan_kare_saati_durdurmaz() {
        // Küreme aracı yoldayken dock menüsü açıldı: kare mesajı işlenmeden
        // kayboldu, `consumed` hiç gelmedi. Saat sonsuza dek beklememeli.
        let sent = Instant::now();
        assert!(should_post(true, Some(sent), sent + STALE));
        assert!(should_post(true, Some(sent), sent + Duration::from_secs(5)));
        assert!(should_post(true, None, sent));
    }
}
