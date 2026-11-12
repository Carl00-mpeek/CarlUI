//! `Live`'ın gerçek hava bağlantısı: son havayı dosyadan okur (ağa çıkmaz),
//! sahneyi buna göre çözer ve sahne değişimlerinde yumuşak geçişi yürütür.

use super::{
    fade::{Fade, FADE_FRAME_MS},
    sky::{self, Resolved},
    sky_weather, weather_poll, Live,
};
use carlui_core::weather::state;
use carlui_ui::gfx::Gfx;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HWND;

/// Hava kaydına en sık bu aralıkla bakılır (UI iş parçacığında küçük bir dosya).
const RECHECK: Duration = Duration::from_secs(20);

impl Live {
    /// Özellik açıksa (ve hata ayıklama değeri yoksa) yoklayıcıyı başlatır.
    pub(super) fn start_poller(&mut self, hwnd: HWND) {
        let w = &self.cfg.weather;
        let debug = carlui_core::weather::debug_override().is_some();
        self.poll = (w.active() && !debug).then(|| weather_poll::start(w.lat, w.lon, hwnd));
    }

    /// Şimdiki sahne: gerçek hava varsa o, yoksa elle seçilen sahne / saat kipi.
    pub(super) fn resolve_scene(&self, hour: u32, minute: u32) -> Option<Resolved> {
        match self.wx {
            Some(w) => sky_weather::resolve_weather(w, self.cfg.weather.day_night),
            None => sky::resolve(self.cfg.scene, hour, minute),
        }
    }

    /// Hava kaydını yeniden okur (`force` yoksa en çok 20 sn'de bir). Değiştiyse
    /// sahneyi yeniden kurar ve `true` döner.
    pub(super) fn check_weather(&mut self, gfx: &Gfx, dock: &crate::dock::Dock, force: bool) -> bool {
        if !force && self.wx_checked.elapsed() < RECHECK {
            return false;
        }
        self.wx_checked = Instant::now();
        let now = state::effective(&self.cfg.weather);
        if now == self.wx {
            return false;
        }
        self.wx = now;
        self.build_scene(gfx, dock);
        true
    }

    /// Geçişleri ilerletir; sürüyorsa `true` (kare aralığı kısaltılmalı).
    pub(super) fn step_fades(&mut self, now: Instant) -> bool {
        let mut busy = false;
        if let Some(f) = self.fade_in {
            if let Some(s) = self.scene.as_ref() {
                let _ = s.set_fade(f.up(now));
            }
            if f.done(now) {
                self.fade_in = None;
            } else {
                busy = true;
            }
        }
        if let Some((s, f)) = self.fade_out.as_ref() {
            let _ = s.set_fade(f.down(now));
            if f.done(now) {
                self.fade_out = None;
            } else {
                busy = true;
            }
        }
        if busy {
            // Sahnesiz kalındıysa (yalnız silinme) ya da sahnenin aralığı seyrekse sıklaştır.
            let soon = now + Duration::from_millis(FADE_FRAME_MS);
            if self.scene.is_none() || self.scene_due > soon {
                self.scene_due = soon;
            }
        }
        busy
    }

    /// Geçişleri hemen bitirir (hareket azaltılmış tek kare, duraklama).
    pub(super) fn finish_fades(&mut self) {
        self.fade_in = None;
        self.fade_out = None;
        if let Some(s) = self.scene.as_ref() {
            let _ = s.set_fade(1.0);
        }
    }

    /// Eski sahne silinirken yeni sahne belirsin diye geçişi kurar. Çalışmıyorsa
    /// (duraklı, hareketi azalt, ilk kurulum) eski sahne hemen bırakılır.
    pub(super) fn begin_fade(&mut self, old: Option<super::scene::Scene>) {
        let animate = self.started && self.state == super::pause::State::Running;
        let now = Instant::now();
        // Yarıda kesilen belirme varsa eski sahne o görünürlükten silinmeye başlar.
        let level = self.fade_in.map_or(1.0, |f| f.up(now));
        self.fade_out = None;
        self.fade_in = None;
        if !animate {
            return;
        }
        if let Some(o) = old {
            self.fade_out = Some((o, Fade::new(now, level)));
        }
        if let Some(s) = self.scene.as_ref() {
            let _ = s.set_fade(0.0);
            self.fade_in = Some(Fade::new(now, 0.0));
        }
    }
}
