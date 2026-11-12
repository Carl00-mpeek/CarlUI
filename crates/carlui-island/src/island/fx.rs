//! Adadaki sahne efekti (yağmur, kar, gece, bulut): bildirim kartının arkasında
//! ya da ("her zaman" seçiliyse) adanın her hâlinde camın içinde.
//! Dock'un `live/scene.rs` yaklaşımının aynısı: her parçacık türü için küçük bir
//! bitmap bir kez çizilir, her parçacık bu bitmap'i paylaşan bir SpriteVisual'dır
//! ve karede yalnızca konum/boyut/saydamlık değişir. Katman kartın içeriğinin EN
//! ALTINA eklenir: yazının arkasında, camın yuvarlak kırpmasının içinde kalır.
//!
//! Animasyon yalnızca bildirim görünürken çalışır: `TIMER_FX` bildirim görünümü
//! kurulunca başlar, görünüm değişince ya da ada gizlenince durur. Hareket
//! azaltılmışsa zamanlayıcı hiç kurulmaz, tek bir durağan kare kalır.

use super::*;
use carlui_core::live::{LiveIntensity, LiveScene};
use std::time::Instant;

mod art;
mod sim;
mod weather;

use art::{draw, palette, sprite_px, visual_size};
use sim::{Kind, Rng, Sim, Sprite};

pub const TIMER_FX: usize = 52;

/// Saydamlık tavanı. Dock ile (%55 / %85) aynı düzeyde: eskiden belirgin düşüktü
/// (%35 / %50, camla birlikte ~%27) ve kartta efekt neredeyse hiç seçilmiyordu.
/// Parçacıklar yazının arkasında kalır; yoğun camda biraz azalır.
fn opacity_cap(intensity: LiveIntensity, glass_alpha: u8) -> f32 {
    let base = match intensity {
        LiveIntensity::Subtle => 0.55,
        LiveIntensity::Normal => 0.85,
    };
    base * (1.0 - 0.15 * glass_alpha as f32 / 255.0)
}

/// Ayardaki sahneyi (saat kipi dahil) tür ve cam tonuna çevirir. Akvaryum ve
/// "yok" için `None` (kartta efekt çizilmez). Dock'taki `live/sky.rs` ile aynı.
fn resolve(scene: LiveScene, hour: u32) -> Option<(Kind, (u8, u8, u8))> {
    Some(match scene {
        LiveScene::None | LiveScene::Aquarium => return None,
        LiveScene::Rain => (Kind::Rain, (90, 110, 140)),
        LiveScene::Snow => (Kind::Snow, (200, 215, 235)),
        LiveScene::Night => (Kind::Night, (24, 32, 80)),
        LiveScene::Clouds => (Kind::Clouds, (255, 228, 170)),
        LiveScene::Clock => match hour % 24 {
            5..=7 => (Kind::Clouds, (255, 168, 176)),
            8..=16 => (Kind::Clouds, (255, 236, 190)),
            17..=19 => (Kind::Clouds, (255, 140, 70)),
            _ => (Kind::Night, (24, 32, 80)),
        },
    })
}

/// Kartın sahne ayarı: dock'un sahnesi, yoğunluğu ve gerçek hava ayarı.
/// Gerçek hava kart kurulurken (her bildirimde) dosyadan okunur; ağa çıkılmaz.
#[derive(Clone, Debug)]
pub struct NoteScene {
    scene: LiveScene,
    intensity: LiveIntensity,
    weather: carlui_core::live_weather::LiveWeather,
    /// Yalnızca bildirimde değil, adanın her hâlinde çizilir.
    always: bool,
}

/// Ayarlardan adanın sahnesi: efekt açıksa dock'un sahnesi ya da gerçek havası
/// (dock'ta ikisi de yoksa hiçbir şey). `scene_always` ile her görünümde çizilir.
/// Görünüme göre efekt düzeyi. Küçük ada (saat, tarih) dar olduğu için hafif;
/// şarkı çalarken biraz daha yoğun; bildirimde yoğun.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Level {
    /// Parçacık sayısı çarpanı.
    fill: f32,
    /// Saydamlık çarpanı (tavan 0,95).
    alpha: f32,
    /// Ayardaki yoğunluktan bağımsız hep yoğun.
    dense: bool,
}

