//! Bu modül pencere yerlerini kendiliğinden hatırlar. Beş saniyede bir açık
//! pencerelerin yeri, o anki ekran dizilimi için bellekte tutulur. Ekran
//! çıkınca dizilim değişir; o andan sonra görülen yığılma mevcut dizilimin
//! kaydına yazılır, eski dizilimin kaydı bozulmaz. Ekran geri gelince ya da
//! uykudan uyanınca o dizilimin kaydı geri konur. Tam ekran oyun varken pencere
//! oynatılmaz. Ayar: `tools.window_memory`.

use carlui_core::{info, layouts};
use std::collections::HashMap;

#[derive(Default)]
pub struct Memory {
    /// Dizilim imzası → pencere (hwnd) → son yer.
    seen: HashMap<String, HashMap<isize, layouts::Spot>>,
    /// Son bilinen dizilim. Değişiklik sonrası geri koyma bitene dek kayıt alınmaz.
    current: String,
    settling: bool,
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
    }

    /// Ekran dizilimi değişti ya da bilgisayar uyumaya gidiyor/uyanıyor:
    /// geri koyma yapılana kadar yerler yazılmasın.
    pub fn hold(&mut self) {
        self.settling = true;
    }

    /// Yeni dizilimin kaydını geri koyar. `blocked`: tam ekran uygulama var.
    pub fn restore(&mut self, blocked: bool) {
        self.settling = false;
        let sig = layouts::signature();
        self.current = sig.clone();
        if blocked {
            return;
        }
        let Some(spots) = self.seen.get(&sig).cloned() else { return };
        // Yanıt vermeyen bir pencere ana süreci bekletmesin.
        std::thread::spawn(move || {
            let moved = spots.iter().filter(|(h, s)| layouts::place(**h, s)).count();
            info!("pencere yerleri geri kondu: {moved}/{}", spots.len());
        });
    }

    /// Ayar kapandı: bellek boşalsın.
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}
