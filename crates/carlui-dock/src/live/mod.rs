//! Canlı dock: dock üstünde yaşayan evcil hayvan ve dock içinde sahne.
//! Varsayılan kapalı; kapalıyken hiçbir zamanlayıcı, yüzey veya bellek yok
//! (`Live` hiç oluşturulmaz).
//!
//! Tek bir zamanlayıcı (`TIMER`) hem hayvanı hem sahneyi sürer; her tikte
//! ikisinin istediği en yakın an hesaplanıp yeniden kurulur. Uyuyan/duran
//! hayvan ve sahne yoksa zamanlayıcı onlarca saniye sonraya kurulur ya da
//! hiç kurulmaz. Duraklatma nedenleri (gizli dock, tam ekran, kilit, ekran
//! kapalı, pil) zamanlayıcıyı tamamen öldürür; yalnızca Windows mesajları
//! (olaylar) yeniden başlatır.

mod aquarium;
mod creature;
mod dolphin;
mod droplets;
mod droplets_view;
mod fade;
mod gpu;
mod load;
mod mix;
mod pack;
mod particles;
mod pause;
mod perch;
mod pet;
mod power;
mod rng;
mod scene;
mod scene_art;
mod sky;
mod sky_weather;
mod snake;
mod weather_link;
mod weather_poll;
mod window;

pub use load::WM_APP_LIVE;
use weather_poll::WM_APP_WEATHER;

use crate::dock::Dock;
use carlui_core::{info, live::LiveConfig, warn};
use carlui_ui::gfx::Gfx;
use creature::Creature;
use pause::{Signals, State};
use power::Watch;
use scene::Scene;
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::*,
    System::SystemInformation::GetLocalTime,
    UI::WindowsAndMessaging::*,
};

/// Canlı dock zamanlayıcısı (dock penceresinde; diğer kimliklerle çakışmaz).
pub const TIMER: usize = 41;
const WM_MOUSELEAVE: u32 = 0x02A3;
/// Dock geri kayarken (otomatik gizleme / tam ekrandan dönüş) hayvan dock
/// yerine oturmadan görünmesin: bu kadar süre daha gizli sayılır. Dock'un
/// geri gelme yayı (45 ms periyot, 0.75 sönüm) ~150 ms'de oturur; 350 ms
/// canlı denemede dock yerindeyken hayvanın gözle görülür geç gelmesine yol açtı.
const SETTLE: Duration = Duration::from_millis(170);

/// `CARLUI_LIVE_DEBUG` açıkken dakikada bir kare sayısı ve süre günlüğe yazılır.
struct Stats {
    since: Instant,
    frames: u32,
    total: Duration,
    max: Duration,
}

impl Stats {
    fn add(&mut self, d: Duration) {
        self.frames += 1;
        self.total += d;
        self.max = self.max.max(d);
        if self.since.elapsed() >= Duration::from_secs(60) {
            let avg = self.total.as_secs_f64() * 1000.0 / self.frames.max(1) as f64;
            info!("canlı dock: {} kare/dk, ort {:.3} ms, en çok {:.3} ms", self.frames, avg, self.max.as_secs_f64() * 1000.0);
            *self = Stats { since: Instant::now(), frames: 0, total: Duration::ZERO, max: Duration::ZERO };
        }
    }
}

pub struct Live {
    cfg: LiveConfig,
    hwnd: HWND,
    watch: Watch,
    state: State,
    /// Dock'un dış etkenleri (son mesajda okunan).
    hidden: (bool, bool),
    /// Gizlilikten yeni çıkıldı: bu ana kadar hâlâ gizli sayılır.
    settle: Option<Instant>,
    fullscreen: bool,
    scene: Option<Scene>,
    scene_last: Instant,
    scene_due: Instant,
    /// Gerçek hava (dosyadan; `None`: özellik kapalı/veri yok → elle seçilen sahne).
    wx: Option<carlui_core::weather::Weather>,
    wx_checked: Instant,
    /// Hava yoklayıcısı (yalnızca özellik açıkken; bırakılınca durur).
    poll: Option<weather_poll::Poller>,
    /// İlk kurulum bitti: bundan sonraki sahne değişimleri geçişli olur.
    started: bool,
    fade_in: Option<fade::Fade>,
    fade_out: Option<(Scene, fade::Fade)>,
    /// Saat kipinde son bakılan dakika (dakikada en çok bir kez).
    clock_min: Option<u32>,
    perch: Option<Creature>,
    pet_last: Instant,
    pet_due: Instant,
    loading: Option<mpsc::Receiver<Result<load::Loaded, String>>>,
    windows: usize,
    stats: Option<Stats>,
}

