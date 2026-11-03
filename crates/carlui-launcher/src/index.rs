//! Bu modül "aranabilecek her şeyin listesi nereden gelir?" sorununu çözüyor.
//!
//! - **Uygulamalar:** Windows'un "Tüm uygulamalar" listesi (`shell:AppsFolder`).
//!   Başlat menüsü kısayolları ve Store uygulamaları tek yerden, Windows'un
//!   gösterdiği adlarla gelir. Liste arka planda kurulur ve diske yazılır;
//!   bir sonraki açılışta arama ilk milisaniyeden çalışır.
//! - **Ayarlar ve konumlar:** Sabit, Türkçe anahtar kelimeli bir liste.
//! - **Açık pencereler:** Başlatıcı her açıldığında bir kez okunur; uygulama
//!   açıksa Enter onu öne getirir.

use carlui_core::{info, paths, warn};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Storage::{
            EnhancedStorage::{PKEY_AppUserModel_ID, PKEY_Link_TargetParsingPath},
            FileSystem::{FindFirstChangeNotificationW, FindNextChangeNotification, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME},
        },
        System::{
            Com::*,
            Threading::{OpenProcess, QueryFullProcessImageNameW, WaitForMultipleObjects, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION},
        },
        UI::{
            Shell::{PropertiesSystem::*, *},
            WindowsAndMessaging::*,
        },
    },
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct App {
    pub name: String,
    /// `shell:AppsFolder\` ardından gelen ad; uygulamayı bununla açarız.
    pub parsing: String,
    /// Kısayolun gösterdiği dosya (masaüstü uygulamaları); Store uygulamalarında boş.
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub aumid: String,
}

impl App {
    pub fn launch_path(&self) -> String {
        format!("shell:AppsFolder\\{}", self.parsing)
    }
}

fn pwstr_take(p: PWSTR) -> String {
    unsafe {
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as _));
        s
    }
}

/// Kaldırıcılar, kurulum sihirbazları ve belgeler listede kalabalık yapmasın.
fn is_noise(name: &str, target: &str) -> bool {
    let n = name.to_lowercase();
    let t = target.to_lowercase();
    let file = t.rsplit('\\').next().unwrap_or("");
    n.starts_with("uninstall") || n.contains("kaldır") || file.starts_with("unins") || file.contains("uninstall") || t.starts_with("http")
        || [".txt", ".url", ".html", ".htm", ".pdf", ".chm", ".rtf"].iter().any(|e| t.ends_with(e))
}

