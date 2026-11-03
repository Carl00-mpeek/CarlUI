//! Genel: Windows ile başlama, görev çubuğu, adanın monitörü.

use crate::{
    ui::{edit, Flow, P},
    App,
};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CMONITORS};

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let c = &app.cfg.general;
    f.section(p, "Başlangıç");
    f.toggle(p, "Windows ile başlat", "Oturum açılınca CarlUI hemen başlar.", c.start_with_windows, |c| c.general.start_with_windows ^= true);
    f.section(p, "Görev çubuğu");
    f.toggle(p, "Windows görev çubuğunu gizle", "Dock çalışırken Windows'un kendi görev çubuğu görünmez.", c.hide_taskbar, |c| c.general.hide_taskbar ^= true);

    let n = unsafe { GetSystemMetrics(SM_CMONITORS) }.clamp(1, 4) as u32;
    if n > 1 {
        f.section(p, "Monitör");
        let names: Vec<String> = (0..n).map(|i| if i == 0 { "Ana".to_string() } else { format!("{}.", i + 1) }).collect();
        let items: Vec<(&str, crate::Act)> = names.iter().enumerate().map(|(i, s)| (s.as_str(), edit(move |c| c.general.top_monitor = i as u32))).collect();
        f.choice(p, "Adanın monitörü", "Üstteki ada ve bildirimler bu monitörde görünür.", &items, c.top_monitor.min(n - 1) as usize, 70.0);
    }
}
