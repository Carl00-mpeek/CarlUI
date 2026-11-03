//! Bu modül "aynı uygulamanın birden çok penceresi var (iki ekranda iki
//! Firefox), hangisine gideceğim?" sorununu şu yöntemle çözüyor: aramada açık
//! pencereler başlıklarıyla ayrı satır olarak listelenir; başlık aranan metne
//! uyuyorsa ("youtube") ya da uygulamanın birden çok penceresi varsa çıkar.
//! ">" ile başlayan arama yalnızca açık pencereleri listeler (Alt+Tab gibi).

use crate::index::Running;
use crate::search::{norm, score, Act, Icon, Item};
use windows::Win32::{Foundation::HWND, UI::WindowsAndMessaging::GetWindowTextW};

const MAX: usize = 6;

fn title(hwnd: isize) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetWindowTextW(HWND(hwnd as _), &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn app_name(exe: &str) -> String {
    let stem = exe.rsplit('\\').next().unwrap_or(exe).trim_end_matches(".exe");
    let mut c = stem.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

fn screen(hwnd: isize) -> String {
    let m = carlui_core::win::monitor_name(HWND(hwnd as _));
    m.trim_start_matches("\\\\.\\DISPLAY").parse::<u32>().map(|n| t!(format!(" · Ekran {n}"), format!(" · Display {n}"))).unwrap_or_default()
}

fn item(r: &Running, title: String) -> Item {
    let sub = format!("{} · {}{}", t!("Açık pencere", "Open window"), app_name(&r.exe), screen(r.hwnd));
    let mut it = Item::new(title, sub, Icon::Shell(r.exe.clone()), Act::App { launch: String::new(), hwnd: Some(r.hwnd) }, "Pencere");
    it.run = Some((r.hwnd, r.pid));
    it
}

/// Aramaya uyan açık pencereler (puanıyla). `all`: ">" ile tüm pencereler.
pub(crate) fn matches(run: &[Running], q: &str, all: bool) -> Vec<(i32, Item)> {
    let mut out = Vec::new();
    for r in run {
        let t = title(r.hwnd);
        if t.is_empty() {
            continue;
        }
        let nt = norm(&t);
        let app = norm(&app_name(&r.exe));
        let siblings = run.iter().filter(|o| o.exe == r.exe).count();
        let s = if all && q.is_empty() {
            500
        } else {
            let by_title = score(&nt, q);
            // Aynı uygulamanın birden çok penceresi: uygulama adı yazılınca hepsi ayrı satır.
            let by_app = if siblings > 1 || all { score(&app, q) - 40 } else { 0 };
            by_title.max(by_app)
        };
        if s >= if all { 250 } else { 520 } {
            out.push((s, item(r, t)));
        }
    }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out.truncate(MAX);
    out
}
