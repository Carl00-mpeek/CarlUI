//! Bu modül "ekran çıkarılıp takılınca ya da uykudan sonra pencereler tek
//! ekrana yığılıyor" sorununun ortak parçası: açık pencerelerin yerini okur,
//! geri koyar ve "iş", "oyun" gibi adlı düzenleri `layouts.json`'da saklar.
//! Otomatik hatırlama ana süreçte (`carlui/src/layouts.rs`), adlı düzen penceresi
//! `carlui-tools --layouts`'tadır; ikisi de bu kodu kullanır.
//!
//! Yalnızca açık pencereler yerleştirilir: kapalı uygulama başlatılmaz.

use crate::win;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::PathBuf};
use windows::Win32::{
    Foundation::*,
    Graphics::Dwm::*,
    UI::WindowsAndMessaging::*,
};

/// Bir pencerenin yeri: normal dikdörtgeni, durumu ve simge/tam ekran noktaları.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Spot {
    /// Normal konumdaki dikdörtgen: sol, üst, sağ, alt.
    pub rect: [i32; 4],
    /// SW_SHOWNORMAL, SW_SHOWMINIMIZED ya da SW_SHOWMAXIMIZED.
    pub show: u32,
    pub flags: u32,
    pub min: [i32; 2],
    pub max: [i32; 2],
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Yalnızca bu oturum için geçerli; kayıtlı düzende eşleştirme exe + sınıfla olur.
    #[serde(skip)]
    pub hwnd: isize,
    pub exe: String,
    pub class: String,
    pub title: String,
    pub spot: Spot,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub name: String,
    pub windows: Vec<Entry>,
}

#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq)]
pub struct Store {
    pub layouts: Vec<Layout>,
}

fn path() -> PathBuf {
    crate::paths::data_dir().join("layouts.json")
}

impl Store {
    pub fn load() -> Self {
        std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let p = path();
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(p, serde_json::to_string_pretty(self)?)
    }

    /// Aynı adlı düzen varsa yerine yazar (büyük/küçük harf farkı sayılmaz).
    pub fn put(&mut self, l: Layout) {
        match self.layouts.iter_mut().find(|o| o.name.to_lowercase() == l.name.to_lowercase()) {
            Some(o) => *o = l,
            None => self.layouts.push(l),
        }
    }
}

/// Düzen adını temizler: kenar boşlukları gider, en çok 40 karakter.
pub fn clean_name(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(40).collect()
}

fn read_spot(h: HWND) -> Option<Spot> {
    let mut w = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
    unsafe { GetWindowPlacement(h, &mut w) }.ok()?;
    let r = w.rcNormalPosition;
    Some(Spot { rect: [r.left, r.top, r.right, r.bottom], show: w.showCmd, flags: w.flags.0, min: [w.ptMinPosition.x, w.ptMinPosition.y], max: [w.ptMaxPosition.x, w.ptMaxPosition.y] })
}

fn title(h: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Kullanıcının gördüğü uygulama penceresi mi? Sanal masaüstündeki ya da
/// gizlenmiş (cloaked) pencereler, araç pencereleri ve kabuk atlanır.
fn wanted(h: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(h).as_bool() || GetWindow(h, GW_OWNER).is_ok_and(|o| !o.is_invalid()) {
            return false;
        }
        if GetWindowLongW(h, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
            return false;
        }
        let mut cloaked = 0u32;
        if DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4).is_ok() && cloaked != 0 {
            return false;
        }
    }
    !matches!(win::class_name(h).as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd")
}

/// Açık pencereleri üstten alta sırayla okur; CarlUI'nin kendi pencereleri hariç.
pub fn capture() -> Vec<Entry> {
    unsafe extern "system" fn each(h: HWND, l: LPARAM) -> BOOL {
        let out = &mut *(l.0 as *mut Vec<HWND>);
        out.push(h);
        TRUE
    }
    let mut all: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut all as *mut Vec<HWND> as isize));
    }
    all.into_iter()
        .filter(|h| wanted(*h))
        .filter_map(|h| {
            let t = title(h);
            let exe = win::window_exe(h);
            let file = exe.rsplit('\\').next().unwrap_or("").to_lowercase();
            if t.is_empty() || exe.is_empty() || file.starts_with("carlui") {
                return None;
            }
            Some(Entry { hwnd: h.0 as isize, exe, class: win::class_name(h), title: t, spot: read_spot(h)? })
        })
        .collect()
}

/// Kayıtlı düzeni açık pencerelerle eşleştirir: önce exe + sınıf + tam başlık,
/// kalanlar sırayla exe + sınıf. Dönen çiftler (açık pencere, kayıtlı yer).
pub fn pair(saved: &[Entry], open: &[Entry]) -> Vec<(isize, Spot)> {
    let same = |a: &Entry, b: &Entry| a.exe.eq_ignore_ascii_case(&b.exe) && a.class == b.class;
    let mut used: HashSet<isize> = HashSet::new();
    let mut done = vec![false; saved.len()];
    let mut out = Vec::new();
    for (i, s) in saved.iter().enumerate() {
        if let Some(o) = open.iter().find(|o| !used.contains(&o.hwnd) && same(s, o) && s.title == o.title) {
            used.insert(o.hwnd);
            done[i] = true;
            out.push((o.hwnd, s.spot));
        }
    }
    for (i, s) in saved.iter().enumerate() {
        if done[i] {
            continue;
        }
        if let Some(o) = open.iter().find(|o| !used.contains(&o.hwnd) && same(s, o)) {
            used.insert(o.hwnd);
            out.push((o.hwnd, s.spot));
        }
    }
    out
}