/// `shell:AppsFolder` içeriğini okur. Bir COM (STA) iş parçacığında çağrılmalı.
pub fn enumerate_apps() -> Vec<App> {
    let mut out = Vec::new();
    unsafe {
        let folder: IShellItem = match SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None) {
            Ok(f) => f,
            Err(e) => {
                warn!("uygulama klasörü açılamadı: {e}");
                return out;
            }
        };
        let Ok(items) = folder.BindToHandler::<_, IEnumShellItems>(None, &BHID_EnumItems) else { return out };
        loop {
            let mut batch: [Option<IShellItem>; 32] = Default::default();
            let mut got = 0u32;
            if items.Next(&mut batch, Some(&mut got)).is_err() || got == 0 {
                break;
            }
            for item in batch.iter().take(got as usize).flatten() {
                let name = item.GetDisplayName(SIGDN_NORMALDISPLAY).map(pwstr_take).unwrap_or_default();
                let parsing = item.GetDisplayName(SIGDN_PARENTRELATIVEPARSING).map(pwstr_take).unwrap_or_default();
                if name.is_empty() || parsing.is_empty() {
                    continue;
                }
                let (mut target, mut aumid) = (String::new(), String::new());
                if let Ok(i2) = item.cast::<IShellItem2>() {
                    target = i2.GetString(&PKEY_Link_TargetParsingPath).map(pwstr_take).unwrap_or_default();
                    aumid = i2.GetString(&PKEY_AppUserModel_ID).map(pwstr_take).unwrap_or_default();
                }
                if is_noise(&name, &target) {
                    continue;
                }
                out.push(App { name, parsing, target, aumid });
            }
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out.dedup_by(|a, b| a.name == b.name && a.parsing == b.parsing);
    out
}

fn cache_file() -> PathBuf {
    paths::data_dir().join("launcher-apps.json")
}

pub struct Index {
    pub apps: Arc<Mutex<Arc<Vec<App>>>>,
    dirty: Arc<Mutex<(bool, Instant)>>,
    building: Arc<Mutex<bool>>,
}

impl Index {
    /// Önbellekten hemen yükler, arka planda tazeler. `on_ready` liste her
    /// yenilendiğinde çağrılır.
    pub fn start(on_ready: impl Fn() + Send + Sync + 'static) -> Self {
        let cached: Vec<App> = std::fs::read(cache_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let idx = Self {
            apps: Arc::new(Mutex::new(Arc::new(cached))),
            dirty: Arc::new(Mutex::new((true, Instant::now()))),
            building: Arc::default(),
        };
        let on_ready: Arc<dyn Fn() + Send + Sync> = Arc::new(on_ready);
        idx.refresh(on_ready.clone());
        idx.watch(on_ready);
        idx
    }

    /// Liste eskiyse (Başlat menüsü değiştiyse ya da 30 dk geçtiyse) arka planda yeniden kurar.
    pub fn refresh_if_stale(&self, on_ready: Arc<dyn Fn() + Send + Sync>) {
        let stale = self.dirty.lock().map(|d| d.0 || d.1.elapsed() > Duration::from_secs(1800)).unwrap_or(false);
        if stale {
            self.refresh(on_ready);
        }
    }

    fn refresh(&self, on_ready: Arc<dyn Fn() + Send + Sync>) {
        {
            let mut b = self.building.lock().unwrap_or_else(|e| e.into_inner());
            if *b {
                return;
            }
            *b = true;
        }
        if let Ok(mut d) = self.dirty.lock() {
            *d = (false, Instant::now());
        }
        let apps = self.apps.clone();
        let building = self.building.clone();
        std::thread::Builder::new()
            .name("app-index".into())
            .spawn(move || {
                unsafe {
                    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                }
                let t = Instant::now();
                let list = enumerate_apps();
                let changed = apps.lock().map(|a| **a != list).unwrap_or(true);
                info!("uygulama listesi: {} uygulama, {} ms{}", list.len(), t.elapsed().as_millis(), if changed { "" } else { " (değişmedi)" });
                if changed && !list.is_empty() {
                    if let Ok(json) = serde_json::to_vec(&list) {
                        let _ = std::fs::write(cache_file(), json);
                    }
                    if let Ok(mut a) = apps.lock() {
                        *a = Arc::new(list);
                    }
                    on_ready();
                }
                *building.lock().unwrap_or_else(|e| e.into_inner()) = false;
            })
            .ok();
    }

    /// Başlat menüsü klasörleri değişince (uygulama kuruldu/kaldırıldı) listeyi eski say.
    fn watch(&self, on_ready: Arc<dyn Fn() + Send + Sync>) {
        let dirty = self.dirty.clone();
        let apps = self.apps.clone();
        let building = self.building.clone();
        std::thread::Builder::new()
            .name("app-watch".into())
            .spawn(move || unsafe {
                let dirs = [
                    std::env::var("APPDATA").map(|d| format!("{d}\\Microsoft\\Windows\\Start Menu\\Programs")).unwrap_or_default(),
                    std::env::var("ProgramData").map(|d| format!("{d}\\Microsoft\\Windows\\Start Menu\\Programs")).unwrap_or_default(),
                ];
                let handles: Vec<HANDLE> = dirs
                    .iter()
                    .filter_map(|d| {
                        let w: Vec<u16> = d.encode_utf16().chain([0]).collect();
                        FindFirstChangeNotificationW(PCWSTR(w.as_ptr()), true, FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME).ok()
                    })
                    .collect();
                if handles.is_empty() {
                    return;
                }
                loop {
                    let r = WaitForMultipleObjects(&handles, false, u32::MAX);
                    let i = r.0 as usize;
                    if i >= handles.len() {
                        break;
                    }
                    // Kurulumlar birçok dosyayı art arda yazar; durulmasını bekle.
                    std::thread::sleep(Duration::from_secs(3));
                    for h in &handles {
                        let _ = FindNextChangeNotification(*h);
                    }
                    if let Ok(mut d) = dirty.lock() {
                        d.0 = true;
                    }
                    let tmp = Index { apps: apps.clone(), dirty: dirty.clone(), building: building.clone() };
                    tmp.refresh(on_ready.clone());
                }
            })
            .ok();
    }

    pub fn snapshot(&self) -> Arc<Vec<App>> {
        self.apps.lock().map(|a| a.clone()).unwrap_or_default()
    }
}

// ---- Ayarlar ve konumlar ----

pub struct Setting {
    pub name: &'static str,
    pub uri: &'static str,
    pub keywords: &'static str,
    pub glyph: char,
}

pub const SETTINGS: &[Setting] = &[
    Setting { name: "Ayarlar", uri: "ms-settings:", keywords: "settings sistem ayar", glyph: '\u{E713}' },
    Setting { name: "Ekran", uri: "ms-settings:display", keywords: "display çözünürlük ölçek parlaklık hdr monitör", glyph: '\u{E7F4}' },
    Setting { name: "Ses", uri: "ms-settings:sound", keywords: "sound hoparlör mikrofon çıkış giriş", glyph: '\u{E767}' },
    Setting { name: "Ses karıştırıcısı", uri: "ms-settings:apps-volume", keywords: "volume mixer uygulama sesi", glyph: '\u{E767}' },
    Setting { name: "Bildirimler", uri: "ms-settings:notifications", keywords: "notifications rahatsız etmeyin odak", glyph: '\u{EA8F}' },
    Setting { name: "Güç ve pil", uri: "ms-settings:powersleep", keywords: "power battery uyku enerji pil", glyph: '\u{E83F}' },
    Setting { name: "Depolama", uri: "ms-settings:storagesense", keywords: "storage disk alan temizlik", glyph: '\u{EDA2}' },
    Setting { name: "Bluetooth ve cihazlar", uri: "ms-settings:bluetooth", keywords: "bluetooth cihaz kulaklık eşleştir", glyph: '\u{E702}' },
    Setting { name: "Fare", uri: "ms-settings:mousetouchpad", keywords: "mouse imleç touchpad dokunmatik", glyph: '\u{E962}' },
    Setting { name: "Klavye", uri: "ms-settings:keyboard", keywords: "keyboard yazma dil", glyph: '\u{E765}' },
    Setting { name: "Yazıcılar ve tarayıcılar", uri: "ms-settings:printers", keywords: "printer yazıcı", glyph: '\u{E749}' },
    Setting { name: "Wi-Fi", uri: "ms-settings:network-wifi", keywords: "wifi kablosuz ağ internet", glyph: '\u{E701}' },
    Setting { name: "Ağ ve internet", uri: "ms-settings:network", keywords: "network ethernet vpn proxy", glyph: '\u{E774}' },
    Setting { name: "VPN", uri: "ms-settings:network-vpn", keywords: "vpn", glyph: '\u{E705}' },
    Setting { name: "Kişiselleştirme", uri: "ms-settings:personalization", keywords: "personalization tema renk", glyph: '\u{E771}' },
    Setting { name: "Arka plan", uri: "ms-settings:personalization-background", keywords: "wallpaper duvar kağıdı arka plan", glyph: '\u{E91B}' },
    Setting { name: "Renkler", uri: "ms-settings:colors", keywords: "colors vurgu koyu açık mod", glyph: '\u{E790}' },
    Setting { name: "Yüklü uygulamalar", uri: "ms-settings:appsfeatures", keywords: "apps kaldır program uygulamalar", glyph: '\u{E71D}' },
    Setting { name: "Varsayılan uygulamalar", uri: "ms-settings:defaultapps", keywords: "default tarayıcı varsayılan", glyph: '\u{E71D}' },
    Setting { name: "Başlangıç uygulamaları", uri: "ms-settings:startupapps", keywords: "startup açılış başlangıç", glyph: '\u{E7B5}' },
    Setting { name: "Hesaplar", uri: "ms-settings:yourinfo", keywords: "account hesap profil", glyph: '\u{E77B}' },
    Setting { name: "Oturum açma seçenekleri", uri: "ms-settings:signinoptions", keywords: "pin parola şifre hello", glyph: '\u{E72E}' },
    Setting { name: "Tarih ve saat", uri: "ms-settings:dateandtime", keywords: "date time saat dilimi", glyph: '\u{E787}' },
    Setting { name: "Dil ve bölge", uri: "ms-settings:regionlanguage", keywords: "language bölge dil", glyph: '\u{F2B7}' },
    Setting { name: "Oyun modu", uri: "ms-settings:gaming-gamemode", keywords: "game oyun", glyph: '\u{E7FC}' },
    Setting { name: "Erişilebilirlik", uri: "ms-settings:easeofaccess", keywords: "accessibility erişim", glyph: '\u{E776}' },
    Setting { name: "Gizlilik ve güvenlik", uri: "ms-settings:privacy", keywords: "privacy izin güvenlik", glyph: '\u{E72E}' },
    Setting { name: "Windows Güvenliği", uri: "windowsdefender:", keywords: "defender virüs antivirüs güvenlik duvarı", glyph: '\u{E83D}' },
    Setting { name: "Windows Update", uri: "ms-settings:windowsupdate", keywords: "update güncelleme", glyph: '\u{E895}' },
    Setting { name: "Hakkında", uri: "ms-settings:about", keywords: "about sistem bilgisi bilgisayar adı", glyph: '\u{E946}' },
    Setting { name: "Kurtarma", uri: "ms-settings:recovery", keywords: "recovery sıfırla", glyph: '\u{E777}' },
    Setting { name: "Gece ışığı", uri: "ms-settings:nightlight", keywords: "night light mavi ışık", glyph: '\u{E708}' },
    Setting { name: "Denetim Masası", uri: "control", keywords: "control panel", glyph: '\u{E713}' },
    Setting { name: "Görev Yöneticisi", uri: "taskmgr", keywords: "task manager işlem süreç", glyph: '\u{E9D9}' },
];

/// Sistem klasörleri: (ad, yol, anahtar kelimeler).
pub fn places() -> Vec<(String, String, &'static str)> {
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    let known = |id: &GUID| unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).map(pwstr_take).unwrap_or_default() };
    let mut v = vec![
        ("Masaüstü".to_string(), known(&FOLDERID_Desktop), "desktop"),
        ("Belgeler".to_string(), known(&FOLDERID_Documents), "documents dökümanlar"),
        ("İndirilenler".to_string(), known(&FOLDERID_Downloads), "downloads indirme"),
        ("Resimler".to_string(), known(&FOLDERID_Pictures), "pictures fotoğraflar"),
        ("Müzik".to_string(), known(&FOLDERID_Music), "music"),
        ("Videolar".to_string(), known(&FOLDERID_Videos), "videos"),
        ("Kullanıcı klasörü".to_string(), home, "home kişisel"),
        ("Dosya Gezgini".to_string(), "explorer:".into(), "explorer file manager klasör"),
        ("Bu bilgisayar".to_string(), "shell:MyComputerFolder".into(), "this pc bilgisayarım sürücüler"),
        ("Geri Dönüşüm Kutusu".to_string(), "shell:RecycleBinFolder".into(), "recycle bin çöp"),
    ];
    unsafe {
        let mask = windows::Win32::Storage::FileSystem::GetLogicalDrives();
        for i in 0..26u32 {
            if mask & (1 << i) != 0 {
                let letter = (b'A' + i as u8) as char;
                v.push((format!("Yerel Disk ({letter}:)"), format!("{letter}:\\"), "drive sürücü disk"));
            }
        }
    }
    v.retain(|p| !p.1.is_empty());
    v
}

