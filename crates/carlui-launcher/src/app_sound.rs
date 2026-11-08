//! Bu modül "yalnızca Discord'un sesini kıs" sorununu Windows ses karıştırıcısının
//! uygulama başına oturumlarıyla çözüyor. Sistem sesi değişmez. Uygulama hiç ses
//! açmadıysa Windows'ta ayarlanacak bir oturumu yoktur.

use crate::index::{self, App};
use carlui_core::quick;
use windows::Win32::Media::Audio::ISimpleAudioVolume;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sound {
    /// Yüzde.
    Level(u8),
    /// Bir kademe aç (+1) / kıs (-1).
    Step(i8),
    Mute(bool),
}

/// Bir kademe, yüzde olarak.
const STEP: f32 = 0.1;
/// Yeni açılan uygulamanın ses oturumu için bekleme.
const TRIES: u32 = 10;
const TRY_EVERY: std::time::Duration = std::time::Duration::from_millis(800);

fn sessions(app: &App) -> Vec<ISimpleAudioVolume> {
    quick::app_sessions(|pid| index::owns(app, &quick::process_exe(pid).to_lowercase(), &quick::process_aumid(pid)))
}

pub fn has_sound(app: &App) -> bool {
    !sessions(app).is_empty()
}

fn change(v: &ISimpleAudioVolume, what: Sound) {
    let none = std::ptr::null();
    unsafe {
        match what {
            Sound::Level(n) => {
                let _ = v.SetMasterVolume(n as f32 / 100.0, none);
                let _ = v.SetMute(n == 0, none);
            }
            Sound::Step(d) => {
                let now = v.GetMasterVolume().unwrap_or(1.0);
                let _ = v.SetMasterVolume((now + STEP * d as f32).clamp(0.0, 1.0), none);
                let _ = v.SetMute(false, none);
            }
            Sound::Mute(on) => {
                let _ = v.SetMute(on, none);
            }
        }
    }
}

/// Bir uygulamanın tüm oturumlarını değiştirir. "Aç ve sesini kıs" gibi komutlarda
/// oturum biraz geç açılır, bu yüzden kısa süre bekler.
pub fn apply(app: &App, what: Sound) {
    for _ in 0..TRIES {
        let list = sessions(app);
        if !list.is_empty() {
            return list.iter().for_each(|v| change(v, what));
        }
        std::thread::sleep(TRY_EVERY);
    }
    carlui_core::warn!("asistan: {} için ses oturumu yok", app.name);
}
