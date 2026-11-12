//! Saate göre sahne: günün bölümü → sahne türü ve cam tonu. Saf işlevler.

use super::particles::Kind;
use carlui_core::live::LiveScene;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 05:00–08:00 şafak: pembe pus, seyrek bulut.
    Dawn,
    /// 08:00–17:00 gündüz: bulutlar.
    Day,
    /// 17:00–20:00 akşam: turuncu pus, bulutlar.
    Dusk,
    /// 20:00–05:00 gece: yıldızlar ve ateşböcekleri.
    Night,
}

/// Saatten (0-23) ve dakikadan günün bölümü.
pub fn phase(hour: u32, _minute: u32) -> Phase {
    match hour % 24 {
        5..=7 => Phase::Dawn,
        8..=16 => Phase::Day,
        17..=19 => Phase::Dusk,
        _ => Phase::Night,
    }
}

/// Cam tonu (RGB); saydamlığı yoğunluk ayarı belirler.
pub type Tint = (u8, u8, u8);

/// Çözülmüş sahne: parçacık türü ve isteğe bağlı ton.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub kind: Kind,
    pub tint: Option<Tint>,
    /// Canlı havada şiddet, gece katmanı ve cam damlaları; elle seçilen sahnede varsayılan.
    pub mix: super::mix::Mix,
}

/// Ayardaki sahneyi (saat kipi dahil) somut sahneye çevirir. `None`: sahne yok.
pub fn resolve(scene: LiveScene, hour: u32, minute: u32) -> Option<Resolved> {
    let r = |kind, tint| Some(Resolved { kind, tint, mix: Default::default() });
    match scene {
        LiveScene::None => None,
        LiveScene::Rain => r(Kind::Rain, Some((90, 110, 140))),
        LiveScene::Snow => r(Kind::Snow, Some((200, 215, 235))),
        LiveScene::Night => r(Kind::Night, Some((24, 32, 80))),
        LiveScene::Clouds => r(Kind::Clouds, Some((255, 228, 170))),
        LiveScene::Aquarium => r(Kind::Aquarium, Some((40, 110, 150))),
        LiveScene::Clock => match phase(hour, minute) {
            Phase::Dawn => r(Kind::Clouds, Some((255, 168, 176))),
            Phase::Day => r(Kind::Clouds, Some((255, 236, 190))),
            Phase::Dusk => r(Kind::Clouds, Some((255, 140, 70))),
            Phase::Night => r(Kind::Night, Some((24, 32, 80))),
        },
    }
}

/// Saat kipinde bir sonraki bölüm değişimine kadar kalan dakika. Denetleyici
/// şimdilik çalışırken dakikada bir bakıyor; bu, bölüm sınırlarının testi.
#[cfg(test)]
pub fn minutes_to_next(hour: u32, minute: u32) -> u32 {
    let now = (hour % 24) * 60 + minute.min(59);
    [5 * 60, 8 * 60, 17 * 60, 20 * 60]
        .iter()
        .map(|&b| if b > now { b - now } else { b + 24 * 60 - now })
        .min()
        .unwrap_or(60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gunun_bolumleri() {
        assert_eq!(phase(4, 59), Phase::Night);
        assert_eq!(phase(5, 0), Phase::Dawn);
        assert_eq!(phase(7, 59), Phase::Dawn);
        assert_eq!(phase(8, 0), Phase::Day);
        assert_eq!(phase(16, 59), Phase::Day);
        assert_eq!(phase(17, 0), Phase::Dusk);
        assert_eq!(phase(19, 59), Phase::Dusk);
        assert_eq!(phase(20, 0), Phase::Night);
        assert_eq!(phase(0, 0), Phase::Night);
        assert_eq!(phase(23, 30), Phase::Night);
        // Taşan saat de güvenli.
        assert_eq!(phase(29, 0), Phase::Dawn);
    }

    #[test]
    fn saat_kipi_sahneye_cozulur() {
        assert_eq!(resolve(LiveScene::Clock, 2, 0).unwrap().kind, Kind::Night);
        assert_eq!(resolve(LiveScene::Clock, 6, 0).unwrap().kind, Kind::Clouds);
        assert_eq!(resolve(LiveScene::Clock, 12, 0).unwrap().kind, Kind::Clouds);
        assert_eq!(resolve(LiveScene::Clock, 18, 0).unwrap().tint, Some((255, 140, 70)));
        assert_eq!(resolve(LiveScene::Clock, 21, 0).unwrap().kind, Kind::Night);
        assert_eq!(resolve(LiveScene::None, 12, 0), None);
        // Sabit sahneler saatten bağımsız.
        for h in 0..24 {
            assert_eq!(resolve(LiveScene::Rain, h, 0).unwrap().kind, Kind::Rain);
            assert_eq!(resolve(LiveScene::Snow, h, 0).unwrap().kind, Kind::Snow);
        }
    }

    #[test]
    fn sonraki_degisim() {
        assert_eq!(minutes_to_next(4, 30), 30);
        assert_eq!(minutes_to_next(12, 0), 5 * 60);
        assert_eq!(minutes_to_next(20, 0), 9 * 60);
        assert_eq!(minutes_to_next(23, 0), 6 * 60);
    }
}
