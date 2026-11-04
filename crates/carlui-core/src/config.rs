//! Bu modül "tüm ayarlar tek yerde ve yeniden başlatmadan uygulansın" sorununu
//! çözüyor: tüm modüllerin ayarları tek bir `%APPDATA%\CarlUI\config.toml`
//! dosyasındadır. Dosya değişince bir izleyici iş parçacığı (Windows dosya
//! değişikliği bildirimiyle, yoklamasız) yeni ayarı okuyup geri çağırır.

use serde::{Deserialize, Serialize};
use std::time::Duration;
use windows::Win32::{
    Foundation::{HANDLE, WAIT_OBJECT_0},
    Storage::FileSystem::{
        FindCloseChangeNotification, FindFirstChangeNotificationW, FindNextChangeNotification,
        FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE,
    },
    System::Threading::{WaitForSingleObject, INFINITE},
};

pub use crate::theme::Palette;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub theme: ThemeConfig,
    pub dock: DockConfig,
    pub clock: ClockConfig,
    pub island: IslandConfig,
    pub notify: NotifyConfig,
    pub launcher: LauncherConfig,
    pub shortcuts: ShortcutsConfig,
    pub clipboard: ClipboardConfig,
    pub game: GameConfig,
    pub stats: StatsConfig,
    pub cursor: CursorConfig,
    pub download: DownloadConfig,
    pub alerts: AlertsConfig,
    pub vault: VaultConfig,
}

/// Carl Vault (yerel şifreli kasa, `carlui-vault.exe`; kısayol: `shortcuts.vault`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct VaultConfig {
    /// Bu kadar dakika boyunca klavye/fare kullanılmazsa kasa kilitlenir (en az 1).
    pub auto_lock_minutes: u32,
    /// Bilgisayar uykuya geçerken ya da oturum kilitlenirken kilitle.
    pub lock_on_sleep: bool,
    /// Kopyalanan alan bu kadar saniye sonra panodan silinir (hâlâ oradaysa). 0 = silinmez.
    pub clear_clipboard_seconds: u64,
    /// Kasa penceresi ekran görüntülerinde ve ekran paylaşımında görünmesin.
    pub hide_from_capture: bool,
    /// Kopyalanan alan panoya değer yerine tek kullanımlık belirteç olarak konur;
    /// değer yalnızca CarlUI panosundan (Win+V) seçilince hedef pencereye yazılır.
    /// Kapalıysa değer doğrudan panoya konur (eski davranış).
    pub token_paste: bool,
    /// Tarayıcı eklentisi giriş sayfalarını kasadan doldurabilsin (kullanıcı
    /// eklentide kaydı seçince; yalnızca adresi uyan kayıtlar).
    pub browser_fill: bool,
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self { auto_lock_minutes: 5, lock_on_sleep: true, clear_clipboard_seconds: 30, hide_from_capture: true, token_paste: true, browser_fill: true }
    }
}

/// Sıcaklık ve disk uyarıları: sınır aşılınca adada bildirim.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct AlertsConfig {
    /// CPU sıcaklığı (°C); yalnızca CarlUI Sensors çalışıyorsa okunur.
    pub cpu: bool,
    pub cpu_temp: u32,
    /// GPU sıcaklığı (°C; NVIDIA).
    pub gpu: bool,
    pub gpu_temp: u32,
    /// SSD/disk sıcaklığı (°C).
    pub disk: bool,
    pub disk_temp: u32,
    /// Bir sürücüde (C:, D: …) boş alan bu yüzdenin altına inince.
    pub space: bool,
    pub space_pct: u32,
}

impl Default for AlertsConfig {
    fn default() -> Self {
        Self { cpu: true, cpu_temp: 90, gpu: true, gpu_temp: 85, disk: true, disk_temp: 70, space: true, space_pct: 10 }
    }
}

