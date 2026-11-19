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
mod ledge;
mod ledge_view;
mod load;
mod mix;
mod pack;
mod particles;
mod pacer;
mod pause;
mod perch;
mod perch_react;
mod pet;
mod pet_fx;
mod pet_fx_view;
mod pet_hit;
mod power;
mod react;
mod rng;
mod scene;
mod scene_art;
mod signal;
mod sky;
mod sky_weather;
mod snake;
mod shimmer;
mod snowball;
mod snowfall;
mod snowplay;
mod weather_link;
mod weather_poll;
mod window;

pub use load::WM_APP_LIVE;
use pet_hit::WM_APP_PET_CLICK;
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
    /// Sık karelerde (ekran saati) iki tik arası: en uzunu ve kaçan kare sayısı.
    last: Option<Instant>,
    gap_max: Duration,
    missed: u32,
}

impl Stats {
    fn new() -> Self {
        Stats { since: Instant::now(), frames: 0, total: Duration::ZERO, max: Duration::ZERO, last: None, gap_max: Duration::ZERO, missed: 0 }
    }

    fn add(&mut self, start: Instant, d: Duration) {
        if let Some(p) = self.last.replace(start) {
            let g = start - p;
            if g < Duration::from_millis(100) {
                self.gap_max = self.gap_max.max(g);
                self.missed += (g > Duration::from_millis(25)) as u32;
            }
        }
        self.frames += 1;
        self.total += d;
        self.max = self.max.max(d);
        if self.since.elapsed() >= Duration::from_secs(60) {
            let avg = self.total.as_secs_f64() * 1000.0 / self.frames.max(1) as f64;
            info!("canlı dock: {} kare/dk, ort {:.3} ms, en çok {:.3} ms, en uzun ara {:.1} ms, kaçan {}", self.frames, avg, self.max.as_secs_f64() * 1000.0, self.gap_max.as_secs_f64() * 1000.0, self.missed);
            *self = Stats::new();
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
    /// Adanın bildirdiği son müzik durumu (hayvan sonradan yüklenirse de uygulanır).
    music: bool,
    windows: usize,
    stats: Option<Stats>,
    /// Kar birikintisi / yağmur ıslaklığı (yalnız kar ya da yağmur varken, eriyene dek).
    ledge: Option<ledge_view::LedgeView>,
    /// Yağmurda simgelerin üstünden kayan parlaklık.
    shimmer: Option<shimmer::Shimmer>,
    /// Dock altta: pill'in üstü de kar tutar.
    bottom: bool,
    /// Fare kancası kar olaylarını gönderiyor.
    snow_hook: bool,
    /// Ekran yenilemesine bağlı kare saati (ilk sürekli harekette kurulur).
    pacer: Option<pacer::Pacer>,
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
            music: false,
            windows,
            stats: std::env::var_os("CARLUI_LIVE_DEBUG").map(|_| Stats::new()),
            ledge: None,
            shimmer: None,
            bottom: pet_allowed,
            snow_hook: false,
            pacer: None,
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
        l.ask_music();
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
            self.sync_ledge(gfx, dock);
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
        self.sync_ledge(gfx, dock);
    }

    /// Yağmurun şiddeti (sahne yağmur değilse `None`).
    fn rain_level(&self) -> Option<f32> {
        let s = self.scene.as_ref()?;
        (s.resolved.kind == particles::Kind::Rain).then(|| (s.resolved.mix.level.unwrap_or(60) as f32 / 100.0).clamp(0.15, 1.0))
    }

    /// Yağmurda simge parlaklığını eşler; değilse kaldırır.
    fn sync_shimmer(&mut self, gfx: &Gfx, dock: &Dock, now: Instant) {
        match self.rain_level() {
            Some(l) => self.shimmer.get_or_insert_with(|| shimmer::Shimmer::new(l)).sync(gfx, dock, now, l),
            None => self.shimmer = None,
        }
    }

    /// Sahne kar ya da yağmursa kenar birikintisini kurar; değilse eritmeye bırakır.
    fn sync_ledge(&mut self, gfx: &Gfx, dock: &Dock) {
        let mode = self.scene.as_ref().and_then(|s| {
            let m = match s.resolved.kind {
                particles::Kind::Snow => ledge_view::Mode::Snow,
                particles::Kind::Rain => ledge_view::Mode::Rain,
                _ => return None,
            };
            Some((m, (s.resolved.mix.level.unwrap_or(60) as f32 / 100.0).clamp(0.15, 1.0)))
        });
        if self.ledge.is_none() {
            if mode.is_none() {
                return;
            }
            match ledge_view::LedgeView::new(gfx, &dock.live_view(), self.cfg.intensity) {
                Ok(l) => self.ledge = Some(l),
                Err(e) => {
                    warn!("canlı dock: kar katmanı kurulamadı: {e}");
                    return;
                }
            }
        }
        if let Some(l) = self.ledge.as_mut() {
            l.set_mode(mode);
            l.show(self.state.visible());
        }
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
            p.set_live(st == State::Running);
        }
        if let Some(l) = self.ledge.as_mut() {
            l.show(st.visible());
        }
        if st != State::Running {
            // Duraklayınca/gizlenince parlaklık da durur (animasyon boşa dönmesin).
            self.shimmer = None;
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
        if let Some(p) = self.pacer.as_ref() {
            p.set(false);
        }
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
        let pill = self.bottom.then(|| dock.live_pill());
        self.shimmer = None;
        if let Some(l) = self.ledge.as_mut() {
            if let Err(e) = l.settle(gfx, pill) {
                warn!("canlı dock: kar katmanı çizilemedi: {e}");
                self.ledge = None;
            }
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
        let lpill = self.bottom.then_some(pill);
        if let Some(l) = self.ledge.as_mut() {
            match l.step(gfx, now, lpill) {
                Ok(()) if l.finished() => self.ledge = None,
                Ok(()) => {}
                Err(e) => {
                    warn!("canlı dock: kar katmanı çizilemedi: {e}");
                    self.ledge = None;
                }
            }
        }
        self.sync_shimmer(gfx, dock, now);
        if let Some(p) = self.perch.as_mut() {
            if now >= self.pet_due {
                let dt = (now - self.pet_last).as_secs_f32();
                self.pet_last = now;
                let ms = p.step(dt, pill, dock.live_size(), scale);
                self.pet_due = now + Duration::from_secs_f32(ms.max(0.0) / 1000.0);
            }
        }
        if let Some(st) = self.stats.as_mut() {
            st.add(start, start.elapsed());
        }
        self.arm();
    }

    fn arm(&mut self) {
        let now = Instant::now();
        let mut want: Option<Duration> = None;
        if self.scene.is_some() || self.fade_out.is_some() {
            want = Some(self.scene_due.saturating_duration_since(now));
        }
        if self.perch.is_some() {
            let d = self.pet_due.saturating_duration_since(now);
            want = Some(want.map_or(d, |w| w.min(d)));
        }
        if let Some(l) = self.ledge.as_ref() {
            let d = l.due().saturating_duration_since(now);
            want = Some(want.map_or(d, |w| w.min(d)));
        }
        let ms = want.map(|d| d.as_millis().min(u32::MAX as u128) as u32);
        // Sürekli harekette kareleri ekran saati getirir (her ekran karesinde
        // bir tik); zamanlayıcının kaba, titrek adımları hareketi takıltır.
        let fast = self.state == State::Running && self.ledge.as_ref().is_some_and(|l| l.fast());
        if fast {
            let hwnd = self.hwnd;
            self.pacer.get_or_insert_with(|| pacer::Pacer::new(hwnd)).set(true);
            if self.settle.is_none() {
                unsafe {
                    let _ = KillTimer(self.hwnd, TIMER);
                }
            }
            return;
        }
        if let Some(p) = self.pacer.as_ref() {
            p.set(false);
        }
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
            Ok(d) => match Creature::new(gfx, &dock.live_view(), d, self.cfg.reactions) {
                Ok(mut p) => {
                    p.show(self.state.visible());
                    p.set_live(self.state == State::Running);
                    p.music(self.music);
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
            pacer::WM_APP_FRAME => {
                if let Some(p) = self.pacer.as_ref() {
                    p.consumed();
                    if p.running() && self.state == State::Running {
                        self.tick(gfx, dock);
                    }
                }
            }
            WM_APP_PET_CLICK => self.pet_click(),
            signal::WM_APP_PET_DEMO => self.pet_demo(wp.0),
            crate::media::WM_APP_MEDIA => self.media_changed(),
            WM_MOUSEMOVE => self.cursor(true),
            WM_MOUSELEAVE => self.cursor(false),
            crate::input::WM_APP_SNOW => self.snow_mouse(dock, wp, lp),
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
        self.sync_snow_hook();
    }

    /// Kar yağarken ve dock canlıyken fare kancası kar olaylarını da göndersin.
    fn sync_snow_hook(&mut self) {
        let want = self.state == State::Running && self.ledge.as_ref().is_some_and(|l| l.snowy());
        if want != self.snow_hook {
            self.snow_hook = want;
            crate::input::set_snow(want);
        }
    }

    /// Kar topu oyunu: basış, bırakış, hareket.
    fn snow_mouse(&mut self, dock: &Dock, wp: WPARAM, lp: LPARAM) {
        if self.state != State::Running {
            return;
        }
        let (x, y) = if wp.0 == 2 { crate::input::snow_take_move() } else { crate::input::snow_point(lp) };
        let pill = self.bottom.then(|| dock.live_pill());
        let hwnd = self.hwnd;
        if let Some(l) = self.ledge.as_mut() {
            if l.mouse(wp.0, x, y, hwnd, pill) {
                self.arm();
            }
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
        if self.snow_hook {
            crate::input::set_snow(false);
        }
        // Sahne, tünek ve güç kayıtları kendi Drop'larında bırakılır.
    }
}
