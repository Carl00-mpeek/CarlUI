//! Pano. Kopyalanan değer KDE'nin pano geçmişine yazılmasın diye
//! `x-kde-passwordManagerHint` işaretiyle konur (`exclude_from_history`) ve
//! ayarlanan süre sonra, panoda hâlâ aynı değer duruyorsa silinir.
//!
//! Wayland'de pano içeriği onu koyan uygulama açıkken yaşar; bu yüzden
//! `Clipboard` nesnesi uygulama boyunca tutulur.

use std::time::{Duration, Instant};
use zeroize::Zeroizing;

#[derive(Default)]
pub struct Clip {
    cb: Option<arboard::Clipboard>,
    ours: Option<(Zeroizing<String>, Instant)>,
}

impl Clip {
    fn board(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.cb.is_none() {
            match arboard::Clipboard::new() {
                Ok(c) => self.cb = Some(c),
                Err(e) => eprintln!("pano açılamadı: {e}"),
            }
        }
        self.cb.as_mut()
    }

    /// Kopyalar; `secs` > 0 ise o kadar saniye sonra silinmek üzere işaretler.
    pub fn copy(&mut self, text: &str, secs: u32) -> bool {
        let Some(cb) = self.board() else { return false };
        #[cfg(target_os = "linux")]
        let r = {
            use arboard::SetExtLinux;
            cb.set().exclude_from_history().text(text.to_string())
        };
        #[cfg(not(target_os = "linux"))]
        let r = cb.set_text(text.to_string());
        match r {
            Ok(()) => {
                self.ours = (secs > 0).then(|| (Zeroizing::new(text.to_string()), Instant::now() + Duration::from_secs(secs as u64)));
                true
            }
            Err(e) => {
                eprintln!("panoya yazılamadı: {e}");
                false
            }
        }
    }

    /// Süresi dolduysa panoyu temizler. Bir sonraki denetime kalan süre.
    pub fn tick(&mut self) -> Option<Duration> {
        let (_, at) = self.ours.as_ref()?;
        let now = Instant::now();
        if now < *at {
            return Some(*at - now);
        }
        let (value, _) = self.ours.take()?;
        if let Some(cb) = self.board() {
            // Kullanıcı bu arada başka bir şey kopyaladıysa dokunma.
            if cb.get_text().is_ok_and(|t| Zeroizing::new(t).as_str() == value.as_str()) {
                let _ = cb.clear();
            }
        }
        None
    }

    /// Çıkışta: bizim değerimiz panodaysa sil.
    pub fn clear_now(&mut self) {
        if let Some((_, at)) = self.ours.as_mut() {
            *at = Instant::now();
        }
        self.tick();
    }
}
