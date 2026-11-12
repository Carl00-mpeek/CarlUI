//! Canlı dock'un ne zaman çalışacağına karar veren saf durum makinesi.
//! Windows'a dokunmaz; girdiler `Signals`, çıktı `State` ve zamanlayıcı
//! kararıdır. Kapalı ya da duraklatılmışken hiçbir zamanlayıcı kurulmaz.

/// Neden duraklatıldı (günlük ve hata ayıklama için).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// Dock gizli (tam ekran bastırması ya da otomatik gizlemede kaymış).
    Hidden,
    /// Önde tam ekran uygulama/oyun var.
    Fullscreen,
    /// Oturum kilitli.
    Locked,
    /// Ekran kapalı.
    DisplayOff,
    /// Pil tasarrufu açık.
    BatterySaver,
    /// Pilde ve "pilde duraklat" açık.
    Battery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Özellik kapalı: hiçbir kaynak yok.
    Off,
    /// Duraklatıldı: son kare donuk kalır, zamanlayıcı yok.
    Paused(Reason),
    /// Hareketi azalt açık: tek bir durağan kare, zamanlayıcı yok.
    Static,
    /// Çalışıyor: zamanlayıcı efektlerin istediği aralıkla kurulur.
    Running,
}

/// Duraklatma kararının girdileri.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Signals {
    pub enabled: bool,
    pub suppressed: bool,
    pub slid_away: bool,
    pub fullscreen: bool,
    pub locked: bool,
    pub display_off: bool,
    pub battery_saver: bool,
    pub on_battery: bool,
    pub pause_on_battery: bool,
    pub reduce_motion: bool,
}

impl Signals {
    pub fn state(&self) -> State {
        if !self.enabled {
            return State::Off;
        }
        // Görünmezlik nedenleri önce: gizliyken durağan kare bile çizilmez.
        let reason = if self.suppressed || self.slid_away {
            Some(Reason::Hidden)
        } else if self.fullscreen {
            Some(Reason::Fullscreen)
        } else if self.locked {
            Some(Reason::Locked)
        } else if self.display_off {
            Some(Reason::DisplayOff)
        } else if self.battery_saver {
            Some(Reason::BatterySaver)
        } else if self.on_battery && self.pause_on_battery {
            Some(Reason::Battery)
        } else {
            None
        };
        match reason {
            Some(r) => State::Paused(r),
            None if self.reduce_motion => State::Static,
            None => State::Running,
        }
    }
}

impl State {
    /// Efektlerin istediği bekleme (ms) verildiğinde kurulacak zamanlayıcı.
    /// Yalnızca `Running` zamanlayıcı kurar; çok kısa aralıklar en az 33 ms'ye
    /// (≈30 fps) yükseltilir, çok uzunlar Win32 sınırında tutulur.
    pub fn timer(&self, want: Option<u32>) -> Option<u32> {
        match self {
            State::Running => want.map(|ms| ms.clamp(MIN_INTERVAL_MS, 0x7FFF_FFFF)),
            _ => None,
        }
    }

    /// Evcil hayvan penceresi görünür mü? (Gizli nedenlerde saklanır;
    /// pil/kilit gibi durumlarda donuk kare görünür kalabilir.)
    pub fn visible(&self) -> bool {
        !matches!(self, State::Off | State::Paused(Reason::Hidden) | State::Paused(Reason::Fullscreen))
    }
}

/// En kısa zamanlayıcı aralığı: 30 fps'yi aşmayız.
pub const MIN_INTERVAL_MS: u32 = 33;

#[cfg(test)]
mod tests {
    use super::*;

    fn on() -> Signals {
        Signals { enabled: true, pause_on_battery: true, ..Default::default() }
    }

    #[test]
    fn kapaliyken_zamanlayici_yok() {
        let s = Signals::default();
        assert_eq!(s.state(), State::Off);
        assert_eq!(s.state().timer(Some(50)), None);
        assert!(!s.state().visible());
    }

    #[test]
    fn duraklatma_nedenleri_zamanlayici_kurmaz() {
        let cases: [(fn(&mut Signals), Reason); 7] = [
            (|s| s.suppressed = true, Reason::Hidden),
            (|s| s.slid_away = true, Reason::Hidden),
            (|s| s.fullscreen = true, Reason::Fullscreen),
            (|s| s.locked = true, Reason::Locked),
            (|s| s.display_off = true, Reason::DisplayOff),
            (|s| s.battery_saver = true, Reason::BatterySaver),
            (|s| s.on_battery = true, Reason::Battery),
        ];
        for (set, reason) in cases {
            let mut s = on();
            set(&mut s);
            assert_eq!(s.state(), State::Paused(reason));
            assert_eq!(s.state().timer(Some(50)), None, "{reason:?}");
        }
    }

    #[test]
    fn pilde_ayar_kapaliysa_calisir() {
        let s = Signals { on_battery: true, pause_on_battery: false, ..on() };
        assert_eq!(s.state(), State::Running);
    }

    #[test]
    fn hareketi_azalt_durgun_kare() {
        let s = Signals { reduce_motion: true, ..on() };
        assert_eq!(s.state(), State::Static);
        assert_eq!(s.state().timer(Some(50)), None);
        assert!(s.state().visible());
        // Gizlilik hareketi azalttan önce gelir.
        assert_eq!(Signals { slid_away: true, ..s }.state(), State::Paused(Reason::Hidden));
    }

    #[test]
    fn calisirken_aralik_sinirli() {
        let s = on().state();
        assert_eq!(s, State::Running);
        assert_eq!(s.timer(Some(5)), Some(MIN_INTERVAL_MS));
        assert_eq!(s.timer(Some(100)), Some(100));
        // Uyuyan kedi, sahne yok: istek yoksa zamanlayıcı da yok.
        assert_eq!(s.timer(None), None);
    }
}
