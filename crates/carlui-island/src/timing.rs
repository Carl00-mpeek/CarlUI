//! Bu modül "yemek/çay/Pomodoro için hızlıca zamanlayıcı kur ve ekranda gör"
//! sorununu şu yöntemle çözüyor: Spotlight'tan gelen komutla ("5dk", "25dk
//! çalış", "kronometre") adanın saatinin yerinde kalan ya da geçen süre
//! görünür ("⏱ 04:32 · çalış"); saat gizli ayarlıysa bile ada sayım boyunca
//! görünür kalır. Süre dolunca bildirim + sistem sesi gelir. Saniyede bir
//! yalnızca küçük hap yeniden çizilir.

use crate::*;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC};

pub(crate) const TIMER_COUNT: usize = 30;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cmd {
    /// (saniye, etiket)
    Start(u32, String),
    Stopwatch,
    Stop,
}

pub(crate) struct Timer {
    start: Instant,
    /// Geri sayım süresi; None ise kronometre.
    secs: Option<u32>,
    label: String,
}

fn clock(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

impl Timer {
    /// Kalan (geri sayım) ya da geçen (kronometre) süre metni.
    pub(crate) fn text(&self) -> String {
        let el = self.start.elapsed().as_secs();
        let t = match self.secs {
            Some(total) => clock((total as u64).saturating_sub(el)),
            None => clock(el),
        };
        if self.label.is_empty() { format!("⏱ {t}") } else { format!("⏱ {t} · {}", self.label) }
    }

    fn done(&self) -> bool {
        self.secs.is_some_and(|s| self.start.elapsed().as_secs() >= s as u64)
    }
}

impl App {
    pub(crate) fn timer_cmd(&mut self, cmd: Cmd) {
        self.timer = match cmd {
            Cmd::Start(secs, label) => {
                info!("zamanlayıcı: {secs} sn {label}");
                Some(Timer { start: Instant::now(), secs: Some(secs), label })
            }
            Cmd::Stopwatch => {
                info!("kronometre başladı");
                Some(Timer { start: Instant::now(), secs: None, label: String::new() })
            }
            Cmd::Stop => {
                info!("zamanlayıcı durduruldu");
                None
            }
        };
        unsafe {
            if self.timer.is_some() {
                SetTimer(self.island.hwnd, TIMER_COUNT, 1000, None);
            } else {
                let _ = KillTimer(self.island.hwnd, TIMER_COUNT);
            }
        }
        self.refresh(true);
    }

    /// Saniyelik tık: hap yenilenir; süre dolduysa bildirim.
    pub(crate) fn timer_tick(&mut self) {
        let Some(t) = &self.timer else {
            unsafe {
                let _ = KillTimer(self.island.hwnd, TIMER_COUNT);
            }
            return;
        };
        if t.done() {
            let label = t.label.clone();
            let total = t.secs.unwrap_or(0);
            self.timer_cmd(Cmd::Stop);
            unsafe {
                let _ = PlaySoundW(w!("SystemNotification"), None, SND_ALIAS | SND_ASYNC);
            }
            let title = if label.is_empty() { "Süre doldu".to_string() } else { format!("Süre doldu: {label}") };
            let body = format!("{} zamanlayıcı bitti", carlui_launcher::timer::human(total));
            self.push_note(Note { app: "Zamanlayıcı".into(), aumid: String::new(), title, body, ..Default::default() });
            return;
        }
        // Arama/pano açıkken hap görünmüyor; boşuna yeniden kurma.
        if matches!(self.island.view, View::Clock | View::Media | View::Date) {
            self.refresh(true);
        }
    }
}