/// Adanın indirme hapında hangi bilgiler görünsün.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct DownloadConfig {
    pub name: bool,
    pub percent: bool,
    /// İnen / toplam ("1,2 GB / 3,5 GB").
    pub size: bool,
    pub speed: bool,
    /// Kalan süre ("2 dk kaldı").
    pub eta: bool,
    /// Başka programların (Steam, Epic, torrent…) indirmeleri: ağ alımı ve
    /// disk yazımından tahmin edilir.
    pub others: bool,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self { name: true, percent: true, size: true, speed: true, eta: true, others: true }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CursorStyle {
    /// Renkli degrade, beyaz kenar, isteğe bağlı ışıma.
    Neon,
    /// Koyu (ya da seçilen renkte) gövde, kalın kontrast kenar.
    Ink,
    /// Yumuşak: kenarsız, yuvarlak köşeli düz gövde ve yumuşak gölge.
    Soft,
    /// Zen: gümüş metalik gövde, mürekkep dalgası ve sakura; ışıma rengi seçilir.
    Zen,
}

/// CarlUI imleci (ok + bağlantı eli). Çalışırken Windows'un imleçlerinin yerine
/// geçer; kapanınca kullanıcının kendi şeması geri gelir (bkz. `cursor`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct CursorConfig {
    pub enabled: bool,
    pub style: CursorStyle,
    /// Neon rengi (#rrggbb).
    pub color: String,
    /// Mürekkep gövde rengi (#rrggbb).
    pub ink_color: String,
    /// Yumuşak gövde rengi (#rrggbb).
    pub soft_color: String,
    /// Zen ışıma rengi (#rrggbb).
    pub zen_color: String,
    /// Neon / Zen ışıması.
    pub glow: bool,
    /// Dış kenar rengi: "auto" (gövdeye göre beyaz/koyu) ya da #rrggbb.
    pub edge: String,
    /// Boyut çarpanı; Windows'un imleç boyutuna (erişilebilirlik × ölçek) uygulanır.
    pub scale: f32,
}

impl Default for CursorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            style: CursorStyle::Neon,
            color: "#ff4fd8".into(),
            ink_color: "#1c1c22".into(),
            soft_color: "#f7f7f8".into(),
            zen_color: "#bcd6ff".into(),
            glow: true,
            edge: "auto".into(),
            scale: 0.75,
        }
    }
}

impl CursorConfig {
    /// Seçili stilin renk ayarı.
    pub fn style_color(&self) -> &str {
        match self.style {
            CursorStyle::Neon => &self.color,
            CursorStyle::Ink => &self.ink_color,
            CursorStyle::Soft => &self.soft_color,
            CursorStyle::Zen => &self.zen_color,
        }
    }
}

/// Adanın üstüne gelince açılan küçük sistem göstergesi: hangileri görünsün.
/// Hepsi kapalıysa ada üzerine gelince genişlemez.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct StatsConfig {
    pub cpu_temp: bool,
    pub gpu_temp: bool,
    pub ram: bool,
    /// Fare adanın üstünde bu kadar ms kalınca genişler.
    pub hover_ms: u32,
}

impl Default for StatsConfig {
    fn default() -> Self {
        Self { cpu_temp: true, gpu_temp: true, ram: true, hover_ms: 350 }
    }
}

/// Pano geçmişi (adada açılır, kısayol: `shortcuts.clipboard`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct ClipboardConfig {
    pub enabled: bool,
    /// En fazla bu kadar kayıt tutulur (sabitlenenler hariç silinir).
    pub max_items: usize,
    /// Enter: seçileni önceki pencereye yapıştır. Kapalıysa yalnızca panoya kopyalar.
    pub paste_on_enter: bool,
    /// Şifre/anahtar gibi görünen kopyalar bu kadar saniye sonra geçmişten
    /// silinir (sabitlenenler kalır). 0 = silinmez, yalnızca maskelenir.
    pub secret_seconds: u64,
}

impl Default for ClipboardConfig {
    fn default() -> Self {
        Self { enabled: true, max_items: 200, paste_on_enter: true, secret_seconds: 60 }
    }
}