// ---- Açık pencereler ----

#[derive(Clone, Debug)]
pub struct Running {
    pub hwnd: isize,
    pub pid: u32,
    pub exe: String,
    pub aumid: String,
}

fn process_exe(pid: u32) -> String {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        if ok.is_ok() { String::from_utf16_lossy(&buf[..len as usize]) } else { String::new() }
    }
}

fn process_aumid(pid: u32) -> String {
    use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let rc = GetApplicationUserModelId(h, &mut len, PWSTR(buf.as_mut_ptr()));
        let _ = CloseHandle(h);
        if rc.is_ok() && len > 1 { String::from_utf16_lossy(&buf[..len as usize - 1]) } else { String::new() }
    }
}

fn window_aumid(hwnd: HWND) -> String {
    unsafe {
        let Ok(store) = SHGetPropertyStoreForWindow::<_, IPropertyStore>(hwnd) else { return String::new() };
        store.GetValue(&PKEY_AppUserModel_ID).map(|v| v.to_string()).unwrap_or_default()
    }
}

fn is_app_window(hwnd: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || !GetWindow(hwnd, GW_OWNER).unwrap_or_default().is_invalid() {
            return false;
        }
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TOOLWINDOW.0 != 0 {
            return false;
        }
        let mut cloaked = 0u32;
        let _ = windows::Win32::Graphics::Dwm::DwmGetWindowAttribute(hwnd, windows::Win32::Graphics::Dwm::DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, 4);
        cloaked == 0 && GetWindowTextLengthW(hwnd) > 0
    }
}

