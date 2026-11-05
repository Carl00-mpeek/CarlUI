//! Stüdyonun model seçme ekranı: "hangi sesi / hangi yazıya dökme modelini
//! kullanacağım?" sorusu burada çözülür. Sekmenin tüm modelleri Küçük, Orta,
//! Büyük diye üç grupta listelenir; her kartta boyut, lisans, önerilen sistem
//! (sınırdaysa turuncu uyarı) ve Seç / İndir / Sil düğmeleri vardır. Seçim
//! ayar dosyasına da yazılır; dikte ve kısayolla okuma aynı modeli kullanır.

use super::{job::Work, Act, Studio, PAD};
use carlui_ai::{
    catalog::{self, Feature, Model, Tier},
    hw, store,
};
use carlui_ui::paint::{Align, Painter, ORANGE, R, TEXT};
use windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_ALIASED;

/// Listenin başladığı yükseklik (üstte geri düğmesi ve başlık).
const TOP: f32 = 64.0;
const CARD: f32 = 78.0;
const GAP: f32 = 6.0;
const HEAD: f32 = 34.0;

/// Kartın gösterileceği grup; "Özel" modeller boyutlarına göre üç gruptan birine girer.
pub fn group(m: &Model) -> Tier {
    match m.tier {
        Tier::Extra => match m.size() {
            s if s < 200_000_000 => Tier::Small,
            s if s < 700_000_000 => Tier::Medium,
            _ => Tier::Large,
        },
        t => t,
    }
}

fn mb(n: u64) -> String {
    crate::ai::mb(n)
}

/// Satır: Otomatik (yalnızca ses için) ya da bir model.
enum Item {
    Auto,
    Model(&'static Model),
}

impl Studio {
    pub(super) fn feature(&self) -> Feature {
        if self.mode == 0 {
            Feature::Speak
        } else {
            Feature::Speech
        }
    }

    fn items(&self) -> Vec<(Option<Tier>, Item)> {
        let f = self.feature();
        let mut v = Vec::new();
        if f == Feature::Speak {
            v.push((None, Item::Auto));
        }
        for t in [Tier::Small, Tier::Medium, Tier::Large] {
            v.extend(catalog::of(f).filter(|m| group(m) == t).map(|m| (Some(t), Item::Model(m))));
        }
        v
    }

    /// Seçili olan mı: ses sekmesinde stüdyonun sesi, yazıya dökmede ayar.
    fn is_chosen(&self, it: &Item) -> bool {
        match (self.mode, it) {
            (0, Item::Auto) => self.voices[self.voice].is_none(),
            (0, Item::Model(m)) => self.voices[self.voice].is_some_and(|v| v.id == m.id),
            (_, Item::Model(m)) => self.stt_model().id == m.id,
            _ => false,
        }
    }

    pub(super) fn choose(&mut self, id: Option<&'static str>) {
        self.apply(id);
        self.note(t!("Seçildi", "Selected").into(), false);
    }

    /// Seçimi uygular: ses sekmesinde stüdyonun sesi, seçilebilen modelde ayar dosyası.
    fn apply(&mut self, id: Option<&'static str>) {
        let f = self.feature();
        if f == Feature::Speak {
            self.voice = self.voices.iter().position(|v| v.map(|m| m.id) == id).unwrap_or(0);
        }
        // Seçilebilir modeller ayara da yazılır (Türkçe ses Türkçe metinde kendiliğinden seçilir).
        if let Some(m) = id.and_then(catalog::get).filter(|m| catalog::selectable(m)) {
            let mut cfg = carlui_core::config::load();
            if cfg.ai.chosen(f.key()) != m.id {
                cfg.ai.set(f.key(), m.id);
                carlui_core::config::save(&cfg);
            }
        }
    }

    pub(super) fn fetch(&mut self, id: &'static str) {
        let Some(m) = catalog::get(id) else { return };
        if self.job.work() != Work::Idle || !self.ask(m) {
            return;
        }
        if self.job.begin(Work::Preparing, t!("Hazırlanıyor…", "Preparing…").into()) {
            self.fetching = Some(id);
            let job = self.job.clone();
            std::thread::spawn(move || match job.prepare(m) {
                Ok(()) => job.finish(t!(format!("İndirildi ve seçildi: {}", m.name), format!("Downloaded and selected: {}", m.name)), None),
                Err(e) => job.fail(e),
            });
        }
    }

