//! Tanı aracı: başlığında verilen metin geçen pencerelerin dock'un gördüğü
//! kimliğini (AUMID, exe, stem) ve Alt+Tab'a uygunluk bilgisini yazdırır.
#[path = "../src/apps.rs"]
#[allow(dead_code)]
mod apps;
#[path = "../src/resolver.rs"]
#[allow(dead_code)]
mod resolver;

use windows::Win32::{
    Foundation::*,
    System::Com::*,
    UI::WindowsAndMessaging::*,
};

static mut NEEDLE: String = String::new();

unsafe extern "system" fn each(h: HWND, _: LPARAM) -> BOOL {
    let mut buf = [0u16; 512];
    let n = GetWindowTextW(h, &mut buf);
    let t = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
    #[allow(static_mut_refs)]
    if t.to_lowercase().contains(&NEEDLE) {
        let id = apps::AppId::of_window(&apps::window_app_id(h), &apps::process_exe_of_window(h));
        let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
        println!("{:?} '{t}' visible={} ex={ex:#x} explicit='{}' -> {id:?}", h.0, IsWindowVisible(h).as_bool(), apps::window_aumid(h));
        if IsWindowVisible(h).as_bool() {
            println!("   resolver={:?} pin={:?}", resolver::app_id(h), apps::pin_for_window(h));
        }
    }
    TRUE
}

fn main() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        NEEDLE = std::env::args().nth(1).unwrap_or_default().to_lowercase();
        let _ = EnumWindows(Some(each), LPARAM(0));
        for p in apps::import_taskbar_pins() {
            println!("pin {} -> {:?}", p.name, apps::AppId::of_pin(&p));
        }
    }
}
