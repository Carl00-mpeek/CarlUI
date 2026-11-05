//! Dock öğesine tıklanınca: uygulamayı başlat, öne getir/küçült ya da pencereler
//! arasında dön; Başlat düğmesi Win menüsünü, Ara düğmesi adanın aramasını açar.

use super::*;
use carlui_core::config::WinKey;

impl Dock {
    // ---- eylemler -----------------------------------------------------------

    /// Uygulamanın yeni bir örneğini başlatır.
    pub fn launch(&self, i: usize) {
        let e = &self.entries[i];
        let (path, args) = match &e.pin {
            // Kısayol silinmişse (Windows'un önbellek kısayolları temizlenebilir) hedef exe.
            Some(p) if p.path.to_lowercase().ends_with(".lnk") && !p.exe.is_empty() && !std::path::Path::new(&expand_env(&p.path)).exists() => (expand_env(&p.exe), p.args.clone()),
            Some(p) => (expand_env(&p.path), p.args.clone()),
            None if !e.app.aumid.is_empty() && e.exe.to_lowercase().contains("\\windowsapps\\") => {
                (format!("shell:AppsFolder\\{}", e.app.aumid), String::new())
            }
            None => (e.exe.clone(), String::new()),
        };
        if path.is_empty() {
            return;
        }
        unsafe {
            let _ = AllowSetForegroundWindow(ASFW_ANY);
            ShellExecuteW(HWND::default(), w!("open"), &HSTRING::from(path), &HSTRING::from(args), PCWSTR::null(), SW_SHOWNORMAL);
        }
    }

    /// Tıklama davranışı: kapalıysa aç; tek pencere öndeyse küçült, değilse öne getir;
    /// birden çok pencere varsa sırayla aralarında dön.
    pub fn activate(&self, i: usize, foreground: HWND) {
        let e = &self.entries[i];
        match e.kind {
            // Başlat: Win menüsü (Win tuşu Windows'a bırakılmadıysa). Ara: adanın araması.
            // Hiçbiri çalışmıyorsa Windows'unkiler.
            Kind::Start if self.cfg.shortcuts.win_key != WinKey::Windows && carlui_core::win::toggle_start() => {}
            Kind::Start | Kind::Search if self.cfg.launcher.enabled && carlui_core::win::toggle_search() => {}
            Kind::Start => send_keys(&[VK_LWIN]),
            Kind::Search => send_keys(&[VK_LWIN, VK_S]),
            // Yığın menüsünü dock penceresi açar (bkz. places::stack_menu).
            Kind::Tray | Kind::Folder => {}
            Kind::Trash => places::open(places::RECYCLE_BIN),
            Kind::App => match e.windows.len() {
                0 => self.launch(i),
                1 => {
                    let w = e.windows[0];
                    if w == foreground && !tracker::is_minimized(w) {
                        tracker::minimize(w);
                    } else {
                        tracker::activate(w);
                    }
                }
                n => {
                    let next = match e.windows.iter().position(|&w| w == foreground) {
                        Some(p) => e.windows[(p + 1) % n],
                        None => e.windows[0],
                    };
                    tracker::activate(next);
                }
            },
        }
    }
}


fn send_keys(keys: &[VIRTUAL_KEY]) {
    let key = |vk: VIRTUAL_KEY, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, ..Default::default() },
        },
    };
    let mut inputs: Vec<INPUT> = keys.iter().map(|&k| key(k, false)).collect();
    inputs.extend(keys.iter().rev().map(|&k| key(k, true)));
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}
