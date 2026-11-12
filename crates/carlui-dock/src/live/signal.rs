//! Hayvan tepkilerinin `Live`'a bağlandığı yer: adadan gelen bus sinyalleri
//! ("pet-note", "pet-music") ve okşama alanının tıklaması. Hiçbiri kendi
//! zamanlayıcısını kurmaz; yalnızca hayvanın sıradaki tikini öne çeker.

use super::{pause::State, Live};
use std::time::Instant;

/// Adanın bildirim sinyali (içerik taşımaz).
const NOTE: &str = "pet-note";
/// Adanın müzik sinyali (`{"playing": bool}`).
const MUSIC: &str = "pet-music";
/// Kum havuzu denemesi için dock penceresine gönderilen mesaj.
pub const WM_APP_PET_DEMO: u32 = super::pet_hit::WM_APP_PET_CLICK + 1;

impl Live {
    /// Kurulumda (dock açılışı ya da ayar değişimi) şu an müzik çalıyor mu
    /// diye bir kez SMTC taraması ister; sonuç `WM_APP_MEDIA` ile gelir.
    /// Sonraki değişimler adadan `pet-music` ile gelir (dock sürekli izlemez).
    pub(super) fn ask_music(&mut self) {
        if self.cfg.reactions && self.cfg.has_pet() && self.loading.is_some() {
            // Liste değişmediyse tarama mesaj göndermez: bilinen son durumla başla.
            self.music = crate::media::playing();
            crate::media::refresh(self.hwnd);
        }
    }

    /// Dock'un medya listesi yenilendi (kurulum taraması ya da önizleme).
    pub(super) fn media_changed(&mut self) {
        self.set_music(crate::media::playing());
    }

    fn set_music(&mut self, on: bool) {
        // Hayvan henüz yüklenmediyse de saklanır; yüklenince uygulanır.
        self.music = on;
        if let Some(p) = self.perch.as_mut() {
            p.music(on);
        }
    }

    /// Bus'tan gelen hayvan sinyali. Tanınmayan adlar yok sayılır.
    pub fn pet_signal(&mut self, name: &str, data: &serde_json::Value) {
        match name {
            MUSIC => self.set_music(data.get("playing").and_then(|v| v.as_bool()).unwrap_or(false)),
            NOTE => {
                // Duraklamışken (gizli, tam ekran, kilit…) hiçbir şey yapılmaz.
                if !self.cfg.reactions || self.state != State::Running {
                    return;
                }
                if let Some(p) = self.perch.as_mut() {
                    p.note();
                    self.pet_due = Instant::now();
                    self.arm();
                }
            }
            _ => {}
        }
    }

    /// Kum havuzu denemesi (`WM_APP_PET_DEMO`, yalnızca `CARLUI_DOCK_SANDBOX=1`
    /// ile; kum havuzu bus'a bağlanmadığı için sinyaller böyle verilir).
    /// wParam: 1 müzik açık, 2 müzik kapalı, 3 bildirim, 4 boşta, 5 boşta bitti.
    pub(super) fn pet_demo(&mut self, wp: usize) {
        if !crate::sandbox::on() {
            return;
        }
        match wp {
            1 | 2 => self.set_music(wp == 1),
            3 => self.pet_signal(NOTE, &serde_json::Value::Null),
            4 | 5 => {
                if let Some(p) = self.perch.as_mut() {
                    p.fake_idle(wp == 4);
                }
            }
            _ => return,
        }
        if self.perch.is_some() && self.state == State::Running {
            self.pet_due = Instant::now();
            self.arm();
        }
    }

    /// Okşama alanına tıklandı.
    pub(super) fn pet_click(&mut self) {
        if !self.cfg.reactions || self.state != State::Running {
            return;
        }
        if let Some(p) = self.perch.as_mut() {
            if p.click() {
                self.pet_due = Instant::now();
                self.arm();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // Adanın gönderdiği adlarla aynı olmalı (carlui-island/src/pet_signal.rs).
    #[test]
    fn sinyal_adlari_adayla_ayni() {
        assert_eq!(super::NOTE, "pet-note");
        assert_eq!(super::MUSIC, "pet-music");
    }
}
