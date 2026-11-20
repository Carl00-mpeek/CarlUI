//! Bu modül pencere yerlerini kendiliğinden hatırlar. Beş saniyede bir açık
//! pencerelerin yeri, o anki ekran dizilimi için bellekte tutulur. Ekran
//! çıkınca dizilim değişir; o andan sonra görülen yığılma mevcut dizilimin
//! kaydına yazılır, eski dizilimin kaydı bozulmaz. Ekran geri gelince ya da
//! uykudan uyanınca o dizilimin kaydı geri konur. Tam ekran oyun varken pencere
//! oynatılmaz. Ayar: `tools.window_memory`.
//!
//! Ekrana çapa: Tam ekran oyun ana ekranın çözünürlüğünü değiştirince (ör. CS2
//! 1568x1080) Windows sağdaki ekranı yeni genişliğe göre kaydırır (X 1920 → 1568).
//! Kenarlıksız tam ekran ya da büyütülmüş pencereler bu kaydırmayı her zaman
//! izlemez ve ikinci ekranda yana kaymış kalır. Dizilim değişince, boyutu aynı
//! kalıp yalnızca yeri değişen ekrandaki pencereler ekranla birlikte taşınır.
//! Z sırası ve odak değişmez; bu yüzden oyun tam ekrandayken de güvenlidir.

use carlui_core::{info, layouts, win};
use std::collections::HashMap;
use windows::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    UI::WindowsAndMessaging::*,
};

/// Bir pencerenin son kayıtta bulunduğu ekran ve ekran içindeki yeri.
#[derive(Clone, Debug)]
struct Anchor {
    hwnd: isize,
    device: String,
    /// Kayıt anındaki ekran dikdörtgeni (sol, üst, sağ, alt).
    monitor: [i32; 4],
    /// Kayıt anındaki pencere dikdörtgeni (ekran koordinatı).
    rect: [i32; 4],
}

#[derive(Default)]
pub struct Memory {
    /// Dizilim imzası → pencere (hwnd) → son yer.
    seen: HashMap<String, HashMap<isize, layouts::Spot>>,
    /// Son kayıttaki pencerelerin ekranları; dizilim değişince ekranla taşımak için.
    anchors: Vec<Anchor>,
    /// Son bilinen dizilim. Değişiklik sonrası geri koyma bitene dek kayıt alınmaz.
    current: String,
    settling: bool,
}

fn rect4(r: RECT) -> [i32; 4] {
    [r.left, r.top, r.right, r.bottom]
}

/// Ekran aygıt adı → ekran dikdörtgeni.
fn monitor_rects() -> HashMap<String, [i32; 4]> {
    win::monitors().into_iter().map(|m| (win::device_name(m.handle), rect4(m.rect))).collect()
}

fn anchors_now() -> Vec<Anchor> {
    let mons = monitor_rects();
    layouts::capture()
        .into_iter()
        .filter_map(|e| unsafe {
            let h = HWND(e.hwnd as _);
            if IsIconic(h).as_bool() {
                return None;
            }
            let mut r = RECT::default();
            GetWindowRect(h, &mut r).ok()?;
            let device = win::device_name(MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST));
            let monitor = *mons.get(&device)?;
            Some(Anchor { hwnd: e.hwnd, device, monitor, rect: rect4(r) })
        })
        .collect()
}

/// Ekranı yalnızca yer değiştirdiyse (boyutu aynı) pencerenin ekranla birlikte
/// gideceği yer. Ekran boyutu değiştiyse (oyunun kendi ekranı) ya da hiç
/// kıpırdamadıysa `None`: o pencereye dokunulmaz.
fn follow_monitor(rect: [i32; 4], was: [i32; 4], now: [i32; 4]) -> Option<[i32; 4]> {
    let size = |m: [i32; 4]| (m[2] - m[0], m[3] - m[1]);
    if size(was) != size(now) {
        return None;
    }
    let (dx, dy) = (now[0] - was[0], now[1] - was[1]);
    Some([rect[0] + dx, rect[1] + dy, rect[2] + dx, rect[3] + dy])
}