/// Tüm genel kısayollar tek yerde. Boş metin = kapalı. Geçersiz değer
/// (fare tuşu, değiştiricisiz tek tuş, Windows'un temel kısayolu …) gelirse
/// varsayılan kullanılır ve loga yazılır; bkz. `keys` modülü.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct ShortcutsConfig {
    /// Aramayı aç/kapat, ör. "Ctrl+Space", "Alt+Space", "Ctrl+Shift+K".
    pub search: String,
    /// Windows tuşuna tek başına basınca ne açılsın: CarlUI menüsü, arama ya da Windows'un Başlat menüsü.
    pub win_key: WinKey,
    /// Dock'taki 1.–9. uygulamayı açan değiştirici: "Win" → Win+1…9, "Ctrl+Alt" → Ctrl+Alt+1…9.
    pub app_numbers: String,
    /// Pano geçmişini aç/kapat, ör. "Win+V" (Windows'un kendi pano geçmişinin yerine), "Ctrl+Shift+V".
    pub clipboard: String,
    /// Bölgesel ekran görüntüsü, ör. "Alt+X", "PrintScreen", "Win+Shift+S".
    pub screenshot: String,
    /// Dönüştürücü: yalnızca değiştiriciler, birlikte basılıp bırakılınca açılır (ör. "Win+Alt").
    pub converter: String,
    /// Carl Vault penceresini aç, ör. "Ctrl+Alt+P".
    pub vault: String,
}

impl Default for ShortcutsConfig {
    fn default() -> Self {
        Self {
            search: "Ctrl+Space".into(),
            win_key: WinKey::Menu,
            app_numbers: "Win".into(),
            clipboard: "Win+V".into(),
            screenshot: "Alt+X".into(),
            converter: "Win+Alt".into(),
            vault: "Ctrl+Alt+P".into(),
        }
    }
}

impl ShortcutsConfig {
    /// Ekran görüntüsü kısayolu (kapalıysa None).
    pub fn screenshot_combo(&self) -> Option<crate::keys::Combo> {
        if self.screenshot.trim().is_empty() {
            return None;
        }
        crate::keys::parse_combo(&self.screenshot).or_else(|e| {
            crate::warn!("ekran görüntüsü kısayolu '{}' kullanılamaz: {e}; varsayılan (Alt+X) kullanılıyor", self.screenshot);
            crate::keys::parse_combo(&Self::default().screenshot)
        }).ok()
    }

    /// Dönüştürücü değiştiricileri (kapalıysa None). Shift'li ya da tek Win olamaz.
    pub fn converter_mods(&self) -> Option<crate::keys::Mods> {
        if self.converter.trim().is_empty() {
            return None;
        }
        crate::keys::parse_mods(&self.converter)
            .and_then(|m| {
                let n = [m.ctrl, m.alt, m.shift, m.win].iter().filter(|b| **b).count();
                if n < 2 { Err(t!("en az iki değiştirici gerekli (tek başına Win/Alt başka işlere ait)", "at least two modifiers needed (Win/Alt alone are taken)").to_string()) } else { Ok(m) }
            })
            .or_else(|e| {
                crate::warn!("dönüştürücü kısayolu '{}' kullanılamaz: {e}; varsayılan (Win+Alt) kullanılıyor", self.converter);
                crate::keys::parse_mods("Win+Alt")
            })
            .ok()
    }

    /// Pano kısayolu (kapalıysa None).
    pub fn clipboard_combo(&self) -> Option<crate::keys::Combo> {
        if self.clipboard.trim().is_empty() {
            return None;
        }
        crate::keys::parse_combo(&self.clipboard).or_else(|e| {
            crate::warn!("pano kısayolu '{}' kullanılamaz: {e}; varsayılan (Win+V) kullanılıyor", self.clipboard);
            crate::keys::parse_combo(&Self::default().clipboard)
        }).ok()
    }

