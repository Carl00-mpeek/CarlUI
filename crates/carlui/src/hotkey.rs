//! Ana sürecin üstlendiği genel kısayollar: Carl Vault (`shortcuts.vault`),
//! renk seçici (`shortcuts.color_picker`, varsayılan boş = kapalı), disk
//! analizörü (`shortcuts.disk`, varsayılan boş) ve toplu yeniden adlandırma
//! (`shortcuts.rename`), dosya kilidi açıcı (`shortcuts.unlock`), pencere
//! düzenleri (`shortcuts.layouts`), sesle yazma (`shortcuts.dictate`) ve sesli okuma (`shortcuts.speak`). Yeniden adlandırma kısayolu yalnızca Gezgin ya da
//! masaüstü öndeyken kaydedilir; böylece tarayıcıdaki Ctrl+Shift+R gibi
//! kısayollar elinden alınmaz. Doğrulama (sol tık, tek tuş, Windows kısayolu
//! reddi) ortak `keys` modülünde, kayıt (fare tuşları dahil) `hotkey` modülündedir; ayar değişince kısayollar yeniden kaydedilir.

use carlui_core::{config::Config, hotkey, info, keys::Combo, paths, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use windows::{
    core::*,
    Win32::{
        Foundation::HWND,
        UI::{Shell::ShellExecuteW, WindowsAndMessaging::*},
    },
};

pub const VAULT: i32 = 1;
pub const PICKER: i32 = 2;
pub const DISK: i32 = 3;
pub const RENAME: i32 = 4;
pub const DICTATE: i32 = 5;
pub const SPEAK: i32 = 6;
pub const ASSIST: i32 = 7;
pub const UNLOCK: i32 = 8;
pub const LAYOUTS: i32 = 9;

static RENAME_ON: AtomicBool = AtomicBool::new(false);
static UNLOCK_ON: AtomicBool = AtomicBool::new(false);

fn reg(hwnd: HWND, id: i32, c: Option<Combo>, name: &str, text: &str) {
    let Some(c) = c else { return };
    if let Err(e) = hotkey::register(hwnd, id, c) {
        warn!("{name} kısayolu {text} kaydedilemedi (başka bir uygulama kullanıyor olabilir): {e}");
    }
}

pub fn register(hwnd: HWND, cfg: &Config) {
    for id in [VAULT, PICKER, DISK, RENAME, DICTATE, SPEAK, ASSIST, UNLOCK, LAYOUTS] {
        hotkey::unregister(hwnd, id);
    }
    RENAME_ON.store(false, Ordering::SeqCst);
    UNLOCK_ON.store(false, Ordering::SeqCst);
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
    reg(hwnd, LAYOUTS, s.layouts_combo(), "Pencere düzenleri", &s.layouts);
    sync_rename(hwnd, cfg);
}

fn explorer_foreground() -> bool {
    let class = carlui_core::win::class_name(unsafe { GetForegroundWindow() });
    matches!(class.as_str(), "CabinetWClass" | "ExploreWClass" | "Progman" | "WorkerW")
}

/// Ön plan değişince çağrılır: yeniden adlandırma ve dosya kilidi açıcı
/// kısayollarını yalnızca Gezgin/masaüstü öndeyken tutar.
pub fn sync_rename(hwnd: HWND, cfg: &Config) {
    let front = explorer_foreground();
    let s = &cfg.shortcuts;
    sync_one(hwnd, RENAME, &RENAME_ON, cfg.tools.rename_enabled && front, s.rename_combo(), "Yeniden adlandırma", &s.rename);
    sync_one(hwnd, UNLOCK, &UNLOCK_ON, cfg.tools.unlock_enabled && front, s.unlock_combo(), "Dosya kilidi açıcı", &s.unlock);
}

fn sync_one(hwnd: HWND, id: i32, on: &AtomicBool, want: bool, c: Option<Combo>, name: &str, text: &str) {
    if want == on.load(Ordering::SeqCst) {
        return;
    }
    on.store(want, Ordering::SeqCst);
    if want {
        reg(hwnd, id, c, name, text);
    } else {
        hotkey::unregister(hwnd, id);
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
        UNLOCK => format!("--unlock --hwnd {}", unsafe { GetForegroundWindow() }.0 as isize),
        LAYOUTS => "--layouts".to_string(),
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
