//! Bildirim kartının arkasındaki sahne efekti (yağmur, kar, gece, bulut).
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

/// Saydamlık tavanı. Dock'tan (%55 / %85) belirgin düşük: kartta yazı var ve
/// kart küçük; yazı her zaman tam okunmalı. Yoğun camda biraz daha azalır.
fn opacity_cap(intensity: LiveIntensity, glass_alpha: u8) -> f32 {
    let base = match intensity {
        LiveIntensity::Subtle => 0.35,
        LiveIntensity::Normal => 0.5,
    };
    base * (1.0 - 0.3 * glass_alpha as f32 / 255.0)
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
}

/// Ayarlardan kartın sahnesi: bildirim efekti açıksa dock'un sahnesi ya da
/// gerçek havası (dock'ta ikisi de yoksa hiçbir şey).
pub fn scene_from(cfg: &carlui_core::config::Config) -> Option<NoteScene> {
    let live = &cfg.dock.live;
    let any = live.scene != LiveScene::None || live.weather.active() || carlui_core::weather::debug_override().is_some();
    (cfg.notify.notif_scene && any).then(|| NoteScene { scene: live.scene, intensity: live.intensity, weather: live.weather.clone() })
}

pub(super) struct Fx {
    parent: ContainerVisual,
    root: ContainerVisual,
    visuals: Vec<SpriteVisual>,
    sim: Sim,
    last: Vec<(f32, f32, f32)>,
}

impl Fx {
    fn new(gfx: &Gfx, parent: &ContainerVisual, kind: Kind, tint: (u8, u8, u8), size: (f32, f32), s: f32, intensity: LiveIntensity, theme: &Theme, light: bool) -> Result<Self> {
        let comp = &gfx.comp;
        let root = comp.CreateContainerVisual()?;
        root.SetRelativeSizeAdjustment(v2(1.0, 1.0))?;
        root.SetOpacity(opacity_cap(intensity, theme.glass_tint.3))?;
        // Cam tonu (pus).
        let tint_v = comp.CreateSpriteVisual()?;
        tint_v.SetRelativeSizeAdjustment(v2(1.0, 1.0))?;
        tint_v.SetBrush(&comp.CreateColorBrushWithColor(carlui_ui::glass::exact(Rgba(tint.0, tint.1, tint.2, if light { 30 } else { 44 })))?)?;
        root.Children()?.InsertAtTop(&tint_v)?;

        let sim = Sim::new(kind, size.0, size.1, s, intensity == LiveIntensity::Normal, Rng::from_time());
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
        let mut fx = Self { parent: parent.clone(), root, visuals, sim, last };
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
    /// Bildirim kartında çizilecek sahne (`None`: efekt yok). Bir sonraki bildirim
    /// kurulumunda geçerli olur.
    pub fn set_note_scene(&mut self, scene: Option<NoteScene>) {
        self.fx.scene = scene;
    }

    /// Bildirim kartının içeriğine (`c`, `w`×`h` px) sahneyi en alta ekler.
    /// Sahne kurulamazsa bildirim yine de gösterilir.
    pub(super) fn fx_attach(&mut self, gfx: &Gfx, c: &ContainerVisual, w: f32, h: f32) {
        self.fx.fx = None;
        let Some(ns) = self.fx.scene.as_ref() else { return };
        let intensity = ns.intensity;
        let hour = unsafe { GetLocalTime() }.wHour as u32;
        // Gerçek hava (dosyadan) varsa o; yoksa dock'ta elle seçilen sahne.
        let picked = match carlui_core::weather::state::effective(&ns.weather) {
            Some(wx) => weather::resolve(wx, ns.weather.day_night),
            None => resolve(ns.scene, hour),
        };
        let Some((kind, tint)) = picked else { return };
        match Fx::new(gfx, c, kind, tint, (w, h), self.s, intensity, &self.theme, carlui_ui::tone::light()) {
            Ok(fx) => self.fx.fx = Some(fx),
            Err(e) => carlui_core::warn!("bildirim sahnesi kurulamadı: {e}"),
        }
    }

    /// Zamanlayıcıyı durdurur (görünüm değişirken ve gizlenirken).
    pub(super) fn fx_stop(&mut self) {
        unsafe {
            let _ = KillTimer(self.hwnd, TIMER_FX);
        }
        self.fx.last = None;
    }

    /// Bildirim görünümü kurulduysa ve hareket azaltılmamışsa kareleri başlatır.
    pub(super) fn fx_begin(&mut self) {
        let Some(fx) = &self.fx.fx else { return };
        if self.view != View::Note || carlui_core::theme::reduced_motion() {
            return;
        }
        self.fx.last = Some(Instant::now());
        unsafe {
            SetTimer(self.hwnd, TIMER_FX, fx.kind().interval_ms(), None);
        }
    }

    /// Bildirim görünümü bittiyse sahneyi bırakır (animasyon sönmesi tamamlandı).
    pub(super) fn fx_release(&mut self) {
        if self.view != View::Note {
            self.fx_stop();
            self.fx.fx = None;
        }
    }

    /// `TIMER_FX`: bir kare ilerlet.
    pub fn fx_tick(&mut self) {
        if self.view != View::Note || self.fx.fx.is_none() {
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
            assert!(opacity_cap(LiveIntensity::Subtle, a) <= 0.35);
            assert!(opacity_cap(LiveIntensity::Normal, a) <= 0.5);
        }
        assert!(opacity_cap(LiveIntensity::Normal, 230) < opacity_cap(LiveIntensity::Normal, 110));
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
