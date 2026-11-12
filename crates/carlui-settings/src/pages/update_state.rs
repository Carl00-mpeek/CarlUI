//! Güncelleme sayfasının durum makinesi (çizimden ve ağdan bağımsız, sınanabilir).
//! Akış: boşta → denetleniyor → güncel | yeni sürüm var → indiriliyor /
//! doğrulanıyor / kurucu açılıyor → kurucu açıldı; her adımda hata olabilir,
//! hatadan aynı iş yeniden denenir.

use carlui_core::update::{newer, Release, Stage};

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Installing(Release, Stage),
    /// Kurucu açıldı; CarlUI'yi o kapatıp yeniden açacak.
    Launched,
    /// `release` varsa kurulum, yoksa denetim yeniden denenir.
    Failed { msg: String, release: Option<Release> },
}

/// Arka plan iş parçacığından gelen haberler.
#[derive(Clone, Debug)]
pub enum Event {
    Checked(Result<Release, String>),
    Stage(Stage),
    Installed(Result<(), String>),
}

impl Phase {
    /// Arka planda bir iş sürüyor mu (düğme kapalı, dönen simge).
    pub fn busy(&self) -> bool {
        matches!(self, Phase::Checking | Phase::Installing(..))
    }

    /// "Güncellemeleri denetle" basılabilir mi?
    pub fn can_check(&self) -> bool {
        !self.busy() && !matches!(self, Phase::Launched)
    }

    /// Kurulacak sürüm (Güncelle ya da kurulum hatasından sonra Yeniden dene).
    pub fn installable(&self) -> Option<&Release> {
        match self {
            Phase::Available(r) | Phase::Failed { release: Some(r), .. } if r.installable() => Some(r),
            _ => None,
        }
    }

    /// Bir haberden sonraki durum. Beklenmeyen haber (ör. eski bir denetimin
    /// geç gelen sonucu kurulum sürerken) durumu değiştirmez.
    pub fn next(&self, ev: Event, current: &str) -> Phase {
        match (self, ev) {
            (Phase::Checking, Event::Checked(Ok(r))) => {
                if newer(&r.tag, current) {
                    Phase::Available(r)
                } else {
                    Phase::UpToDate
                }
            }
            (Phase::Checking, Event::Checked(Err(msg))) => Phase::Failed { msg, release: None },
            (Phase::Installing(r, _), Event::Stage(s)) => Phase::Installing(r.clone(), s),
            (Phase::Installing(..), Event::Installed(Ok(()))) => Phase::Launched,
            (Phase::Installing(r, _), Event::Installed(Err(msg))) => Phase::Failed { msg, release: Some(r.clone()) },
            (p, _) => p.clone(),
        }
    }
}

// --- Yerleşim (One UI gibi: grup ortada, düğme dipte) ---

/// Marka (66) + sürüm (22) + durumdan önceki boşluk (48).
pub const HERO_H: f32 = 136.0;
/// Durum bloğunun en az yüksekliği (ana satır + alt satır).
pub const STATUS_MIN: f32 = 46.0;
/// Anahtar kartıyla düğme arası.
pub const BUTTON_GAP: f32 = 22.0;
/// "Otomatik denetim" başlığı (42) + tek satırlık anahtar kartı (50 + 6).
const TOGGLE_BLOCK: f32 = 98.0;
/// Düğmenin altındaki boşluk: "Kaydedildi" bildirimi düğmenin üstüne binmesin.
const BOTTOM_PAD: f32 = 40.0;
const TOP_MIN: f32 = 24.0;
/// Grupla alttaki yığın arasında en az boşluk.
const MIN_GAP: f32 = 40.0;

/// Alttaki yığının (anahtar + düğme [+ bağlantı] + dip payı) yüksekliği.
pub fn bottom_stack_h(link: bool) -> f32 {
    TOGGLE_BLOCK + BUTTON_GAP + 44.0 + if link { 36.0 } else { 0.0 } + BOTTOM_PAD
}

/// Sayfanın üstüne göre konumlar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    /// Grubun (marka, sürüm, durum, notlar) üstü.
    pub group: f32,
    /// Alttaki yığının üstü.
    pub bottom: f32,
    /// Sabitlenmişse sayfanın sonu (kaydırma yok).
    pub end: f32,
    /// false: pencere çok kısa, sıradan akış ve kaydırma.
    pub pinned: bool,
}