impl Live {
    /// Kapalıysa ya da ek ekran dock'uysa `None`: hiçbir kaynak ayrılmaz.
    pub fn new(cfg: &LiveConfig, gfx: &Gfx, dock: &Dock, fullscreen: bool, windows: usize) -> Option<Self> {
        if cfg.is_off() {
            return None;
        }
        let view = dock.live_view();
        if !view.primary {
            return None;
        }
        // Hayvan yalnızca altta duran ana dock'ta (yan/üst dock'ta oturacak yatay kenar yok).
        let pet_allowed = view.side == carlui_core::config::DockPosition::Bottom;
        let now = Instant::now();
        let mut l = Self {
            cfg: cfg.clone(),
            hwnd: view.hwnd,
            watch: Watch::new(view.hwnd),
            state: State::Off,
            hidden: (view.suppressed, view.slid_away),
            settle: None,
            fullscreen,
            scene: None,
            scene_last: now,
            scene_due: now,
            wx: carlui_core::weather::state::effective(&cfg.weather),
            wx_checked: now,
            poll: None,
            started: false,
            fade_in: None,
            fade_out: None,
            clock_min: None,
            perch: None,
            pet_last: now,
            pet_due: now,
            loading: None,
            windows,
            stats: std::env::var_os("CARLUI_LIVE_DEBUG").map(|_| Stats { since: now, frames: 0, total: Duration::ZERO, max: Duration::ZERO }),
        };
        if cfg.has_pet() && !pet_allowed {
            info!("canlı dock: evcil hayvan yalnızca alttaki dock'ta");
        }
        if pet_allowed {
            if let Some(src) = load::Source::from_config(&cfg.pet) {
                l.loading = Some(load::spawn(src, view.hwnd));
            } else if cfg.has_pet() {
                warn!("canlı dock: geçersiz paket adı {:?}", cfg.pet);
            }
        }
        l.start_poller(view.hwnd);
        l.build_scene(gfx, dock);
        l.refresh(gfx, dock);
        l.started = true;
        Some(l)
    }

    pub fn config(&self) -> &LiveConfig {
        &self.cfg
    }

    /// Sahneyi (saat kipinde şimdiki bölüme göre) kurar ya da kaldırır.
    fn build_scene(&mut self, gfx: &Gfx, dock: &Dock) {
        let t = unsafe { GetLocalTime() };
        let (h, m) = (t.wHour as u32, t.wMinute as u32);
        self.clock_min = Some(h * 60 + m);
        let resolved = self.resolve_scene(h, m);
        if self.scene.as_ref().map(|s| s.resolved) == resolved {
            return;
        }
        // Eskisi önce ayrılır: geçişli ise silinerek gider, değilse hemen bırakılır (bellek).
        let old = self.scene.take();
        let Some(resolved) = resolved else {
            self.begin_fade(old);
            return;
        };
        let view = dock.live_view();
        match Scene::new(gfx, &view.content, resolved, view.size, view.scale, self.cfg.intensity, &view.theme, view.light) {
            Ok(s) => self.scene = Some(s),
            Err(e) => warn!("canlı dock: sahne kurulamadı: {e}"),
        }
        self.begin_fade(old);
        self.scene_last = Instant::now();
        self.scene_due = self.scene_last;
    }

    fn signals(&self) -> Signals {
        let p = self.watch.now;
        Signals {
            enabled: true,
            suppressed: self.hidden.0 || self.settle.is_some(),
            slid_away: self.hidden.1,
            fullscreen: self.fullscreen,
            locked: p.locked,
            display_off: p.display_off,
            battery_saver: p.saver,
            on_battery: p.on_battery,
            pause_on_battery: self.cfg.pause_on_battery,
            reduce_motion: carlui_core::theme::reduced_motion(),
        }
    }