    pub(super) fn remove(&mut self, id: &'static str) {
        let Some(m) = catalog::get(id) else { return };
        match store::remove(m) {
            Ok(()) => self.note(t!(format!("Silindi: {}", m.name), format!("Removed: {}", m.name)), false),
            Err(e) => self.note(e, true),
        }
    }

    pub(super) fn models_wheel(&mut self, delta: f32) {
        self.list_scroll = (self.list_scroll - delta * 60.0).clamp(0.0, self.list_max);
    }

    pub(super) fn paint_models(&mut self, p: &mut Painter<Act>, w: f32, h: f32, work: Work) {
        if work == Work::Idle {
            // İndirme bittiyse indirilen model kendiliğinden seçilir.
            if let Some(m) = self.fetching.take().and_then(catalog::get).filter(|m| store::installed(m)) {
                self.apply(Some(m.id));
            }
        }
        p.button(R::new(PAD, 16.0, 44.0, 34.0), Some('\u{E72B}'), "", Act::Models(false), true, false);
        let title = if self.mode == 0 { t!("Ses seç", "Choose a voice") } else { t!("Yazıya dökme modeli seç", "Choose a speech-to-text model") };
        p.text(title, R::new(PAD + 58.0, 16.0, w - PAD * 2.0 - 58.0, 34.0), 17.0, true, TEXT, 0.95, Align::Left);

        let mut top = TOP;
        if self.mode == 1 {
            // Konuşma dili yazıya dökme modelinin yanında seçilir.
            p.text(t!("Konuşma dili", "Speech language"), R::new(PAD + 2.0, 62.0, 140.0, 32.0), 13.0, false, TEXT, 0.7, Align::Left);
            let langs = [(t!("Otomatik", "Automatic"), Act::Lang(0)), ("Türkçe", Act::Lang(1)), ("English", Act::Lang(2))];
            p.segments(R::new(w - PAD - 300.0, 62.0, 300.0, 32.0), &langs, self.lang);
            top += 44.0;
        }

        let bottom = h - 70.0;
        let cw = w - 2.0 * PAD;
        // Toplam yükseklik: kaydırma sınırı için.
        let items = self.items();
        let mut total = 0.0;
        let mut last: Option<Option<Tier>> = None;
        for (g, _) in &items {
            if last != Some(*g) && g.is_some() {
                total += HEAD;
            }
            last = Some(*g);
            total += CARD + GAP;
        }
        self.list_max = (total - (bottom - top)).max(0.0);
        self.list_scroll = self.list_scroll.min(self.list_max);

        // Liste kendi alanına kırpılır; alan dışında kalan düğmeler tıklanmaz.
        let band = R::new(0.0, top, w, bottom - top);
        let first_hit = p.hits.len();
        unsafe { p.rt.PushAxisAlignedClip(&band.d2d(), D2D1_ANTIALIAS_MODE_ALIASED) };
        let mut y = top - self.list_scroll;
        let mut last: Option<Option<Tier>> = None;
        let rec = catalog::recommended(self.feature()).id;
        for (g, it) in &items {
            if last != Some(*g) {
                if let Some(t) = g {
                    p.text(&t.label().to_uppercase(), R::new(PAD + 4.0, y + 10.0, cw, 20.0), 11.5, true, TEXT, 0.5, Align::Left);
                    y += HEAD;
                }
                last = Some(*g);
            }
            if y + CARD > top && y < bottom {
                self.card(p, R::new(PAD, y, cw, CARD), it, rec, work);
            }
            y += CARD + GAP;
        }
        unsafe { p.rt.PopAxisAlignedClip() };
        let mut i = first_hit;
        while i < p.hits.len() {
            let r = p.hits[i].0;
            if r.y < top || r.y + r.h > bottom {
                p.hits.remove(i);
            } else {
                i += 1;
            }
        }
        if self.list_max > 0.0 {
            let track = bottom - top;
            let th = (track * track / (track + self.list_max)).max(30.0);
            let ty = top + (track - th) * (self.list_scroll / self.list_max);
            p.round(R::new(w - 8.0, ty, 4.0, th), 2.0, 0xFFFFFF, 0.25);
        }
        self.status(p, PAD, h - 54.0, w - PAD);
    }

