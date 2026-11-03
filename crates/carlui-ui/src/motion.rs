//! Spring tabanlı animasyon yardımcıları. Animasyonlar compositor'de çalışır;
//! burada yalnızca bir kez tanımlanıp başlatılırlar.

use carlui_core::theme::Theme;
use windows::{
    core::*,
    Foundation::{IReference, PropertyValue, TimeSpan},
    UI::Composition::*,
};

pub fn ts(ms: i64) -> TimeSpan {
    TimeSpan { Duration: ms * 10_000 }
}

#[derive(Clone)]
pub struct Motion {
    pub comp: Compositor,
    /// Temadaki animasyon hızı çarpanı.
    pub speed: f32,
}

impl Motion {
    pub fn new(comp: &Compositor, theme: &Theme) -> Self {
        Self { comp: comp.clone(), speed: theme.anim_scale }
    }

    fn scaled(&self, ms: i64) -> i64 {
        ((ms as f32) * self.speed).round().max(1.0) as i64
    }

    /// Skaler spring. Vektör özellikler alt kanallardan ("Scale.X" gibi) sürülür,
    /// çünkü windows-rs vektör `IReference` üretmiyor.
    pub fn spring(&self, target: f32, damping: f32, period_ms: i64, delay_ms: i64) -> Result<SpringScalarNaturalMotionAnimation> {
        let a = self.comp.CreateSpringScalarAnimation()?;
        a.SetFinalValue(&PropertyValue::CreateSingle(target)?.cast::<IReference<f32>>()?)?;
        a.SetDampingRatio(damping)?;
        a.SetPeriod(ts(self.scaled(period_ms)))?;
        if delay_ms > 0 {
            a.SetDelayTime(ts(self.scaled(delay_ms)))?;
            a.SetDelayBehavior(AnimationDelayBehavior::SetInitialValueBeforeDelay)?;
        }
        Ok(a)
    }

    pub fn scale(&self, v: &impl Interface, scale: f32, damping: f32, period_ms: i64, delay_ms: i64) -> Result<()> {
        let obj: CompositionObject = v.cast()?;
        let a = self.spring(scale, damping, period_ms, delay_ms)?;
        obj.StartAnimation(h!("Scale.X"), &a)?;
        obj.StartAnimation(h!("Scale.Y"), &a)?;
        Ok(())
    }

    pub fn animate(&self, v: &impl Interface, prop: &HSTRING, target: f32, damping: f32, period_ms: i64, delay_ms: i64) -> Result<()> {
        let obj: CompositionObject = v.cast()?;
        obj.StartAnimation(prop, &self.spring(target, damping, period_ms, delay_ms)?)
    }

    /// Doğrusal-yumuşak geçiş (opaklık gibi spring'in fazla olduğu yerler için).
    pub fn fade(&self, v: &impl Interface, prop: &HSTRING, to: f32, ms: i64, delay_ms: i64) -> Result<()> {
        let obj: CompositionObject = v.cast()?;
        let a = self.comp.CreateScalarKeyFrameAnimation()?;
        a.InsertKeyFrame(1.0, to)?;
        a.SetDuration(ts(self.scaled(ms)))?;
        if delay_ms > 0 {
            a.SetDelayTime(ts(self.scaled(delay_ms)))?;
        }
        obj.StartAnimation(prop, &a)
    }
}
