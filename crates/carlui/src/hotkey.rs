//! Ana sürecin üstlendiği genel kısayollar. Şimdilik yalnızca Carl Vault:
//! `shortcuts.vault` (varsayılan Ctrl+Alt+P) kasa penceresini açar ya da
//! zaten açıksa öne getirir (kasa tek örnek çalışır). Doğrulama (fare tuşu,
//! tek tuş, Windows kısayolu reddi) ortak `keys` modülündedir; ayar değişince
//! kısayol yeniden kaydedilir.

use carlui_core::{config::Config, info, paths, warn};
use windows::Win32::{Foundation::HWND, UI::Input::KeyboardAndMouse::*};

pub const VAULT: i32 = 1;

pub fn register(hwnd: HWND, cfg: &Config) {
    unsafe {
        let _ = UnregisterHotKey(hwnd, VAULT);
        let Some(c) = cfg.shortcuts.vault_combo() else { return };
        let mut mods = MOD_NOREPEAT;
        for (on, m) in [(c.mods.ctrl, MOD_CONTROL), (c.mods.alt, MOD_ALT), (c.mods.shift, MOD_SHIFT), (c.mods.win, MOD_WIN)] {
            if on {
                mods |= m;
            }
        }
        if let Err(e) = RegisterHotKey(hwnd, VAULT, mods, c.vk) {
            warn!("Vault kısayolu {} kaydedilemedi (başka bir uygulama kullanıyor olabilir): {e}", cfg.shortcuts.vault);
        }
    }
}

pub fn open_vault() {
    let exe = paths::sibling_exe("carlui-vault.exe");
    match std::process::Command::new(&exe).spawn() {
        Ok(_) => info!("Carl Vault açıldı (kısayol)"),
        Err(e) => warn!("{} açılamadı: {e}", exe.display()),
    }
}
