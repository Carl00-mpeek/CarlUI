//! Bu modül "ayrı süreçlerdeki modüller birbirinden nasıl haberdar olur?"
//! sorununu iki yönlü named pipe ile çözüyor. Ana süreç (`carlui.exe`) sunucu
//! olur; her modül bir "yukarı" (modül → ana) ve bir "aşağı" (ana → modül)
//! bağlantısı açar. Ana sürece gelen her olay tüm modüllere yayınlanır.
//! Mesajlar satır başına bir JSON'dur. İki ayrı boru kullanılmasının sebebi,
//! Windows'ta aynı senkron handle üzerinde okuma beklerken yazmanın
//! kilitlenmesidir.
//!
//! Güvenlik: borular yalnızca bu kullanıcıya açık kurulur; iki uç da karşısındaki
//! sürecin aynı klasörden, aynı oturum ve kullanıcıyla çalıştığını doğrular
//! (`auth`). Satırlar `line::MAX_LINE` ile sınırlıdır; aşan bağlantı düşer.

mod auth;
mod line;

use line::MAX_LINE;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{BufReader, Write},
    os::windows::{
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle},
    },
    sync::{
        mpsc::{self, SyncSender, TrySendError},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Storage::FileSystem::*,
        System::{Pipes::*, RemoteDesktop::ProcessIdToSessionId, Threading::GetCurrentProcessId},
    },
};

/// Boru adları oturuma özel: aynı bilgisayarda başka bir kullanıcı oturumundaki
/// CarlUI ile karışmasın.
fn names() -> &'static (String, String) {
    NAMES.get_or_init(|| {
        let mut session = 0;
        unsafe {
            let _ = ProcessIdToSessionId(GetCurrentProcessId(), &mut session);
        }
        (format!(r"\\.\pipe\carlui-bus-up-{session}"), format!(r"\\.\pipe\carlui-bus-down-{session}"))
    })
}

static NAMES: OnceLock<(String, String)> = OnceLock::new();

/// Yalnızca testler: bu süreçte olay yolu canlı borular yerine
/// `carlui-bus-test-{tag}-up/down` adlarını kullansın (canlı CarlUI'ye dokunmadan
/// süreçler arası deneme). Borular ilk kullanılmadan önce çağrılmalı; adlar
/// zaten belirlendiyse `false`. Ürün kodu çağırmaz; doğrulama aynen geçerli.
#[doc(hidden)]
pub fn use_test_pipes(tag: &str) -> bool {
    NAMES.set((format!(r"\\.\pipe\carlui-bus-test-{tag}-up"), format!(r"\\.\pipe\carlui-bus-test-{tag}-down"))).is_ok()
}

/// Yalnızca testler: (yukarı, aşağı) boru adları; ham istemci denemeleri için.
#[doc(hidden)]
pub fn pipe_names() -> (&'static str, &'static str) {
    (up_name(), down_name())
}

fn up_name() -> &'static str {
    &names().0
}

fn down_name() -> &'static str {
    &names().1
}