/// Alt+Tab'da görünen pencereler, z-sırasıyla (öndeki önce).
pub fn running() -> Vec<Running> {
    unsafe extern "system" fn cb(hwnd: HWND, lp: LPARAM) -> BOOL {
        let out = &mut *(lp.0 as *mut Vec<Running>);
        if is_app_window(hwnd) {
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid != std::process::id() {
                let mut aumid = window_aumid(hwnd);
                if aumid.is_empty() {
                    aumid = process_aumid(pid);
                }
                if aumid.is_empty() {
                    // UWP uygulamaları ApplicationFrameHost içinde çalışır; asıl süreç çocuk penceredir.
                    if let Ok(core) = FindWindowExW(hwnd, HWND::default(), w!("Windows.UI.Core.CoreWindow"), PCWSTR::null()) {
                        let mut cpid = 0;
                        GetWindowThreadProcessId(core, Some(&mut cpid));
                        aumid = process_aumid(cpid);
                    }
                }
                out.push(Running { hwnd: hwnd.0 as isize, pid, exe: process_exe(pid).to_lowercase(), aumid: aumid.to_lowercase() });
            }
        }
        TRUE
    }
    let mut out = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut out as *mut _ as isize));
    }
    out
}

/// Uygulamanın açık penceresi (en öndeki).
pub fn window_of(app: &App, run: &[Running]) -> Option<Running> {
    let aumid = if app.aumid.is_empty() && !app.parsing.contains('\\') { app.parsing.to_lowercase() } else { app.aumid.to_lowercase() };
    let target = app.target.to_lowercase();
    run.iter()
        .find(|r| (!aumid.is_empty() && r.aumid == aumid) || (!target.is_empty() && target.ends_with(".exe") && r.exe == target))
        .cloned()
}

/// Eski CarlSpotlight'ın kullanım sayılarını bir kez içeri aktarmak için.
pub fn legacy_usage() -> HashMap<String, u32> {
    let path = std::env::var("LOCALAPPDATA").map(|d| PathBuf::from(d).join("CarlSpotlight").join("usage.json")).unwrap_or_default();
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}