    fn card(&self, p: &mut Painter<Act>, r: R, it: &Item, rec: &str, work: Work) {
        p.card(r);
        let x = r.x + 16.0;
        let tw = r.w - 16.0 - 170.0;
        let chosen = self.is_chosen(it);
        let (name, about, info, warn) = match it {
            Item::Auto => {
                let en = catalog::chosen(&carlui_core::config::load().ai, Feature::Speak);
                (
                    t!("Otomatik", "Automatic").to_string(),
                    t!(format!("Türkçe metin Türkçe sesle, diğer metinler {} ile okunur.", en.name), format!("Turkish text uses the Turkish voice, other text uses {}.", en.name)),
                    t!("Metne göre ses kendiliğinden seçilir", "The voice is picked from the text").to_string(),
                    false,
                )
            }
            Item::Model(m) => {
                let mut info = format!("{} · {}", mb(m.size()), hw::summary(m));
                let mut warn = false;
                if let Some(wr) = hw::warning(m) {
                    info = format!("{info} · {wr}");
                    warn = true;
                }
                if m.restricted {
                    info = t!(format!("{info} · {} · yalnızca ticari olmayan kullanım", m.license), format!("{info} · {} · non-commercial use only", m.license));
                    warn = true;
                } else {
                    info = format!("{info} · {}", m.license);
                }
                (m.name.to_string(), m.about().to_string(), info, warn)
            }
        };
        // Ad ve gerekiyorsa küçük "Önerilen" etiketi.
        let nw = p.fonts.width(&name, 14.0, true).min(tw);
        p.text(&name, R::new(x, r.y + 10.0, nw + 4.0, 22.0), 14.0, true, TEXT, 0.95, Align::Left);
        if matches!(it, Item::Model(m) if m.id == rec) {
            let l = t!("Önerilen", "Recommended");
            let lw = p.fonts.width(l, 11.0, true) + 14.0;
            let pr = R::new(x + nw + 10.0, r.y + 12.0, lw, 18.0);
            p.round(pr, 9.0, p.accent, 0.22);
            p.text(l, pr, 11.0, true, p.accent, 1.0, Align::Center);
        }
        p.text(&about, R::new(x, r.y + 33.0, tw, 18.0), 12.0, false, TEXT, 0.6, Align::Left);
        let (c, a) = if warn { (ORANGE, 0.95) } else { (TEXT, 0.42) };
        p.text(&info, R::new(x, r.y + 52.0, tw, 18.0), 11.5, false, c, a, Align::Left);

        // Tek ana düğme: İndir → Seç → Seçili; kurulu modelde yanında küçük çöp kutusu.
        let by = r.y + (r.h - 32.0) / 2.0;
        let main = R::new(r.right() - 16.0 - 108.0, by, 108.0, 32.0);
        let idle = work == Work::Idle;
        let Item::Model(m) = it else {
            if chosen {
                p.button(main, Some('\u{E73E}'), t!("Seçili", "Selected"), Act::Area, false, false);
            } else {
                p.button(main, None, t!("Seç", "Select"), Act::Choose(None), true, false);
            }
            return;
        };
        if self.fetching == Some(m.id) {
            let frac = self.job.state.lock().map(|s| s.frac).unwrap_or(0.0);
            p.button(main, Some('\u{E896}'), &format!("%{:.0}", frac * 100.0), Act::Area, false, false);
        } else if !store::installed(m) {
            p.button(main, Some('\u{E896}'), t!("İndir", "Download"), Act::Fetch(m.id), idle, false);
        } else {
            if chosen {
                p.button(main, Some('\u{E73E}'), t!("Seçili", "Selected"), Act::Area, false, false);
            } else {
                p.button(main, None, t!("Seç", "Select"), Act::Choose(Some(m.id)), true, false);
            }
            p.button(R::new(main.x - 8.0 - 36.0, by, 36.0, 32.0), Some('\u{E74D}'), "", Act::Remove(m.id), idle, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_studio_model_is_in_a_group() {
        for f in [Feature::Speak, Feature::Speech] {
            for m in catalog::of(f) {
                assert!(matches!(group(m), Tier::Small | Tier::Medium | Tier::Large), "{}", m.id);
            }
        }
        assert_eq!(group(catalog::get(catalog::TURKISH_VOICE).unwrap()), Tier::Small);
    }
}