/// Bir modüle gönderilmeyi bekleyen en çok olay sayısı. Dolarsa modül donmuş
/// sayılır ve bağlantısı düşürülür (yeniden bağlanır); öbür modüller beklemez.
const QUEUE: usize = 256;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Event {
    /// Bir modül bağlandı.
    Hello { module: String },
    /// Tüm modüller düzgünce kapansın.
    Shutdown,
    /// Tam ekran uygulama/oyun durumu değişti. `monitor`: tam ekran pencerenin
    /// monitörünün aygıt adı (`win::monitor_name`); Windows görev çubuğu gibi
    /// yüzeyler yalnızca kendi monitörlerindeki tam ekranda gizlenir.
    Fullscreen {
        active: bool,
        exclusive: bool,
        #[serde(default)]
        monitor: String,
    },
    /// Win+D / "masaüstünü göster" algılandı; yüzeyler kendini yeniden göstermeli.
    ShowDesktop,
    /// Explorer yeniden başladı (görev çubuğu yeniden oluşturuldu).
    ExplorerRestarted,
    /// Genel amaçlı modül mesajı.
    Custom { name: String, #[serde(default)] data: serde_json::Value },
}

fn pipe_name(s: &str) -> HSTRING {
    HSTRING::from(s)
}

fn create_instance(name: &str, inbound: bool) -> Result<HANDLE> {
    // Yalnızca bu kullanıcı + SYSTEM; kurulamazsa Windows'un varsayılanı
    // (o da yazmayı sahiple sınırlar) ve süreç doğrulaması yine devrede.
    let sec = auth::PipeSecurity::new();
    if sec.is_none() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| crate::warn!("bus: boru güvenlik tanımı kurulamadı, varsayılan kullanılıyor"));
    }
    let sa = sec.as_ref().map(|s| s.attrs());
    unsafe {
        let mode = if inbound { PIPE_ACCESS_INBOUND } else { PIPE_ACCESS_OUTBOUND };
        let h = CreateNamedPipeW(
            &pipe_name(name),
            mode,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            64 * 1024,
            64 * 1024,
            0,
            sa.as_ref().map(|s| s as *const _),
        );
        if h.is_invalid() {
            return Err(Error::from_win32());
        }
        Ok(h)
    }
}

fn wait_client(h: HANDLE) -> bool {
    unsafe {
        match ConnectNamedPipe(h, None) {
            Ok(()) => true,
            Err(e) => e.code() == ERROR_PIPE_CONNECTED.to_hresult(),
        }
    }
}

/// Sunucu tarafı: bağlanan istemciyi doğrular; yabancıysa loglar ve `false`
/// (çağıran handle'ı bırakınca bağlantı kapanır).
fn client_ok(f: &File, pipe: &str) -> bool {
    let pid = auth::client_pid(HANDLE(f.as_raw_handle() as _));
    match auth::verify_pid(pid) {
        Ok(()) => true,
        Err(e) => {
            if auth::should_log(&format!("{pipe}:{e}"), Duration::from_secs(60)) {
                crate::warn!("bus ({pipe}): bağlantı reddedildi, {e}");
            }
            false
        }
    }
}

/// İstemci tarafı: borunun sunucusu gerçekten bizim ana süreç mi (boru adını
/// önce kapan bir sahte sunucuya olay sızmasın).
fn server_ok(f: &File) -> bool {
    let pid = auth::server_pid(HANDLE(f.as_raw_handle() as _));
    match auth::verify_pid(pid) {
        Ok(()) => true,
        Err(e) => {
            // Bağlanma döngüsü 2 sn'de bir dener; uyarı 5 dakikada bir.
            if auth::should_log(&format!("sunucu:{e}"), Duration::from_secs(300)) {
                crate::warn!("bus: sunucu doğrulanamadı, bağlanılmadı: {e}");
            }
            false
        }
    }
}

/// Satırları sınırlı okuyup çözer; akış bitince, okuma hatasında ya da
/// `MAX_LINE`'ı aşan satırda döner (çağıran bağlantıyı düşürür).
fn read_events(f: File, who: &str, mut on: impl FnMut(Event)) {
    let mut r = BufReader::new(f);
    loop {
        match line::read_line(&mut r, MAX_LINE) {
            Ok(Some(l)) => match serde_json::from_slice::<Event>(&l) {
                Ok(ev) => on(ev),
                Err(e) => crate::warn!("bus ({who}): geçersiz mesaj: {e}"),
            },
            Ok(None) => return,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::InvalidData {
                    crate::warn!("bus ({who}): {MAX_LINE} baytı aşan satır, bağlantı düşürüldü");
                }
                return;
            }
        }
    }
}

fn encode(ev: &Event) -> Vec<u8> {
    let mut line = serde_json::to_vec(ev).unwrap_or_default();
    line.push(b'\n');
    line
}

