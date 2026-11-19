//! Camdaki sahne. Her parçacık türü için küçük bir bitmap bir kez çizilir
//! (önbellek); her parçacık bu bitmap'i paylaşan bir SpriteVisual'dır ve her
//! karede yalnızca konumu, boyutu ve saydamlığı değişir. Hiçbir karede yeni
//! yüzey ya da görsel oluşturulmaz.
//!
//! Kar taneleri ve yıldızlar CPU'da hiç adımlanmaz: hareketleri kapalı
//! formüllerdir (`particles::FlakeDrive`), compositor her ekran karesinde
//! ortak saatten (`c.t`) kendisi hesaplar. Sahnede yalnız bunlar (ve duran
//! ay) varsa sahne "edilgen"dir: kare zamanlayıcısı gerekmez.
//!
//! Katman camın içerik katmanının EN ALTINA eklenir: ikonların arkasında,
//! camın yuvarlak kırpmasının içinde kalır ve tıklamaya karışmaz.

use super::{
    aquarium::Aquarium,
    droplets_view::DropletsView,
    particles::{star_opacity_expr, FlakeDrive, Kind, Sim, Sprite},
    rng::Rng,
    scene_art::{draw, palette, sprite_px, visual_size},
    sky::Resolved,
};
use carlui_core::live::LiveIntensity;
use carlui_core::theme::{Rgba, Theme};
use carlui_ui::{
    gfx::Gfx,
    glass::{exact, v2, v3},
};
use windows::{core::*, UI::Composition::*};

/// Saydamlık tavanı: hafifte %55, normalde %85; cam yoğunlaştıkça biraz
/// azalır (koyu/yoğun camda efekt ikonlarla yarışmasın). Sahne ikonların
/// ARKASINDA kalır; bu tavan ilk bakışta fark edilsin diye seçildi.
pub fn opacity_cap(intensity: LiveIntensity, glass_alpha: u8) -> f32 {
    let base = match intensity {
        LiveIntensity::Subtle => 0.55,
        LiveIntensity::Normal => 0.85,
    };
    base * (1.0 - 0.3 * glass_alpha as f32 / 255.0)
}

pub struct Scene {
    parent: ContainerVisual,
    root: ContainerVisual,
    visuals: Vec<SpriteVisual>,
    sim: Sim,
    aquarium: Option<Aquarium>,
    /// Gündüz yağmuru: camdaki damlalar (yalnız canlı havada).
    droplets: Option<DropletsView>,
    pub resolved: Resolved,
    last: Vec<(f32, f32, f32)>,
    /// Son gönderilen konum (yalnız compositor'ın sürmediği görseller için
    /// her karede; yıldız/ay için yalnız değişince).
    last_at: Vec<(f32, f32)>,
    /// Kök saydamlığın tavanı; geçişte `cap * fade` uygulanır.
    cap: f32,
    comp: Compositor,
    /// Ortak saat: `t` (sn, compositor ilerletir), `w`, `h` (alan boyu).
    clock: CompositionPropertySet,
    /// Görselin hareketini compositor mu sürüyor (kar tanesi, yıldız)?
    driven: Vec<bool>,
    /// Sahnede yalnız compositor'ın sürdüğü ya da duran görseller var.
    passive: bool,
    /// Saat işliyor mu (duraklatınca/hareketi azaltınca durur).
    live: bool,
    /// Boyut değişti: bir sonraki adımda yeniden yerleştir.
    dirty: bool,
}

/// Saatin bir turu (sn). Turda bir kez taneler yerinden sıçrar; saatte bir yeter.
const CLOCK_LOOP: f32 = 3600.0;

fn ts(ms: f32) -> windows::Foundation::TimeSpan {
    windows::Foundation::TimeSpan { Duration: (ms.max(1.0) * 10_000.0) as i64 }
}