/// Pencereyi yerine koyar; odak değişmez. `true`: pencere yerinde.
pub fn place(hwnd: isize, s: &Spot) -> bool {
    let h = HWND(hwnd as _);
    if !unsafe { IsWindow(h) }.as_bool() || unsafe { IsHungAppWindow(h) }.as_bool() {
        return false;
    }
    if read_spot(h).is_some_and(|now| now.rect == s.rect && now.show == s.show) {
        return true;
    }
    let show = match SHOW_WINDOW_CMD(s.show as i32) {
        SW_SHOWMINIMIZED => SW_SHOWMINNOACTIVE,
        SW_SHOWMAXIMIZED => SW_SHOWMAXIMIZED,
        _ => SW_SHOWNOACTIVATE,
    };
    let w = WINDOWPLACEMENT {
        length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
        flags: WINDOWPLACEMENT_FLAGS(s.flags),
        showCmd: show.0 as u32,
        ptMinPosition: POINT { x: s.min[0], y: s.min[1] },
        ptMaxPosition: POINT { x: s.max[0], y: s.max[1] },
        rcNormalPosition: RECT { left: s.rect[0], top: s.rect[1], right: s.rect[2], bottom: s.rect[3] },
    };
    let front = unsafe { GetForegroundWindow() };
    // Pencere başka DPI'lı ekrana geçince boyutu yeniden hesaplanır; ikinci
    // deneme son boyutu oturtur.
    for _ in 0..2 {
        if unsafe { SetWindowPlacement(h, &w) }.is_err() {
            return false;
        }
        if read_spot(h).is_some_and(|now| now.rect == s.rect) {
            break;
        }
    }
    if unsafe { GetForegroundWindow() } != front {
        win::force_foreground(front);
    }
    true
}

/// Düzeni uygular; yerine konan pencere sayısını döndürür.
pub fn apply(l: &Layout) -> usize {
    pair(&l.windows, &capture()).iter().filter(|(h, s)| place(*h, s)).count()
}

/// Ekran dizilimi imzası: hatırlanan yerler yalnızca aynı dizilime geri konur.
pub fn signature() -> String {
    let mut parts: Vec<String> = win::monitors().iter().map(|m| format!("{},{},{},{}", m.rect.left, m.rect.top, m.rect.right, m.rect.bottom)).collect();
    parts.sort();
    parts.join("|")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(hwnd: isize, exe: &str, title: &str, x: i32) -> Entry {
        Entry { hwnd, exe: exe.into(), class: "C".into(), title: title.into(), spot: Spot { rect: [x, 0, x + 100, 100], show: 1, ..Default::default() } }
    }

    #[test]
    fn exact_title_wins_then_order() {
        let saved = [e(0, "a.exe", "ikinci", 1), e(0, "a.exe", "birinci", 2), e(0, "b.exe", "x", 3)];
        let open = [e(10, "A.EXE", "birinci", 0), e(11, "a.exe", "başka", 0), e(12, "c.exe", "x", 0)];
        let got = pair(&saved, &open);
        assert_eq!(got.len(), 2);
        assert!(got.contains(&(10, saved[1].spot)));
        assert!(got.contains(&(11, saved[0].spot)));
    }

    #[test]
    fn one_window_is_used_once() {
        let saved = [e(0, "a.exe", "t", 1), e(0, "a.exe", "t", 2)];
        assert_eq!(pair(&saved, &[e(10, "a.exe", "t", 0)]).len(), 1);
    }

    #[test]
    fn names_are_cleaned_and_replaced() {
        assert_eq!(clean_name("  iş   düzeni "), "iş düzeni");
        assert_eq!(clean_name(&"x".repeat(60)).chars().count(), 40);
        let mut s = Store::default();
        s.put(Layout { name: "İş".into(), windows: vec![] });
        s.put(Layout { name: "oyun".into(), windows: vec![] });
        s.put(Layout { name: "OYUN".into(), windows: vec![e(1, "a.exe", "t", 1)] });
        assert_eq!(s.layouts.len(), 2);
        assert_eq!(s.layouts[1].windows.len(), 1);
    }

    #[test]
    fn store_round_trips_without_handles() {
        let s = Store { layouts: vec![Layout { name: "iş".into(), windows: vec![e(77, "a.exe", "t", 5)] }] };
        let back: Store = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.layouts[0].windows[0].hwnd, 0);
        assert_eq!(back.layouts[0].windows[0].spot, s.layouts[0].windows[0].spot);
    }
}