impl Level {
    pub(super) fn of(view: View) -> Self {
        match view {
            View::Note => Self { fill: 1.0, alpha: 1.2, dense: true },
            View::Media | View::MediaFull | View::Hub | View::Offer => Self { fill: 0.75, alpha: 1.0, dense: false },
            _ => Self { fill: 0.4, alpha: 0.75, dense: false },
        }
    }
}

pub fn scene_from(cfg: &carlui_core::config::Config) -> Option<NoteScene> {
    let live = &cfg.dock.live;
    let any = live.scene != LiveScene::None || live.weather.active() || carlui_core::weather::debug_override().is_some();
    (cfg.notify.notif_scene && any).then(|| NoteScene { scene: live.scene, intensity: live.intensity, weather: live.weather.clone(), always: cfg.notify.scene_always })
}

pub(super) struct Fx {
    parent: ContainerVisual,
    root: ContainerVisual,
    visuals: Vec<SpriteVisual>,
    sim: Sim,
    last: Vec<(f32, f32, f32)>,
    tint: (u8, u8, u8),
    size: (f32, f32),
    level: Level,
}

impl Fx {
    fn new(gfx: &Gfx, parent: &ContainerVisual, kind: Kind, tint: (u8, u8, u8), size: (f32, f32), s: f32, intensity: LiveIntensity, level: Level, theme: &Theme, light: bool) -> Result<Self> {
        let comp = &gfx.comp;
        let root = comp.CreateContainerVisual()?;
        root.SetSize(v2(size.0, size.1))?;
        root.SetOpacity((opacity_cap(intensity, theme.glass_tint.3) * level.alpha).min(0.95))?;
        // Cam tonu (pus).
        let tint_v = comp.CreateSpriteVisual()?;
        tint_v.SetRelativeSizeAdjustment(v2(1.0, 1.0))?;
        tint_v.SetBrush(&comp.CreateColorBrushWithColor(carlui_ui::glass::exact(Rgba(tint.0, tint.1, tint.2, if light { 36 } else { 56 })))?)?;
        root.Children()?.InsertAtTop(&tint_v)?;

        let sim = Sim::new(kind, size.0, size.1, s, level.dense || intensity == LiveIntensity::Normal, level.fill, Rng::from_time());
        let pal = palette(light, theme);
        let mut cache: Vec<(Sprite, CompositionSurfaceBrush)> = Vec::new();
        let mut visuals = Vec::with_capacity(sim.pool.len());
        for p in &sim.pool {
            let brush = match cache.iter().find(|(sp, _)| *sp == p.sprite) {
                Some((_, b)) => b.clone(),
                None => {
                    let (w, h) = sprite_px(p.sprite);
                    let surf = gfx.surface(w, h, |dc| unsafe { draw(dc, p.sprite, &pal) })?;
                    let b = comp.CreateSurfaceBrushWithSurface(&surf)?;
                    b.SetStretch(CompositionStretch::Fill)?;
                    cache.push((p.sprite, b.clone()));
                    b
                }
            };
            let v = comp.CreateSpriteVisual()?;
            v.SetBrush(&brush)?;
            root.Children()?.InsertAtTop(&v)?;
            visuals.push(v);
        }
        parent.Children()?.InsertAtBottom(&root)?;
        let last = vec![(f32::NAN, f32::NAN, f32::NAN); visuals.len()];
        let mut fx = Self { parent: parent.clone(), root, visuals, sim, last, tint, size, level };
        fx.render()?;
        Ok(fx)
    }

    fn kind(&self) -> Kind {
        self.sim.kind
    }

    fn step(&mut self, dt: f32) -> Result<()> {
        self.sim.step(dt);
        self.render()
    }