/// Kayıttaki pencereleri, yeri değişmiş ekranlarıyla birlikte taşır.
fn reanchor(anchors: Vec<Anchor>) {
    let mons = monitor_rects();
    // Ekran takılıp çıkarıldıysa çapa değil, kayıtlı dizilim geri koyması geçerli.
    if anchors.iter().any(|a| !mons.contains_key(&a.device)) {
        return;
    }
    let mut moved = 0;
    for a in &anchors {
        let Some(want) = follow_monitor(a.rect, a.monitor, mons[&a.device]) else { continue };
        unsafe {
            let h = HWND(a.hwnd as _);
            if !IsWindow(h).as_bool() || IsHungAppWindow(h).as_bool() || IsIconic(h).as_bool() {
                continue;
            }
            let mut r = RECT::default();
            if GetWindowRect(h, &mut r).is_err() || rect4(r) == want {
                continue;
            }
            let flags = SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_ASYNCWINDOWPOS;
            if SetWindowPos(h, None, want[0], want[1], want[2] - want[0], want[3] - want[1], flags).is_ok() {
                moved += 1;
            }
        }
    }
    if moved > 0 {
        info!("ekranı kayan pencereler ekranıyla taşındı: {moved}");
    }
}

impl Memory {
    /// Düzenli kayıt: dizilim değişmişse yerleşmesini bekler.
    pub fn snapshot(&mut self) {
        if self.settling {
            return;
        }
        let sig = layouts::signature();
        if !self.current.is_empty() && sig != self.current {
            return;
        }
        self.current = sig.clone();
        let spots = layouts::capture().into_iter().map(|e| (e.hwnd, e.spot)).collect();
        self.seen.insert(sig, spots);
        self.anchors = anchors_now();
    }

    /// Ekran dizilimi değişti ya da bilgisayar uyumaya gidiyor/uyanıyor:
    /// geri koyma yapılana kadar yerler yazılmasın.
    pub fn hold(&mut self) {
        self.settling = true;
    }

    /// Yeni dizilimin kaydını geri koyar. `blocked`: tam ekran uygulama var.
    /// Engelliyken de pencereler kayan ekranlarıyla taşınır (z sırası değişmeden).
    pub fn restore(&mut self, blocked: bool) {
        self.settling = false;
        let sig = layouts::signature();
        let changed = sig != self.current;
        self.current = sig.clone();
        let anchors = if changed || blocked { self.anchors.clone() } else { Vec::new() };
        let spots = if blocked { None } else { self.seen.get(&sig).cloned() };
        if anchors.is_empty() && spots.is_none() {
            return;
        }
        // Yanıt vermeyen bir pencere ana süreci bekletmesin.
        std::thread::spawn(move || {
            reanchor(anchors);
            if let Some(spots) = spots {
                let moved = spots.iter().filter(|(h, s)| layouts::place(**h, s)).count();
                info!("pencere yerleri geri kondu: {moved}/{}", spots.len());
            }
        });
    }

    /// Ayar kapandı: bellek boşalsın.
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANA: [i32; 4] = [0, 0, 1920, 1080];
    const IKINCI: [i32; 4] = [1920, 0, 3840, 1080];

    #[test]
    fn oyun_cozunurlugu_degisince_ikinci_ekrandaki_pencere_ekranla_gider() {
        // CS2 ana ekranı 1568x1080 yapınca ikinci ekran X=1568'e kayar.
        let simdi = [1568, 0, 3488, 1080];
        assert_eq!(follow_monitor(IKINCI, IKINCI, simdi), Some(simdi));
        assert_eq!(follow_monitor([2000, 100, 2800, 700], IKINCI, simdi), Some([1648, 100, 2448, 700]));
        // Oyundan çıkınca ekran geri gelir, pencere de eski yerine döner.
        assert_eq!(follow_monitor(IKINCI, IKINCI, IKINCI), Some(IKINCI));
    }

    #[test]
    fn boyutu_degisen_ekrandaki_pencereye_dokunulmaz() {
        // Oyunun kendi ekranı: boyut değişti, oyun penceresi oynatılmaz.
        assert_eq!(follow_monitor(ANA, ANA, [0, 0, 1568, 1080]), None);
        assert_eq!(follow_monitor([100, 100, 500, 500], ANA, [0, 0, 2560, 1440]), None);
    }
}