    /// Durumu yeniden değerlendirir; değiştiyse görünürlüğü ve zamanlayıcıyı ayarlar.
    fn refresh(&mut self, gfx: &Gfx, dock: &Dock) {
        let st = self.signals().state();
        if st == self.state {
            return;
        }
        info!("canlı dock: {st:?}");
        self.state = st;
        if let Some(p) = self.perch.as_mut() {
            p.show(st.visible());
        }
        match st {
            State::Running => {
                // Duraklamadan dönüş: aradaki süre "geçmemiş" sayılır.
                let now = Instant::now();
                self.scene_last = now;
                self.pet_last = now;
                self.scene_due = now;
                self.pet_due = now;
                self.tick(gfx, dock);
            }
            State::Static => {
                self.kill();
                self.draw_static(gfx, dock);
            }
            _ => self.kill(),
        }
    }

    /// Kare zamanlayıcısını durdurur. Kayma beklemesi sürüyorsa zamanlayıcı
    /// yalnızca onun bitişine kurulur (tek zamanlayıcı ikisine de yeter).
    fn kill(&self) {
        unsafe {
            match self.settle {
                Some(t) => {
                    let ms = t.saturating_duration_since(Instant::now()).as_millis() as u32;
                    SetTimer(self.hwnd, TIMER, ms.max(10), None);
                }
                None => {
                    let _ = KillTimer(self.hwnd, TIMER);
                }
            }
        }
    }

    /// Hareketi azalt: tek, durağan kare (zamanlayıcı yok).
    fn draw_static(&mut self, gfx: &Gfx, dock: &Dock) {
        self.check_clock(gfx, dock);
        self.check_weather(gfx, dock, false);
        // Hareketi azalt: geçiş yok, tek durağan kare.
        self.finish_fades();
        if let Some(s) = self.scene.as_mut() {
            let _ = s.render();
        }
        if let Some(p) = self.perch.as_mut() {
            p.render(dock.live_pill());
        }
    }

    /// Saat kipinde dakika değiştiyse bölümü yeniden çözer.
    fn check_clock(&mut self, gfx: &Gfx, dock: &Dock) {
        // Gerçek hava varken saat kipi kullanılmaz (hava kendi gece/gündüzünü verir).
        if self.cfg.scene != carlui_core::live::LiveScene::Clock || self.wx.is_some() {
            return;
        }
        let t = unsafe { GetLocalTime() };
        let now = t.wHour as u32 * 60 + t.wMinute as u32;
        if self.clock_min != Some(now) {
            self.build_scene(gfx, dock);
        }
    }

    /// Bir kare: süresi gelen efektleri ilerletir, sonra zamanlayıcıyı en yakın ana kurar.
    fn tick(&mut self, gfx: &Gfx, dock: &Dock) {
        if self.state != State::Running {
            self.kill();
            return;
        }
        let start = Instant::now();
        self.check_clock(gfx, dock);
        self.check_weather(gfx, dock, false);
        let now = Instant::now();
        let pill = dock.live_pill();
        let scale = dock.live_view_scale();
        if let Some(s) = self.scene.as_mut() {
            if now >= self.scene_due {
                let dt = (now - self.scene_last).as_secs_f32().min(0.25);
                self.scene_last = now;
                s.resize(dock.live_size());
                if let Err(e) = s.step(dt) {
                    warn!("canlı dock: sahne çizilemedi: {e}");
                }
                self.scene_due = now + Duration::from_millis(s.kind().interval_ms() as u64);
            }
        }
        // Sahne geçişi sürüyorsa kareler sıklaşır (sahne aralığı seyrek olsa da).
        self.step_fades(now);
        if let Some(p) = self.perch.as_mut() {
            if now >= self.pet_due {
                let dt = (now - self.pet_last).as_secs_f32();
                self.pet_last = now;
                let ms = p.step(dt, pill, dock.live_size(), scale);
                self.pet_due = now + Duration::from_secs_f32(ms.max(0.0) / 1000.0);
            }
        }
        if let Some(st) = self.stats.as_mut() {
            st.add(start.elapsed());
        }
        self.arm();
    }

    fn arm(&self) {
        let now = Instant::now();
        let mut want: Option<Duration> = None;
        if self.scene.is_some() || self.fade_out.is_some() {
            want = Some(self.scene_due.saturating_duration_since(now));
        }
        if self.perch.is_some() {
            let d = self.pet_due.saturating_duration_since(now);
            want = Some(want.map_or(d, |w| w.min(d)));
        }
        let ms = want.map(|d| d.as_millis().min(u32::MAX as u128) as u32);
        match self.state.timer(ms) {
            Some(ms) => unsafe {
                SetTimer(self.hwnd, TIMER, ms, None);
            },
            None => self.kill(),
        }
    }