    /// Simülasyonu görsellere aktarır: yalnızca değişen konum, boyut, saydamlık.
    fn render(&mut self) -> Result<()> {
        for (i, (p, v)) in self.sim.pool.iter().zip(&self.visuals).enumerate() {
            let (w, h) = visual_size(p.sprite, p.r, p.len);
            let (lw, lh, la) = &mut self.last[i];
            if (*lw, *lh) != (w, h) {
                v.SetSize(v2(w, h))?;
                (*lw, *lh) = (w, h);
            }
            if *la != p.a {
                v.SetOpacity(p.a)?;
                *la = p.a;
            }
            v.SetOffset(v3(p.x - w / 2.0, p.y - h / 2.0))?;
        }
        Ok(())
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        if let Ok(kids) = self.parent.Children() {
            let _ = kids.Remove(&self.root);
        }
    }
}

/// Adanın efekt durumu: ayar, kurulu sahne ve son kare saati.
#[derive(Default)]
pub(super) struct FxState {
    scene: Option<NoteScene>,
    fx: Option<Fx>,
    last: Option<Instant>,
}

impl Island {
    /// Adada çizilecek sahne (`None`: efekt yok). Bildirim kipinde bir sonraki
    /// bildirimde, "her zaman" kipinde bir sonraki görünüm kurulumunda geçerli olur.
    pub fn set_note_scene(&mut self, scene: Option<NoteScene>) {
        let changed = format!("{scene:?}") != format!("{:?}", self.fx.scene);
        self.fx.scene = scene;
        if changed {
            self.fx_stop();
            self.fx.fx = None;
        }
    }

    fn fx_always(&self) -> bool {
        self.fx.scene.as_ref().is_some_and(|s| s.always)
    }

    /// Bu görünümde efekt çalışır mı?
    fn fx_live_view(&self) -> bool {
        if self.fx_always() {
            // Ses göstergesi tam ekran oyunda da çıkar: orada efekt çalışmasın.
            !matches!(self.view, View::Hidden | View::Osd)
        } else {
            self.view == View::Note
        }
    }

    /// Ayardaki sahneyi (gerçek hava ya da elle seçilen) tür ve tona çevirir.
    fn fx_pick(&self) -> Option<(Kind, (u8, u8, u8), LiveIntensity)> {
        let ns = self.fx.scene.as_ref()?;
        let hour = unsafe { GetLocalTime() }.wHour as u32;
        // Gerçek hava (dosyadan) varsa o; yoksa dock'ta elle seçilen sahne.
        let picked = match carlui_core::weather::state::effective(&ns.weather) {
            Some(wx) => weather::resolve(wx, ns.weather.day_night),
            None => resolve(ns.scene, hour),
        };
        picked.map(|(k, t)| (k, t, ns.intensity))
    }

    fn fx_make(&mut self, gfx: &Gfx, parent: &ContainerVisual, w: f32, h: f32) -> Option<()> {
        let (kind, tint, intensity) = self.fx_pick()?;
        match Fx::new(gfx, parent, kind, tint, (w, h), self.s, intensity, Level::of(self.view), &self.theme, carlui_ui::tone::light()) {
            Ok(fx) => self.fx.fx = Some(fx),
            Err(e) => carlui_core::warn!("ada sahnesi kurulamadı: {e}"),
        }
        Some(())
    }

    /// Bildirim kipi: kartın içeriğine (`c`, `w`×`h` px) sahneyi en alta ekler.
    /// Sahne kurulamazsa bildirim yine de gösterilir. "Her zaman" kipinde
    /// sahne camın içinde durur (`fx_sync`), burada bir şey yapılmaz.
    pub(super) fn fx_attach(&mut self, gfx: &Gfx, c: &ContainerVisual, w: f32, h: f32) {
        if self.fx_always() {
            return;
        }
        self.fx.fx = None;
        self.fx_make(gfx, c, w, h);
    }