impl Scene {
    pub fn new(gfx: &Gfx, parent: &ContainerVisual, resolved: Resolved, size: (f32, f32), s: f32, intensity: LiveIntensity, theme: &Theme, light: bool) -> Result<Self> {
        let comp = &gfx.comp;
        let root = comp.CreateContainerVisual()?;
        root.SetRelativeSizeAdjustment(v2(1.0, 1.0))?;
        let cap = opacity_cap(intensity, theme.glass_tint.3);
        root.SetOpacity(cap)?;
        // Cam tonu (pus): saydamlığı düşük, tavanla birlikte daha da azalır.
        if let Some((r, g, b)) = resolved.tint {
            // Tavan yükseldi; pus aynı kalsın diye ton biraz azaltıldı.
            let a = if light { 30 } else { 44 };
            let tint = comp.CreateSpriteVisual()?;
            tint.SetRelativeSizeAdjustment(v2(1.0, 1.0))?;
            tint.SetBrush(&comp.CreateColorBrushWithColor(exact(Rgba(r, g, b, a)))?)?;
            root.Children()?.InsertAtTop(&tint)?;
        }
        let sim = Sim::with_mix(resolved.kind, size.0, size.1, s, intensity == LiveIntensity::Normal, Rng::from_time(), resolved.mix);
        let pal = palette(light, theme);
        let clock = comp.CreatePropertySet()?;
        clock.InsertScalar(h!("t"), 0.0)?;
        clock.InsertScalar(h!("w"), sim.size().0)?;
        clock.InsertScalar(h!("h"), sim.size().1)?;
        // Türe göre önbellek: her bitmap bir kez çizilir, fırça paylaşılır.
        let mut cache: Vec<(Sprite, CompositionSurfaceBrush)> = Vec::new();
        let mut visuals = Vec::with_capacity(sim.pool.len());
        let mut driven = Vec::with_capacity(sim.pool.len());
        let (sw, sh) = sim.size();
        let expr = |e: String| -> Result<ExpressionAnimation> {
            let a = comp.CreateExpressionAnimationWithExpression(&HSTRING::from(e))?;
            a.SetReferenceParameter(h!("c"), &clock)?;
            Ok(a)
        };
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
            match p.sprite {
                Sprite::Flake => {
                    let (w, h) = visual_size(p.sprite, p.r, p.len);
                    v.SetSize(v2(w, h))?;
                    let d = FlakeDrive::new(p, sim.scale(), sw, sh);
                    v.StartAnimation(h!("Offset"), &expr(d.offset_expr((w / 2.0, h / 2.0)))?)?;
                    v.StartAnimation(h!("Opacity"), &expr(d.opacity_expr())?)?;
                    driven.push(true);
                }
                Sprite::Star => {
                    v.StartAnimation(h!("Opacity"), &expr(star_opacity_expr(p))?)?;
                    driven.push(true);
                }
                _ => driven.push(false),
            }
            root.Children()?.InsertAtTop(&v)?;
            visuals.push(v);
        }
        // Akvaryum: parçacık havuzu boş; balık ve kabarcıklar kendi modülünde.
        let aquarium = match resolved.kind {
            Kind::Aquarium => Some(Aquarium::new(gfx, &root, size, s, intensity == LiveIntensity::Normal)?),
            _ => None,
        };
        // Gündüz yağmuru: pencere camındaki damlalar (kendi modülünde).
        let droplets = match (resolved.kind, resolved.mix.droplets) {
            (Kind::Rain, true) => Some(DropletsView::new(gfx, &root, size, s, resolved.mix.level.unwrap_or(50), &pal)?),
            _ => None,
        };
        parent.Children()?.InsertAtBottom(&root)?;
        // Son gönderilen (genişlik, yükseklik, saydamlık); bir kez ayrılır.
        let last = vec![(f32::NAN, f32::NAN, f32::NAN); visuals.len()];
        let last_at = vec![(f32::NAN, f32::NAN); visuals.len()];
        let passive = aquarium.is_none() && droplets.is_none() && sim.pool.iter().all(|p| matches!(p.sprite, Sprite::Flake | Sprite::Star | Sprite::Moon));
        let mut sc = Self { parent: parent.clone(), root, visuals, sim, aquarium, droplets, resolved, last, last_at, cap, comp: comp.clone(), clock, driven, passive, live: false, dirty: false };
        sc.render()?;
        sc.set_live(true)?;
        Ok(sc)
    }

    /// Sahnede yalnız compositor'ın sürdüğü ya da duran görseller var: kare
    /// zamanlayıcısı yalnız boyut/saat denetimi için seyrek çalışsın.
    pub fn passive(&self) -> bool {
        self.passive
    }

    /// Saati yürütür ya da durdurur. Durunca `t` son değerinde kalır; yeniden
    /// başlayınca oradan sürer (taneler sıçramaz).
    pub fn set_live(&mut self, on: bool) -> Result<()> {
        if on == self.live {
            return Ok(());
        }
        self.live = on;
        if !on {
            return self.clock.StopAnimation(h!("t"));
        }
        let lin = self.comp.CreateLinearEasingFunction()?;
        let a = self.comp.CreateScalarKeyFrameAnimation()?;
        a.InsertExpressionKeyFrameWithEasingFunction(0.0, h!("this.StartingValue"), &lin)?;
        a.InsertExpressionKeyFrameWithEasingFunction(1.0, &HSTRING::from(format!("this.StartingValue + {CLOCK_LOOP}")), &lin)?;
        a.SetDuration(ts(CLOCK_LOOP * 1000.0))?;
        a.SetIterationBehavior(AnimationIterationBehavior::Forever)?;
        self.clock.StartAnimation(h!("t"), &a)
    }

    pub fn kind(&self) -> Kind {
        self.resolved.kind
    }

    /// Cam boyutu değiştiyse parçacıklar yeni alana taşınır (havuz aynı).
    pub fn resize(&mut self, size: (f32, f32)) {
        if self.sim.size() != size {
            self.sim.resize(size.0, size.1);
            let (w, h) = self.sim.size();
            let _ = self.clock.InsertScalar(h!("w"), w);
            let _ = self.clock.InsertScalar(h!("h"), h);
            self.dirty = true;
        }
        if let Some(a) = self.aquarium.as_mut() {
            a.resize(size);
        }
        if let Some(d) = self.droplets.as_mut() {
            d.resize(size);
        }
    }

    /// Geçiş: sahnenin görünürlüğü 0..=1 (tavan ile çarpılır; tavan aşılmaz).
    pub fn set_fade(&self, f: f32) -> Result<()> {
        self.root.SetOpacity(self.cap * f.clamp(0.0, 1.0))
    }

    pub fn step(&mut self, dt: f32) -> Result<()> {
        if self.passive {
            // Hareket compositor'da; yalnız boyut değiştiyse yıldız/ay yerleşir.
            if std::mem::take(&mut self.dirty) {
                return self.render();
            }
            return Ok(());
        }
        self.sim.step(dt);
        if let Some(d) = self.droplets.as_mut() {
            d.step(dt);
        }
        if let Some(a) = self.aquarium.as_mut() {
            a.step(dt)?;
        }
        self.render()
    }

    /// Simülasyonu görsellere aktarır: yalnızca konum, boyut, saydamlık.
    pub fn render(&mut self) -> Result<()> {
        if let Some(a) = self.aquarium.as_mut() {
            a.render()?;
        }
        if let Some(d) = self.droplets.as_mut() {
            d.render()?;
        }
        let last = &mut self.last;
        for (i, (p, v)) in self.sim.pool.iter().zip(&self.visuals).enumerate() {
            if self.driven[i] && p.sprite == Sprite::Flake {
                // Konum ve saydamlık ifadede; boyut kurulurken verildi.
                continue;
            }
            let (w, h) = visual_size(p.sprite, p.r, p.len);
            let at = (p.x - w / 2.0, p.y - h / 2.0);
            if self.driven[i] || p.sprite == Sprite::Moon {
                // Yıldız/ay yerinde durur: yalnız yer ya da boy değişince gönder.
                if (last[i].0, last[i].1) != (w, h) {
                    v.SetSize(v2(w, h))?;
                    (last[i].0, last[i].1) = (w, h);
                }
                if !self.driven[i] && last[i].2 != p.a {
                    v.SetOpacity(p.a)?;
                    last[i].2 = p.a;
                }
                if self.last_at[i] != at {
                    v.SetOffset(v3(at.0, at.1))?;
                    self.last_at[i] = at;
                }
                continue;
            }
            // Yağmur çizgisi, yıldız, ay: boyut ve saydamlık çoğu karede aynı;
            // yalnız değişeni gönder (parçacık başına COM çağrısı azalır).
            let (lw, lh, la) = &mut last[i];
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

impl Drop for Scene {
    fn drop(&mut self) {
        // Görseller ve önbellek fırçaları bırakılınca GPU belleği de gider.
        if let Ok(kids) = self.parent.Children() {
            let _ = kids.Remove(&self.root);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saydamlik_tavani() {
        // Fark edilecek kadar görünür, ama hiçbir zaman tam opak değil.
        assert!(opacity_cap(LiveIntensity::Subtle, 0) <= 0.55);
        assert!(opacity_cap(LiveIntensity::Normal, 0) <= 0.85);
        assert!(opacity_cap(LiveIntensity::Subtle, 255) >= 0.35);
        // Yoğun camda biraz azalır.
        assert!(opacity_cap(LiveIntensity::Normal, 230) < opacity_cap(LiveIntensity::Normal, 110));
        assert!(opacity_cap(LiveIntensity::Normal, 255) > 0.55);
    }
}
