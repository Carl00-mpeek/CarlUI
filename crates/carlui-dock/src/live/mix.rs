//! Sahnenin parçacık sayıları. Elle seçilen sahnede tablo aynen eski gibi;
//! canlı havada yağış/bulut sayısı şiddete göre ölçeklenir ve gece ise
//! yıldız + ay katmanı en alta eklenir (aynı havuz, aynı simülasyon).

use super::particles::{Kind, Sprite};

/// Hava karışımı. `Mix::default()` = elle seçilmiş sahne (eski davranış).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mix {
    /// Hava şiddeti yüzde olarak (0..=100); `None`: elle seçilmiş sahne.
    pub level: Option<u8>,
    /// Yağışın/bulutun altına ay ve yıldızlar eklensin.
    pub night: bool,
    /// Gündüz yağmuru cam damlalarıyla çizilir (`droplets.rs`); çizgi yağmuru yok.
    pub droplets: bool,
}

/// Şiddetten sayı çarpanı: 0.15'te ×0.25, 1.0'da ×1.8 (doğrusal, uçlarda kırpılır).
pub fn density(level: Option<u8>) -> f32 {
    match level {
        None => 1.0,
        Some(l) => {
            let i = (l as f32 / 100.0).clamp(0.0, 1.0);
            (0.25 + (i - 0.15) * (1.8 - 0.25) / 0.85).clamp(0.12, 1.8)
        }
    }
}

/// Türe göre (çizim sırasıyla) parçacık grupları. `dense`: Normal yoğunluk.
pub fn counts(kind: Kind, dense: bool, mix: Mix) -> Vec<(Sprite, usize)> {
    let pick = |light: usize, heavy: usize| if dense { heavy } else { light };
    let f = density(mix.level);
    let scale = |n: usize| ((n as f32 * f).round() as usize).max(1);
    let mut v: Vec<(Sprite, usize)> = Vec::new();
    // Gece katmanı en alta: yağış ve bulut onun önünde.
    if mix.night && kind != Kind::Night {
        v.push((Sprite::Star, pick(14, 24)));
        v.push((Sprite::Moon, 1));
    }
    match kind {
        // Yağmur: ağırlık yağmur çizgilerinde, camdaki damlalar ikinci planda.
        Kind::Rain if mix.droplets => {}
        Kind::Rain => {
            v.push((Sprite::Drop, scale(pick(6, 10))));
            v.push((Sprite::Streak, scale(pick(24, 40))));
        }
        Kind::Snow => v.push((Sprite::Flake, scale(pick(26, 44)))),
        // Sıra çizim sırasıdır: ay yıldızların önünde, ateşböceklerinin arkasında.
        Kind::Night => {
            v.push((Sprite::Star, pick(14, 24)));
            v.push((Sprite::Moon, 1));
            v.push((Sprite::Firefly, pick(4, 7)));
        }
        Kind::Clouds => v.push((Sprite::Cloud, scale(pick(4, 6)))),
        Kind::Aquarium => {}
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total(v: &[(Sprite, usize)]) -> usize {
        v.iter().map(|c| c.1).sum()
    }

    #[test]
    fn carpan_uclarda() {
        assert!((density(Some(15)) - 0.25).abs() < 0.01);
        assert!((density(Some(100)) - 1.8).abs() < 0.01);
        assert_eq!(density(None), 1.0);
        assert!(density(Some(0)) >= 0.12);
    }

    #[test]
    fn eski_tablo_aynen() {
        let m = Mix::default();
        assert_eq!(counts(Kind::Rain, false, m), vec![(Sprite::Drop, 6), (Sprite::Streak, 24)]);
        assert_eq!(counts(Kind::Rain, true, m), vec![(Sprite::Drop, 10), (Sprite::Streak, 40)]);
        assert_eq!(counts(Kind::Snow, true, m), vec![(Sprite::Flake, 44)]);
        assert_eq!(counts(Kind::Clouds, false, m), vec![(Sprite::Cloud, 4)]);
    }

    #[test]
    fn siddetle_artar_ve_havuz_sinirda() {
        for dense in [false, true] {
            let a = total(&counts(Kind::Rain, dense, Mix { level: Some(15), ..Mix::default() }));
            let b = total(&counts(Kind::Rain, dense, Mix { level: Some(100), night: true, droplets: false }));
            assert!(b > a * 3, "{a} {b}");
            assert!(b <= 200);
        }
        // Gece katmanı en altta, yıldız + ay.
        let c = counts(Kind::Snow, true, Mix { level: Some(50), night: true, droplets: false });
        assert_eq!(c[0].0, Sprite::Star);
        assert_eq!(c[1], (Sprite::Moon, 1));
        // Gündüz yağmuru: çizgi yok.
        assert!(counts(Kind::Rain, true, Mix { level: Some(50), night: false, droplets: true }).is_empty());
    }
}