fn write_line(f: &mut File, ev: &Event) -> std::io::Result<()> {
    f.write_all(&encode(ev))
}

/// Aşağı borusuna kendi iş parçacığından yazan modül bağlantısı: borusunu
/// okumayan (donmuş) bir modül yayını engelleyemez.
fn spawn_writer(mut f: File) -> Option<SyncSender<Arc<Vec<u8>>>> {
    let (tx, rx) = mpsc::sync_channel::<Arc<Vec<u8>>>(QUEUE);
    std::thread::Builder::new()
        .name("bus-writer".into())
        .spawn(move || {
            for line in rx {
                if f.write_all(&line).is_err() {
                    break;
                }
            }
        })
        .ok()?;
    Some(tx)
}

/// Ana süreçteki olay yolu sunucusu.
#[derive(Clone)]
pub struct BusServer {
    clients: Arc<Mutex<Vec<SyncSender<Arc<Vec<u8>>>>>>,
}

impl BusServer {
    /// Sunucuyu başlatır. Modüllerden gelen her olay önce `on_event`'e verilir,
    /// sonra tüm modüllere yayınlanır.
    pub fn start(on_event: impl Fn(&Event) + Send + Sync + 'static) -> Self {
        let server = Self { clients: Arc::new(Mutex::new(Vec::new())) };
        let on_event = Arc::new(on_event);

        let down = server.clone();
        std::thread::Builder::new()
            .name("bus-down".into())
            .spawn(move || loop {
                match create_instance(down_name(), false) {
                    Ok(h) if wait_client(h) => {
                        let f = unsafe { File::from_raw_handle(h.0 as _) };
                        if !client_ok(&f, "aşağı") {
                            continue;
                        }
                        if let Some(tx) = spawn_writer(f) {
                            down.clients.lock().unwrap_or_else(|e| e.into_inner()).push(tx);
                        }
                    }
                    Ok(h) => unsafe {
                        let _ = CloseHandle(h);
                    },
                    Err(e) => {
                        crate::error!("bus (aşağı) oluşturulamadı: {e}");
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
            })
            .ok();

        let up = server.clone();
        std::thread::Builder::new()
            .name("bus-up".into())
            .spawn(move || loop {
                match create_instance(up_name(), true) {
                    Ok(h) if wait_client(h) => {
                        let f = unsafe { File::from_raw_handle(h.0 as _) };
                        let srv = up.clone();
                        let cb = on_event.clone();
                        // Doğrulama da bu iş parçacığında: kabul döngüsü beklemesin.
                        std::thread::spawn(move || {
                            if !client_ok(&f, "yukarı") {
                                return;
                            }
                            read_events(f, "yukarı", |ev| {
                                cb(&ev);
                                srv.publish(&ev);
                            });
                        });
                    }
                    Ok(h) => unsafe {
                        let _ = CloseHandle(h);
                    },
                    Err(e) => {
                        crate::error!("bus (yukarı) oluşturulamadı: {e}");
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
            })
            .ok();

        server
    }

    /// Tüm bağlı modüllere gönderir; beklemez. Kopmuş ya da kuyruğu dolmuş
    /// (donmuş) bağlantıları düşürür: yazıcı biter, boru kapanır, modül
    /// kopmayı görüp yeniden bağlanır.
    pub fn publish(&self, ev: &Event) {
        let line = encode(ev);
        // Modüllerin okuyucusu bunu reddedip bağlantıyı düşürürdü.
        if line.len() > MAX_LINE {
            crate::warn!("bus: {} baytlık olay yayınlanmadı (sınır {MAX_LINE})", line.len());
            return;
        }
        let line = Arc::new(line);
        let mut clients = self.clients.lock().unwrap_or_else(|e| e.into_inner());
        clients.retain(|tx| match tx.try_send(line.clone()) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                crate::warn!("bus: bir modül olayları okumuyor, bağlantısı düşürüldü");
                false
            }
            Err(TrySendError::Disconnected(_)) => false,
        });
    }
}

/// Modül tarafı bağlantı. Ana süreç yoksa modül tek başına çalışmaya devam
/// eder ve arka planda bağlanmayı denemeyi sürdürür.
#[derive(Clone)]
pub struct BusClient {
    up: Arc<Mutex<Option<File>>>,
}

/// Boruyu açar ve sunucusunu doğrular. Yazma ucunda sunucu kimliği
/// sorgulanabilsin diye öznitelik okuma hakkı da istenir.
fn open_pipe(name: &str, write: bool) -> Option<File> {
    let access = if write { FILE_GENERIC_WRITE.0 | FILE_READ_ATTRIBUTES.0 } else { FILE_GENERIC_READ.0 };
    let f = OpenOptions::new().read(!write).write(write).access_mode(access).open(name).ok()?;
    server_ok(&f).then_some(f)
}

impl BusClient {
    pub fn connect(module: &str, on_event: impl Fn(Event) + Send + 'static) -> Self {
        let client = Self { up: Arc::new(Mutex::new(None)) };
        let me = client.clone();
        let module = module.to_string();
        std::thread::Builder::new()
            .name("bus-client".into())
            .spawn(move || loop {
                let (Some(down), Some(up)) = (open_pipe(down_name(), false), open_pipe(up_name(), true)) else {
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                };
                *me.up.lock().unwrap_or_else(|e| e.into_inner()) = Some(up);
                me.send(&Event::Hello { module: module.clone() });
                crate::info!("olay yoluna bağlanıldı");
                read_events(down, "aşağı", &on_event);
                *me.up.lock().unwrap_or_else(|e| e.into_inner()) = None;
                crate::warn!("olay yolu bağlantısı koptu");
                std::thread::sleep(Duration::from_secs(1));
            })
            .ok();
        client
    }

    /// Kısa ömürlü araçlar için: bağlantı kurmadan tek bir olay gönderir
    /// (ör. adaya bildirim). Ana süreç çalışmıyorsa `false`.
    pub fn send_once(ev: &Event) -> bool {
        // Sunucu bir sonraki boru örneğini açana kadar kısa süre "meşgul" olabilir.
        for _ in 0..10 {
            if let Some(mut f) = open_pipe(up_name(), true) {
                return write_line(&mut f, ev).is_ok();
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        false
    }

    pub fn is_connected(&self) -> bool {
        self.up.lock().map(|u| u.is_some()).unwrap_or(false)
    }

    pub fn send(&self, ev: &Event) {
        let mut up = self.up.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(f) = up.as_mut() {
            if write_line(f, ev).is_err() {
                *up = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Canlı olay yoluna dokunmadan, bu sürece özel bir boruyla: DACL'li boru
    /// kurulur, aynı süreç istemci olarak bağlanır, iki uç da birbirini
    /// doğrular ve olay sınırlı okuyucuyla geri okunur.
    #[test]
    fn ayni_surec_boruda_dogrulanir() {
        let name = format!(r"\\.\pipe\carlui-bus-test-{}", unsafe { GetCurrentProcessId() });
        let h = create_instance(&name, true).expect("boru");
        let n2 = name.clone();
        let t = std::thread::spawn(move || {
            let mut f = open_pipe(&n2, true).expect("istemci açılıp sunucu doğrulanmalı");
            write_line(&mut f, &Event::Custom { name: "x".into(), data: serde_json::json!({ "a": 1 }) }).unwrap();
        });
        assert!(wait_client(h));
        let f = unsafe { File::from_raw_handle(h.0 as _) };
        assert!(client_ok(&f, "test"));
        t.join().unwrap();
        let mut got = Vec::new();
        read_events(f, "test", |ev| got.push(ev));
        assert_eq!(got, vec![Event::Custom { name: "x".into(), data: serde_json::json!({ "a": 1 }) }]);
    }
}