/// `avail`: sayfa alanının yüksekliği. Sığarsa alttaki yığın dibe sabitlenir,
/// grup üstte kalan alanda dikeyde ortalanır; sığmazsa hiçbir şey üst üste
/// binmesin diye sırayla dizilir (sayfa kayar).
pub fn place(avail: f32, group: f32, bottom: f32) -> Place {
    if avail >= TOP_MIN + group + MIN_GAP + bottom {
        let space = avail - bottom;
        Place { group: ((space - group) / 2.0).max(TOP_MIN), bottom: space, end: avail, pinned: true }
    } else {
        Place { group: 32.0, bottom: 32.0 + group + MIN_GAP, end: 0.0, pinned: false }
    }
}

/// Vurgu dolgusu üstünde okunur yazı rengi: göreli parlaklık (WCAG) eşiğe
/// göre siyah ya da beyaz. 0.179 iki karşıtlığın eşitlendiği nokta.
pub fn on_accent(c: u32) -> u32 {
    let ch = |s: u32| {
        let v = ((c >> s) & 0xFF) as f32 / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    let l = 0.2126 * ch(16) + 0.7152 * ch(8) + 0.0722 * ch(0);
    if l > 0.179 { 0x000000 } else { 0xFFFFFF }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(tag: &str, installable: bool) -> Release {
        Release { tag: tag.into(), url: if installable { "https://x/s.exe".into() } else { String::new() }, sha: "a".repeat(64), ..Default::default() }
    }

    #[test]
    fn denetim_sonuclari() {
        let c = Phase::Checking;
        assert_eq!(c.next(Event::Checked(Ok(rel("v1.0.0", true))), "1.0.0"), Phase::UpToDate);
        assert_eq!(c.next(Event::Checked(Ok(rel("v0.9.0", true))), "1.0.0"), Phase::UpToDate);
        assert_eq!(c.next(Event::Checked(Ok(rel("v1.1.0", true))), "1.0.0"), Phase::Available(rel("v1.1.0", true)));
        let f = c.next(Event::Checked(Err("yok".into())), "1.0.0");
        assert_eq!(f, Phase::Failed { msg: "yok".into(), release: None });
        // Denetim hatasından sonra kurulacak bir şey yok; yeniden denetlenebilir.
        assert!(f.installable().is_none() && f.can_check());
    }

    #[test]
    fn kurulum_akisi() {
        let r = rel("v1.1.0", true);
        let mut p = Phase::Installing(r.clone(), Stage::Downloading);
        assert!(p.busy() && !p.can_check());
        p = p.next(Event::Stage(Stage::Verifying), "1.0.0");
        assert_eq!(p, Phase::Installing(r.clone(), Stage::Verifying));
        // Kurulum sürerken gelen eski denetim sonucu yok sayılır.
        assert_eq!(p.next(Event::Checked(Ok(rel("v2.0.0", true))), "1.0.0"), p);
        let fail = p.next(Event::Installed(Err("doğrulanamadı".into())), "1.0.0");
        assert_eq!(fail.installable(), Some(&r));
        assert_eq!(p.next(Event::Installed(Ok(())), "1.0.0"), Phase::Launched);
        assert!(!Phase::Launched.can_check());
    }

    #[test]
    fn kurulamayan_surum() {
        // Kurucusu ya da özeti olmayan sürüm uygulama içinden kurulmaz (yalnızca sayfası).
        assert!(Phase::Available(rel("v1.1.0", false)).installable().is_none());
        assert!(Phase::Available(rel("v1.1.0", true)).installable().is_some());
        assert!(Phase::Idle.installable().is_none() && !Phase::Idle.busy());
    }

    #[test]
    fn yerlesim() {
        let b = bottom_stack_h(false);
        // Büyük pencere: yığın dipte, grup üstte kalan alanda ortada, kaydırma yok.
        let pl = place(800.0, 230.0, b);
        assert!(pl.pinned);
        assert_eq!(pl.bottom, 800.0 - b);
        assert!(((pl.group - (pl.bottom - pl.group - 230.0)).abs()) < 0.01);
        // Kısa pencere: üst üste binmez, akış.
        let pl = place(300.0, 230.0, b);
        assert!(!pl.pinned && pl.bottom >= pl.group + 230.0);
    }

    #[test]
    fn vurgu_yazisi() {
        // Açık vurgularda siyah, koyularda beyaz yazı.
        assert_eq!(on_accent(0xFFFFFF), 0x000000);
        assert_eq!(on_accent(0x9AD0FF), 0x000000);
        assert_eq!(on_accent(0x1A237E), 0xFFFFFF);
        assert_eq!(on_accent(0x333333), 0xFFFFFF);
        assert_eq!(on_accent(0x000000), 0xFFFFFF);
    }
}
