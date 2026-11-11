//! Bu modül "ayrı süreçlerdeki modüller birbirinden nasıl haberdar olur?"
//! sorununu iki yönlü named pipe ile çözüyor. Ana süreç (`carlui.exe`) sunucu
//! olur; her modül bir "yukarı" (modül → ana) ve bir "aşağı" (ana → modül)
//! bağlantısı açar. Ana sürece gelen her olay tüm modüllere yayınlanır.
//! Mesajlar satır başına bir JSON'dur. İki ayrı boru kullanılmasının sebebi,
//! Windows'ta aynı senkron handle üzerinde okuma beklerken yazmanın
//! kilitlenmesidir.

use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Write},
    os::windows::io::FromRawHandle,
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
    static N: OnceLock<(String, String)> = OnceLock::new();
    N.get_or_init(|| {
        let mut session = 0;
        unsafe {
            let _ = ProcessIdToSessionId(GetCurrentProcessId(), &mut session);
        }
        (format!(r"\\.\pipe\carlui-bus-up-{session}"), format!(r"\\.\pipe\carlui-bus-down-{session}"))
    })
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
            None,
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
                        std::thread::spawn(move || {
                            for line in BufReader::new(f).lines() {
                                let Ok(line) = line else { break };
                                match serde_json::from_str::<Event>(&line) {
                                    Ok(ev) => {
                                        cb(&ev);
                                        srv.publish(&ev);
                                    }
                                    Err(e) => crate::warn!("bus: geçersiz mesaj: {e}"),
                                }
                            }
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
        let line = Arc::new(encode(ev));
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

fn open_pipe(name: &str, write: bool) -> Option<File> {
    OpenOptions::new().read(!write).write(write).open(name).ok()
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
                for line in BufReader::new(down).lines() {
                    let Ok(line) = line else { break };
                    if let Ok(ev) = serde_json::from_str::<Event>(&line) {
                        on_event(ev);
                    }
                }
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
