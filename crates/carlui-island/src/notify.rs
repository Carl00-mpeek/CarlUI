//! Bu modül "uygulamaların bildirimlerini nasıl yakalarız?" sorununu
//! Windows'un bildirim dinleyicisiyle (UserNotificationListener) çözüyor.
//! Windows ilk kullanımda erişim izni ister (Ayarlar → Gizlilik → Bildirimler);
//! izin varsa Bildirim Merkezi'ne düşen her bildirim burada da görünür.
//! Kanca, enjeksiyon ya da ekran okuma yoktur.
//!
//! Paketlenmemiş uygulamalarda "bildirim değişti" olayı çalışmıyor. Onun
//! yerine Windows'un her yeni bildirimde güncellediği kayıt defteri anahtarı
//! (Notifications\Settings\<uygulama>\LastNotificationAddedTime) izlenir;
//! yalnızca o zaman liste okunur ve yeni kimlikli bildirimler adaya gönderilir.
//! Windows'un kendi balonları kapalı olsa bile çalışır; boştayken iş parçacığı
//! uyumaz değil, hiç uyanmaz (60 sn'lik güvenlik kontrolü dışında).

use carlui_core::{info, warn};
use std::{
    collections::{HashSet, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};
use windows::{
    core::{Interface, PCWSTR},
    Foundation::{Size, TypedEventHandler},
    UI::Notifications::{
        KnownNotificationBindings,
        Management::{UserNotificationListener, UserNotificationListenerAccessStatus},
        NotificationKinds, UserNotification,
    },
    Win32::{
        Foundation::*,
        System::{
            Registry::{RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_NOTIFY, REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME},
            Threading::{CreateEventW, WaitForSingleObject},
            WinRT::*,
        },
        UI::WindowsAndMessaging::PostMessageW,
    },
};

/// Windows'un kendi balonları kapalı tutulsun mu (`notify.hide_windows_banners`).
static HIDE_BANNERS: AtomicBool = AtomicBool::new(false);

/// Ayar değişince (ve açılış/kapanışta) çağrılır; balonları hemen kapatır ya da geri açar.
pub fn set_hide_banners(hide: bool) {
    HIDE_BANNERS.store(hide, Ordering::SeqCst);
    crate::banners::sync(hide);
}

#[derive(Clone, Debug, Default)]
pub struct Note {
    pub app: String,
    /// Tıklanınca açılacak uygulamanın kimliği (bilinmiyorsa boş).
    pub aumid: String,
    pub title: String,
    pub body: String,
    pub logo: Option<Arc<Vec<u8>>>,
    /// Adada saklanan bildirimin kimliği ve geliş saati (`hubbing`).
    pub id: u64,
    pub at: String,
    /// Ekran görüntüsü bildirimi: dosya yolu ve panodaki kaydın kimliği.
    pub shot: Option<String>,
    pub clip_id: Option<String>,
    /// İndirme bildirimi: tıklanınca açılacak dosya.
    pub file: Option<String>,
    /// Yeni sürüm bildirimi: tıklanınca indirilip sessizce kurulur.
    pub update: Option<crate::update::Release>,
}

fn build(n: &UserNotification) -> Option<Note> {
    let info = n.AppInfo().ok();
    let display = info.as_ref().and_then(|i| i.DisplayInfo().ok());
    let app = display.as_ref().and_then(|d| d.DisplayName().ok()).map(|s| s.to_string()).unwrap_or_default();
    let aumid = info.as_ref().and_then(|i| i.AppUserModelId().ok()).map(|s| s.to_string()).unwrap_or_default();
    let binding = n.Notification().ok()?.Visual().ok()?.GetBinding(&KnownNotificationBindings::ToastGeneric().ok()?).ok()?;
    let texts: Vec<String> = binding
        .GetTextElements()
        .ok()?
        .into_iter()
        .filter_map(|t| t.Text().ok().map(|s| s.to_string().replace(['\r', '\n'], " ").trim().to_string()))
        .filter(|s| !s.is_empty())
        .collect();
    let title = texts.first().cloned().unwrap_or_else(|| app.clone());
    let body = texts.iter().skip(1).cloned().collect::<Vec<_>>().join(" · ");
    let logo = display
        .as_ref()
        .and_then(|d| d.GetLogo(Size { Width: 64.0, Height: 64.0 }).ok())
        .and_then(|r| crate::media::read_stream(&r.cast().ok()?))
        .map(Arc::new);
    Some(Note { app, aumid, title, body, logo, ..Default::default() })
}

/// Dinleyiciyi başlatır; yeni bildirimler `queue`ya eklenir ve `notify`
/// penceresine `msg` gönderilir.
pub fn start(notify: isize, msg: u32, queue: Arc<Mutex<VecDeque<Note>>>) {
    std::thread::Builder::new()
        .name("notify".into())
        .spawn(move || {
            let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
            let listener = match UserNotificationListener::Current() {
                Ok(l) => l,
                Err(e) => {
                    warn!("bildirim dinleyicisi yok: {e}");
                    return;
                }
            };
            let access = listener.RequestAccessAsync().and_then(|op| op.get());
            if access.as_ref().ok() != Some(&UserNotificationListenerAccessStatus::Allowed) {
                warn!("bildirimlere erişim izni yok ({access:?}); Ayarlar → Gizlilik ve güvenlik → Bildirimler");
                return;
            }
            // Zaman aşımında `None`: liste okunamadıysa görülenler unutulmasın.
            let list = |l: &UserNotificationListener| -> Option<Vec<UserNotification>> {
                crate::media::await_op(l.GetNotificationsAsync(NotificationKinds::Toast), 5000, "bildirim listesi").map(|v| v.into_iter().collect())
            };
            // Açılışta zaten var olanlar gösterilmez.
            let mut seen: HashSet<u32> = list(&listener).unwrap_or_default().iter().filter_map(|n| n.Id().ok()).collect();

            let (tx, rx) = mpsc::channel::<()>();
            let evented = listener
                .NotificationChanged(&TypedEventHandler::new(move |_, _| {
                    let _ = tx.send(());
                    Ok(())
                }))
                .is_ok();
            info!("bildirim dinleyicisi başladı ({})", if evented { "olayla" } else { "saniyede bir kontrol" });

            // Olay yoksa Windows'un her yeni bildirimde yazdığı kayıt anahtarını izle.
            // Böylece boşta hiç uyanmayız (yoklama yok).
            let watch = if evented {
                None
            } else {
                unsafe {
                    let key: Vec<u16> = r"Software\Microsoft\Windows\CurrentVersion\Notifications\Settings".encode_utf16().chain([0]).collect();
                    let mut h = HKEY::default();
                    match (RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), 0, KEY_NOTIFY, &mut h).ok(), CreateEventW(None, false, false, None)) {
                        (Ok(()), Ok(ev)) => Some((h, ev)),
                        _ => None,
                    }
                }
            };
            let arm = |(h, ev): (HKEY, HANDLE)| unsafe {
                RegNotifyChangeKeyValue(h, true, REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET, ev, true).is_ok()
            };
            let watch = watch.filter(|&w| arm(w));
            if !evented {
                info!("bildirimler {}", if watch.is_some() { "kayıt defteri izlenerek alınacak" } else { "2 sn'de bir kontrol edilecek" });
            }

            loop {
                if evented {
                    if rx.recv().is_err() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(80));
                    while rx.try_recv().is_ok() {}
                } else if let Some(w) = watch {
                    // 60 sn güvenlik zaman aşımı: izleme bir şekilde kaçırırsa yine de yakala.
                    // Kayıt, bildirim listeye eklenmeden hemen önce yazılır; kısa bekleriz.
                    unsafe {
                        let _ = WaitForSingleObject(w.1, 60_000);
                    }
                    std::thread::sleep(Duration::from_millis(120));
                    arm(w);
                } else {
                    std::thread::sleep(Duration::from_millis(2000));
                }
                // Yeni bir uygulamanın anahtarı ilk bildirimiyle oluşur: balonunu da hemen kapat.
                if HIDE_BANNERS.load(Ordering::SeqCst) {
                    crate::banners::sync(true);
                }
                let Some(now) = list(&listener) else { continue };
                let mut fresh = Vec::new();
                let mut ids = HashSet::new();
                for n in &now {
                    let Ok(id) = n.Id() else { continue };
                    ids.insert(id);
                    if !seen.contains(&id) {
                        match build(n) {
                            Some(note) => fresh.push(note),
                            None => warn!("bildirim {id} okunamadı (metin yok)"),
                        }
                    }
                }
                seen = ids;
                if !fresh.is_empty() {
                    if let Ok(mut q) = queue.lock() {
                        q.extend(fresh);
                    }
                    unsafe {
                        let _ = PostMessageW(HWND(notify as _), msg, WPARAM(0), LPARAM(0));
                    }
                }
            }
        })
        .ok();
}
