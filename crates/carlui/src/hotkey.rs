//! Ana sürecin üstlendiği genel kısayollar: Carl Vault (`shortcuts.vault`),
//! renk seçici (`shortcuts.color_picker`, varsayılan boş = kapalı), disk
//! analizörü (`shortcuts.disk`, varsayılan boş) ve toplu yeniden adlandırma
//! (`shortcuts.rename`), sesle yazma (`shortcuts.dictate`) ve sesli okuma (`shortcuts.speak`). Yeniden adlandırma kısayolu yalnızca Gezgin ya da
//! masaüstü öndeyken kaydedilir; böylece tarayıcıdaki Ctrl+Shift+R gibi
//! kısayollar elinden alınmaz. Doğrulama (fare tuşu, tek tuş, Windows kısayolu
//! reddi) ortak `keys` modülündedir; ayar değişince kısayollar yeniden kaydedilir.

use carlui_core::{config::Config, info, keys::Combo, paths, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use windows::{
    core::*,
    Win32::{
        Foundation::HWND,
        UI::{Input::KeyboardAndMouse::*, Shell::ShellExecuteW, WindowsAndMessaging::*},
    },
};

pub const VAULT: i32 = 1;
pub const PICKER: i32 = 2;
pub const DISK: i32 = 3;
pub const RENAME: i32 = 4;
pub const DICTATE: i32 = 5;
pub const SPEAK: i32 = 6;
pub const ASSIST: i32 = 7;

static RENAME_ON: AtomicBool = AtomicBool::new(false);

fn reg(hwnd: HWND, id: i32, c: Option<Combo>, name: &str, text: &str) {
    let Some(c) = c else { return };
    let mut mods = MOD_NOREPEAT;
    for (on, m) in [(c.mods.ctrl, MOD_CONTROL), (c.mods.alt, MOD_ALT), (c.mods.shift, MOD_SHIFT), (c.mods.win, MOD_WIN)] {
        if on {
            mods |= m;
        }
    }
    unsafe {
        if let Err(e) = RegisterHotKey(hwnd, id, mods, c.vk) {
            warn!("{name} kısayolu {text} kaydedilemedi (başka bir uygulama kullanıyor olabilir): {e}");
        }
    }
}

pub fn register(hwnd: HWND, cfg: &Config) {
    unsafe {
        for id in [VAULT, PICKER, DISK, RENAME, DICTATE, SPEAK, ASSIST] {
            let _ = UnregisterHotKey(hwnd, id);
        }
    }
    RENAME_ON.store(false, Ordering::SeqCst);
    let s = &cfg.shortcuts;
    reg(hwnd, VAULT, s.vault_combo(), "Vault", &s.vault);
    reg(hwnd, PICKER, s.color_picker_combo(), "Renk seçici", &s.color_picker);
    reg(hwnd, DICTATE, s.dictate_combo(), "Dikte", &s.dictate);
    reg(hwnd, SPEAK, s.speak_combo(), "Sesli okuma", &s.speak);
    if cfg.ai.assistant {
        reg(hwnd, ASSIST, s.assist_combo(), "Sesli asistan", &s.assist);
    }
    if cfg.tools.disk_enabled {
        reg(hwnd, DISK, s.disk_combo(), "Disk analizörü", &s.disk);
    }
    sync_rename(hwnd, cfg);
}

fn explorer_foreground() -> bool {
    let class = carlui_core::win::class_name(unsafe { GetForegroundWindow() });
    matches!(class.as_str(), "CabinetWClass" | "ExploreWClass" | "Progman" | "WorkerW")
}

/// Ön plan değişince çağrılır: yeniden adlandırma kısayolunu yalnızca
/// Gezgin/masaüstü öndeyken tutar.
pub fn sync_rename(hwnd: HWND, cfg: &Config) {
    let want = cfg.tools.rename_enabled && explorer_foreground();
    if want == RENAME_ON.load(Ordering::SeqCst) {
        return;
    }
    RENAME_ON.store(want, Ordering::SeqCst);
    if want {
        reg(hwnd, RENAME, cfg.shortcuts.rename_combo(), "Yeniden adlandırma", &cfg.shortcuts.rename);
    } else {
        unsafe {
            let _ = UnregisterHotKey(hwnd, RENAME);
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

/// `carlui-tools.exe`'yi verilen kipte başlatır; araç öne gelebilsin diye
/// ön plan hakkı devredilir (kısayolu alan süreç biziz).
pub fn tool(id: i32) {
    let args = match id {
        PICKER => "--pick".to_string(),
        DISK => "--disk".to_string(),
        RENAME => format!("--rename --hwnd {}", unsafe { GetForegroundWindow() }.0 as isize),
        DICTATE => return convert("--dictate"),
        SPEAK => return convert("--speak"),
        ASSIST => return convert("--assist"),
        _ => return,
    };
    let exe = paths::sibling_exe("carlui-tools.exe");
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        let r = ShellExecuteW(HWND::default(), w!("open"), &HSTRING::from(exe.as_os_str()), &HSTRING::from(args.as_str()), PCWSTR::null(), SW_SHOWNORMAL);
        if r.0 as isize <= 32 {
            warn!("{} açılamadı ({args})", exe.display());
        }
    }
}

/// `carlui-convert.exe`'yi kısayol kipinde başlatır: `--dictate` sesle yazar ya da
/// (çalışıyorsa) bitirir, `--speak` seçimi okur ya da susturur. Öndeki pencere iletilir.
fn convert(mode: &str) {
    let exe = paths::sibling_exe("carlui-convert.exe");
    let fg = unsafe { GetForegroundWindow() }.0 as isize;
    match std::process::Command::new(&exe).args([mode, "--hwnd", &fg.to_string()]).spawn() {
        Ok(child) => {
            // Asistan açtığı uygulamayı öne getirebilsin (kısayolu alan süreç biziz).
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(child.id());
            }
            info!("kısayol: {mode}")
        }
        Err(e) => warn!("{} açılamadı: {e}", exe.display()),
    }
}