    /// Paket çözüldü: tüneği UI iş parçacığında kurar.
    fn loaded(&mut self, gfx: &Gfx, dock: &Dock) {
        let Some(rx) = self.loading.as_ref() else { return };
        let Ok(r) = rx.try_recv() else { return };
        self.loading = None;
        match r {
            Ok(d) => match Creature::new(gfx, &dock.live_view(), d) {
                Ok(mut p) => {
                    p.show(self.state.visible());
                    info!("canlı dock: evcil hayvan hazır (x{})", p.scale());
                    self.perch = Some(p);
                    self.pet_last = Instant::now();
                    self.pet_due = self.pet_last;
                    if self.state == State::Running {
                        self.tick(gfx, dock);
                    }
                }
                Err(e) => warn!("canlı dock: evcil hayvan penceresi kurulamadı: {e}"),
            },
            Err(e) => warn!("canlı dock: paket yüklenemedi ({}): {e}", self.cfg.pet),
        }
    }

    /// Dock penceresinin her mesajından sonra çağrılır. Mesajların çoğunda
    /// yalnızca birkaç bayrak karşılaştırılır.
    pub fn on_message(&mut self, gfx: &Gfx, dock: &Dock, fullscreen: bool, windows: usize, msg: u32, wp: WPARAM, lp: LPARAM) {
        let mut changed = self.watch.handle(msg, wp, lp);
        let hidden = dock.live_hidden();
        if hidden != self.hidden || fullscreen != self.fullscreen {
            // Gizlilikten çıkış: dock yerine kayana dek bekle (hayvan önden görünmesin).
            if (self.hidden.0 || self.hidden.1) && !(hidden.0 || hidden.1) {
                self.settle = Some(Instant::now() + SETTLE);
                unsafe {
                    SetTimer(self.hwnd, TIMER, SETTLE.as_millis() as u32, None);
                }
            } else if hidden.0 || hidden.1 {
                self.settle = None;
            }
            self.hidden = hidden;
            self.fullscreen = fullscreen;
            changed = true;
        }
        let timer = msg == WM_TIMER && wp.0 == TIMER;
        if timer && self.settle.is_some_and(|t| Instant::now() >= t) {
            self.settle = None;
            changed = true;
        }
        match msg {
            WM_APP_LIVE => self.loaded(gfx, dock),
            // Yoklayıcı yeni hava yazdı: sahne değiştiyse hemen yansıt.
            WM_APP_WEATHER => {
                if self.check_weather(gfx, dock, true) {
                    match self.state {
                        State::Running => self.tick(gfx, dock),
                        State::Static => self.draw_static(gfx, dock),
                        _ => {}
                    }
                }
            }
            _ if timer => {
                if self.state == State::Running {
                    self.tick(gfx, dock);
                } else if self.settle.is_none() {
                    self.kill();
                }
            }
            WM_MOUSEMOVE => self.cursor(true),
            WM_MOUSELEAVE => self.cursor(false),
            _ => {}
        }
        // Yeni pencere açıldı: hayvan irkilir/uyanır.
        if windows > self.windows {
            if let Some(p) = self.perch.as_mut() {
                p.poke();
                self.pet_due = Instant::now();
                if self.state == State::Running {
                    self.arm();
                } else if self.state == State::Static {
                    p.render(dock.live_pill());
                }
            }
        }
        self.windows = windows;
        if changed {
            self.refresh(gfx, dock);
        }
    }

    fn cursor(&mut self, inside: bool) {
        if self.state != State::Running {
            return;
        }
        let Some(p) = self.perch.as_mut() else { return };
        let Some(ms) = p.cursor(inside) else { return };
        let due = Instant::now() + Duration::from_secs_f32(ms / 1000.0);
        // Şerit hayvanda bakış başladı/bitti: yeni bekleme süresi. Yılanda
        // yalnızca öne çekilir (fare her kıpırdadığında tik sıklaşmasın).
        let snake = matches!(p, Creature::Snake(_));
        if !snake || due < self.pet_due {
            self.pet_due = due;
            self.arm();
        }
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        self.settle = None;
        self.kill();
        // Sahne, tünek ve güç kayıtları kendi Drop'larında bırakılır.
    }
}
