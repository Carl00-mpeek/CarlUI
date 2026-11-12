//! İlk kurulum karşılaması: ayar dosyası yeni oluşturulduysa
//! (`general.welcomed = false`) modüller açıldıktan birkaç saniye sonra
//! `carlui-settings.exe --welcome` başlatılır. Tam ekran bir uygulama varsa
//! beklenir. Karşılama penceresi açılır açılmaz işareti yazar; bu yüzden
//! akış bir daha gelmez. Öne geçme izni verilmez: oturum açılırken kullanıcı
//! başka bir pencereye geçtiyse karşılama onun önüne atlamaz.

use carlui_core::{config::Config, info, paths};
use std::sync::atomic::{AtomicBool, Ordering};
use windows::{
    core::*,
    Win32::{
        Foundation::HWND,
        UI::{Shell::ShellExecuteW, WindowsAndMessaging::*},
    },
};

pub const TIMER: usize = 7;
/// Modüller açıldıktan sonra ilk bakış ve tam ekranda yeniden deneme aralığı.
const FIRST_MS: u32 = 3000;
const RETRY_MS: u32 = 5000;

/// Bu oturumda bir kez başlatıldı mı (işaret yazılamasa bile tekrar açılmaz).
static LAUNCHED: AtomicBool = AtomicBool::new(false);

/// Gerekiyorsa ilk denemeyi kurar.
pub fn arm(hwnd: HWND, cfg: &Config) {
    if cfg.needs_welcome() {
        unsafe { SetTimer(hwnd, TIMER, FIRST_MS, None) };
    }
}

/// Zamanlayıcı: tam ekran varsa sonra yeniden dener, yoksa bir kez başlatır.
pub fn tick(hwnd: HWND, cfg: &Config, fullscreen: bool) {
    unsafe {
        let _ = KillTimer(hwnd, TIMER);
    }
    if !cfg.needs_welcome() || LAUNCHED.load(Ordering::Relaxed) {
        return;
    }
    if fullscreen {
        unsafe { SetTimer(hwnd, TIMER, RETRY_MS, None) };
        return;
    }
    LAUNCHED.store(true, Ordering::Relaxed);
    info!("ilk kurulum: karşılama açılıyor");
    let exe = paths::sibling_exe("carlui-settings.exe");
    unsafe {
        ShellExecuteW(HWND::default(), w!("open"), &HSTRING::from(exe.as_os_str()), w!("--welcome"), PCWSTR::null(), SW_SHOWNORMAL);
    }
}