    /// "Her zaman" kipi: görünüm kurulunca camın içindeki sahneyi yeni boyuta
    /// uydurur. Boyut ve tür aynıysa sahne yerinde kalır (parçacıklar sıfırlanmaz).
    pub(super) fn fx_sync(&mut self, gfx: &Gfx, w: f32, h: f32) {
        if !self.fx_always() {
            return;
        }
        if !self.fx_live_view() {
            self.fx.fx = None;
            return;
        }
        let want = self.fx_pick().map(|(k, t, _)| (k, t));
        let same = match (&self.fx.fx, want) {
            (Some(fx), Some((k, t))) => fx.kind() == k && fx.tint == t && fx.size == (w, h) && fx.level == Level::of(self.view),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        self.fx.fx = None;
        let parent = self.glass.content.clone();
        if self.fx_make(gfx, &parent, w, h).is_some() {
            if let Some(fx) = &self.fx.fx {
                let _ = self.follow_content(&fx.root, w);
            }
        }
    }

    /// Zamanlayıcıyı durdurur (görünüm değişirken ve gizlenirken).
    pub(super) fn fx_stop(&mut self) {
        unsafe {
            let _ = KillTimer(self.hwnd, TIMER_FX);
        }
        self.fx.last = None;
    }

    /// Efektli görünüm kurulduysa ve hareket azaltılmamışsa kareleri başlatır.
    pub(super) fn fx_begin(&mut self) {
        let Some(fx) = &self.fx.fx else { return };
        if !self.fx_live_view() || carlui_core::theme::reduced_motion() {
            return;
        }
        self.fx.last = Some(Instant::now());
        unsafe {
            SetTimer(self.hwnd, TIMER_FX, fx.kind().interval_ms(), None);
        }
    }

    /// Efektli görünüm bittiyse sahneyi bırakır (animasyon sönmesi tamamlandı).
    pub(super) fn fx_release(&mut self) {
        if !self.fx_live_view() {
            self.fx_stop();
            self.fx.fx = None;
        }
    }

    /// `TIMER_FX`: bir kare ilerlet.
    pub fn fx_tick(&mut self) {
        if !self.fx_live_view() || self.fx.fx.is_none() {
            self.fx_stop();
            return;
        }
        let now = Instant::now();
        let dt = self.fx.last.map_or(0.0, |t| now.duration_since(t).as_secs_f32());
        self.fx.last = Some(now);
        if let Some(fx) = self.fx.fx.as_mut() {
            if fx.step(dt).is_err() {
                self.fx_stop();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tavan_dock_tavanindan_dusuk_ve_okunur() {
        for a in [0u8, 110, 255] {
            assert!(opacity_cap(LiveIntensity::Subtle, a) <= 0.55);
            assert!(opacity_cap(LiveIntensity::Normal, a) <= 0.85);
        }
        assert!(opacity_cap(LiveIntensity::Normal, 230) < opacity_cap(LiveIntensity::Normal, 110));
    }

    #[test]
    fn duzey_kucukte_hafif_bildirimde_yogun() {
        let (c, m, n) = (Level::of(View::Clock), Level::of(View::Media), Level::of(View::Note));
        assert!(c.fill < m.fill && m.fill <= n.fill);
        assert!(c.alpha < m.alpha && m.alpha < n.alpha);
        assert!(n.dense && !c.dense);
    }

    #[test]
    fn sahne_cozumu() {
        assert!(resolve(LiveScene::None, 12).is_none());
        assert!(resolve(LiveScene::Aquarium, 12).is_none());
        assert_eq!(resolve(LiveScene::Rain, 3).unwrap().0, Kind::Rain);
        assert_eq!(resolve(LiveScene::Clock, 2).unwrap().0, Kind::Night);
        assert_eq!(resolve(LiveScene::Clock, 12).unwrap().0, Kind::Clouds);
        assert_eq!(resolve(LiveScene::Clock, 21).unwrap().0, Kind::Night);
    }
}