    /// Arama kısayolu (kapalıysa None).
    pub fn search_combo(&self) -> Option<crate::keys::Combo> {
        if self.search.trim().is_empty() {
            return None;
        }
        crate::keys::parse_combo(&self.search).or_else(|e| {
            crate::warn!("arama kısayolu '{}' kullanılamaz: {e}; varsayılan (Ctrl+Space) kullanılıyor", self.search);
            crate::keys::parse_combo(&Self::default().search)
        }).ok()
    }

    /// Carl Vault kısayolu (kapalıysa None).
    pub fn vault_combo(&self) -> Option<crate::keys::Combo> {
        if self.vault.trim().is_empty() {
            return None;
        }
        crate::keys::parse_combo(&self.vault).or_else(|e| {
            crate::warn!("Vault kısayolu '{}' kullanılamaz: {e}; varsayılan (Ctrl+Alt+P) kullanılıyor", self.vault);
            crate::keys::parse_combo(&Self::default().vault)
        }).ok()
    }

    /// Uygulama numaraları değiştiricisi (kapalıysa None).
    pub fn app_number_mods(&self) -> Option<crate::keys::Mods> {
        if self.app_numbers.trim().is_empty() {
            return None;
        }
        crate::keys::parse_mods(&self.app_numbers).or_else(|e| {
            crate::warn!("uygulama numarası kısayolu '{}' kullanılamaz: {e}; varsayılan (Win) kullanılıyor", self.app_numbers);
            crate::keys::parse_mods("Win")
        }).ok()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct LauncherConfig {
    pub enabled: bool,
    /// Kullanıcı klasörlerinde dosya adı araması.
    pub file_search: bool,
    /// Döviz çevirisi (internetten günlük kur alır).
    pub currency: bool,
    /// Bağlantı yapıştırınca "İndir" önerisi: ada ilerlemeyi gösterir.
    pub download_manager: bool,
    /// Web araması adresi; `{q}` aranan metinle değişir.
    pub web_search: String,
    /// `wp ad mesaj` kişilerle eşleşsin mi? Kapalıysa ya da kişi yoksa `wp` yalnızca WhatsApp'ı açar.
    pub whatsapp_contacts: bool,
    /// `@wp` kişileri: ad → telefon (ülke koduyla, ör. "905551234567"); rehberden (.vcf) içe aktarılır.
    pub whatsapp: std::collections::BTreeMap<String, String>,
    /// Ayarlardaki "+" ile eklenen arama motorları: ad → adres (`{q}` ile).
    pub engines: std::collections::BTreeMap<String, String>,
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            file_search: true,
            currency: true,
            download_manager: true,
            web_search: "https://www.google.com/search?q={q}".into(),
            whatsapp_contacts: false,
            whatsapp: Default::default(),
            engines: Default::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct GeneralConfig {
    /// Dock çalışırken yerel Windows görev çubuğunu gizle.
    pub hide_taskbar: bool,
    /// Üst bildirim alanının gösterileceği monitör (0 = ana monitör).
    pub top_monitor: u32,
    /// Windows oturumu açılınca CarlUI'yi hemen başlat.
    pub start_with_windows: bool,
    /// Arayüz dili: "auto" (Windows'un diline göre), "tr" ya da "en".
    pub language: String,
    /// Günde bir kez GitHub'da yeni sürüm var mı diye bak, varsa bildir.
    pub check_updates: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self { hide_taskbar: true, top_monitor: 0, start_with_windows: true, language: "auto".into(), check_updates: true }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct ThemeConfig {
    pub palette: Palette,
    /// 0 = çok saydam, 1 = koyu ve yoğun cam.
    pub glass_density: f32,
    /// Köşe yuvarlaklığı çarpanı (1.0 = varsayılan).
    pub corner_radius: f32,
    /// Animasyon hızı çarpanı (1.0 = varsayılan, 2.0 = iki kat hızlı).
    pub animation_speed: f32,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self { palette: Palette::BluePurple, glass_density: 0.5, corner_radius: 1.0, animation_speed: 1.0 }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum WinKey {
    /// CarlUI'nin kompakt menüsü (`carlui-start`).
    #[default]
    Menu,
    /// Üstteki adanın araması.
    Search,
    /// Windows'un kendi Başlat menüsü.
    Windows,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum DockBehavior {
    #[default]
    AlwaysVisible,
    /// Bir pencere dock'un üstüne gelince gizlen.
    Intellihide,
    AutoHide,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct DockConfig {
    /// Simge boyutu (DIP, 100% ölçekte piksel).
    pub icon_size: f32,
    /// Ekranın alt kenarından uzaklık (DIP).
    pub bottom_margin: f32,
    pub behavior: DockBehavior,
    pub show_running_apps: bool,
    /// Başlat düğmesinin yanındaki arama (büyüteç) düğmesi.
    pub show_search: bool,
    /// Sağ uçta İndirilenler yığını (tıklayınca son indirilenler listesi).
    pub downloads_stack: bool,
    /// Sağ uçta Geri Dönüşüm Kutusu (sağ tıkla boşaltılabilir).
    pub recycle_bin: bool,
    pub show_labels: bool,
    pub show_previews: bool,
    /// Sağ tık menüsündeki kategori başlıklarının rengi: "system" (Windows
    /// vurgu rengi), "off" (renksiz) ya da "#RRGGBB".
    pub menu_accent: String,
    /// Sağ tık menüsünde uygulamanın "Sık ziyaret edilenler / Son oynanan" gibi
    /// listeleri de gösterilsin mi (Görevler her zaman görünür).
    pub menu_lists: bool,
    /// İlk açılışta görev çubuğundaki sabitlenmişler içeri aktarıldı mı?
    pub imported_taskbar_pins: bool,
    pub pinned: Vec<Pinned>,
}

impl DockConfig {
    /// Menü başlık rengi (0xRRGGBB); `None` = renksiz. Tanınmayan değer
    /// sistem rengine düşer.
    pub fn menu_accent_color(&self, system: impl FnOnce() -> u32) -> Option<u32> {
        let v = self.menu_accent.trim().to_lowercase();
        let hex = v.trim_start_matches('#');
        match v.as_str() {
            "off" | "kapalı" | "kapali" | "none" => None,
            _ if hex.len() == 6 => u32::from_str_radix(hex, 16).ok().or_else(|| Some(system())),
            _ => Some(system()),
        }
    }
}

impl Default for DockConfig {
    fn default() -> Self {
        Self {
            icon_size: 30.0,
            bottom_margin: 12.0,
            behavior: DockBehavior::AlwaysVisible,
            show_running_apps: true,
            show_search: true,
            downloads_stack: true,
            recycle_bin: true,
            show_labels: true,
            show_previews: true,
            menu_accent: "system".into(),
            menu_lists: false,
            imported_taskbar_pins: false,
            pinned: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Pinned {
    pub name: String,
    /// Exe yolu, .lnk ya da `shell:AppsFolder\<AUMID>`.
    pub path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub args: String,
    /// İkon farklı bir dosyadan alınacaksa (ör. başlatıcı exe'nin ikonu yoksa).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub icon: String,
    /// Açık pencerelerle eşleştirme için uygulama kimliği (AppUserModelID).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub aumid: String,
    /// Açık pencerelerle eşleştirme için hedef exe yolu (kısayolun gösterdiği).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub exe: String,
}

/// Üstteki adada saatin ne zaman görüneceği.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ClockMode {
    #[default]
    Always,
    /// Yalnızca bir uygulama öndeyken (masaüstündeyken ada gizlenir).
    WhenAppActive,
    /// Ada yalnızca bildirim ya da çalan medya varken görünür.
    Never,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct ClockConfig {
    pub mode: ClockMode,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct IslandConfig {
    /// Ekranın üst kenarından uzaklık (DIP).
    pub top_margin: f32,
    /// Öndeki pencere maksimize iken adayı yukarı sakla (fare üst kenara gelince çıkar).
    /// Kapalıyken ada her zaman sabit durur.
    pub hide_over_maximized: bool,
    /// Şarkı/video çalarken ada, kapağın rengini alsın.
    pub media_tint: bool,
}

impl Default for IslandConfig {
    fn default() -> Self {
        Self { top_margin: 6.0, hide_over_maximized: false, media_tint: true }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct NotifyConfig {
    pub enabled: bool,
    pub media_bar: bool,
    pub volume_osd: bool,
    /// Bildirimin ekranda kalma süresi (ms).
    pub duration_ms: u32,
    /// Maksimize pencere varken de göster.
    pub over_maximized: bool,
    /// Bildirimler adada gösterilirken Windows'un sağ alttaki balonlarını kapat
    /// (Bildirim Merkezi'ne düşmeye devam ederler).
    pub hide_windows_banners: bool,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self { enabled: true, media_bar: true, volume_osd: true, duration_ms: 2500, over_maximized: false, hide_windows_banners: true }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct GameConfig {
    /// Tam ekran algılanınca tüm arayüzü gizle.
    pub hide_on_fullscreen: bool,
    /// Oyundayken gelen bildirimleri sakla, oyundan çıkınca göster
    /// (kapalıysa oyunda gelenler hiç gösterilmez).
    pub hold_notes: bool,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self { hide_on_fullscreen: true, hold_notes: true }
    }
}

/// `%NAME%` biçimindeki ortam değişkenlerini açar.
pub fn expand_env(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                match std::env::var(name) {
                    Ok(v) => out.push_str(&v),
                    Err(_) => out.push_str(&rest[start..start + end + 2]),
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Ayarları okur. Dosya yoksa varsayılanları yazar; bozuksa varsayılanlarla
/// devam eder ama dosyanın üzerine yazmaz (kullanıcının düzenlemesi kaybolmasın).
pub fn load() -> Config {
    let cfg = read();
    crate::lang::apply(&cfg.general.language);
    cfg
}

fn read() -> Config {
    let path = crate::paths::config_file();
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
            crate::warn!("config.toml okunamadı, varsayılanlar kullanılıyor: {e}");
            Config::default()
        }),
        Err(_) => {
            let cfg = Config::default();
            save(&cfg);
            cfg
        }
    }
}

pub fn save(cfg: &Config) {
    let path = crate::paths::config_file();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match toml::to_string_pretty(cfg) {
        // Önce geçici dosyaya yaz, sonra taşı: izleyiciler yarım dosya görmesin.
        Ok(text) => {
            let tmp = path.with_extension("toml.tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
        Err(e) => crate::error!("config.toml yazılamadı: {e}"),
    }
}

/// Ayar dosyası değiştiğinde `on_change`'i yeni ayarla çağıran bir iş parçacığı başlatır.
/// Yalnızca içerik gerçekten değiştiyse çağırır.
pub fn watch(mut on_change: impl FnMut(Config) + Send + 'static) {
    std::thread::Builder::new()
        .name("config-watch".into())
        .spawn(move || {
            let path = crate::paths::config_file();
            let Some(dir) = path.parent().map(|d| d.to_path_buf()) else { return };
            let _ = std::fs::create_dir_all(&dir);
            let mut last = load();
            let wide: Vec<u16> = dir.as_os_str().to_string_lossy().encode_utf16().chain([0]).collect();
            unsafe {
                let Ok(h): Result<HANDLE, _> = FindFirstChangeNotificationW(
                    windows::core::PCWSTR(wide.as_ptr()),
                    false,
                    FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_FILE_NAME,
                ) else {
                    crate::error!("ayar izleyicisi başlatılamadı");
                    return;
                };
                while WaitForSingleObject(h, INFINITE) == WAIT_OBJECT_0 {
                    // Editörler dosyayı birkaç adımda yazar; kısa bir bekleme ile birleştir.
                    std::thread::sleep(Duration::from_millis(150));
                    let cfg = load();
                    if cfg != last {
                        crate::info!("ayarlar yeniden yüklendi");
                        last = cfg.clone();
                        on_change(cfg);
                    }
                    if FindNextChangeNotification(h).is_err() {
                        break;
                    }
                }
                let _ = FindCloseChangeNotification(h);
            }
        })
        .ok();
}
