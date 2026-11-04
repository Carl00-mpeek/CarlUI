//! Art arda yanlış parola / kurtarma anahtarı: 3. hatadan sonra artan bekleme
//! (5, 10, 20 … sn, en çok 5 dk). Sayaç kasanın yanındaki `unlock.json`'a
//! yazılır; böylece kasa penceresini kapatıp açmak beklemeyi sıfırlamaz.
//!
//! Bu, klavye başındaki birine karşı bir frendir. Dosyayı kopyalayan biri
//! parolayı çevrimdışı dener; ona karşı koruma Argon2'nin maliyetidir.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Saat geri alınsa da bekleme bundan uzun sürmez.
pub const MAX_WAIT: u64 = 300;
const FILE: &str = "unlock.json";

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq)]
pub struct Throttle {
    fails: u32,
    /// Beklemenin bittiği Unix saniyesi.
    until: u64,
}

/// `fails` hatadan sonraki bekleme (sn).
pub fn wait_for(fails: u32) -> u64 {
    if fails < 3 {
        0
    } else {
        (5u64 << (fails - 3).min(6)).min(MAX_WAIT)
    }
}

impl Throttle {
    pub fn load(dir: &Path) -> Self {
        std::fs::read(dir.join(FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn save(&self, dir: &Path) {
        let path = dir.join(FILE);
        if *self == Self::default() {
            let _ = std::fs::remove_file(path);
        } else if let Ok(b) = serde_json::to_vec(self) {
            let _ = std::fs::write(path, b);
        }
    }

    pub fn fail(&mut self, dir: &Path, now: u64) {
        self.fails = self.fails.saturating_add(1);
        self.until = now + wait_for(self.fails);
        self.save(dir);
    }

    pub fn reset(&mut self, dir: &Path) {
        *self = Self::default();
        self.save(dir);
    }

    /// Kalan bekleme (sn); yoksa None.
    pub fn left(&self, now: u64) -> Option<u64> {
        (self.until > now).then(|| (self.until - now).min(MAX_WAIT))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule() {
        let w: Vec<u64> = (0..12).map(wait_for).collect();
        assert_eq!(w, [0, 0, 0, 5, 10, 20, 40, 80, 160, 300, 300, 300]);
        assert_eq!(wait_for(u32::MAX), MAX_WAIT);
    }

    #[test]
    fn survives_restart_and_clock() {
        let d = std::env::temp_dir().join(format!("carlui-throttle-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        let mut t = Throttle::default();
        for _ in 0..4 {
            t.fail(&d, 1000);
        }
        assert_eq!(t.left(1000), Some(10));
        // Pencere kapanıp açıldı: sayaç diskten gelir.
        let back = Throttle::load(&d);
        assert_eq!(back, t);
        assert_eq!(back.left(1005), Some(5));
        assert_eq!(back.left(1010), None);
        // Saat geri alındı: bekleme yine en çok 5 dk.
        assert_eq!(back.left(0), Some(MAX_WAIT));
        // Doğru parola: dosya silinir.
        t.reset(&d);
        assert!(!d.join(FILE).exists());
        assert_eq!(Throttle::load(&d).left(1000), None);
        // Bozuk dosya beklemesiz sayılır ama çökmez.
        std::fs::write(d.join(FILE), b"{bozuk").unwrap();
        assert_eq!(Throttle::load(&d), Throttle::default());
        let _ = std::fs::remove_dir_all(&d);
    }
}
