//! Olay yolu doğrulamasının gerçek süreçler arası denemesi. Test exe'si kendini
//! "çocuk istemci" kipinde yeniden başlatır (`CARLUI_BUS_CHILD`); canlı CarlUI
//! borularına dokunmamak için `bus::use_test_pipes` ile teste özel adlar
//! kullanılır. Senaryolar tek testte sırayla: süreç başına tek sunucu olabilir.
//!
//! 1. Aynı exe, başka süreç: Hello + olay yukarı, yayın aşağı çalışır.
//! 2. Exe başka klasöre kopyalanınca sunucu reddeder (ham istemci) ve
//!    BusClient de sahte sayılan sunucuya bağlanmaz.
//! 3. Kabul edilmiş istemci sınırı aşan satır yollarsa düşürülür; sunucu
//!    sonraki istemciye hizmet etmeyi sürdürür.

use carlui_core::bus::{self, BusClient, BusServer, Event};
use serde_json::json;
use std::{
    collections::HashSet,
    fs::OpenOptions,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};

const CHILD: &str = "CARLUI_BUS_CHILD";
const TAG: &str = "CARLUI_BUS_TAG";
const ID: &str = "CARLUI_BUS_ID";

fn ping(id: &str) -> Event {
    Event::Custom { name: "ping".into(), data: json!({ "id": id }) }
}

/// Çocuk süreç girişi. Normal test koşusunda ortam değişkeni yoktur, hiçbir şey yapmaz.
#[test]
fn child_entry() {
    let Ok(mode) = std::env::var(CHILD) else { return };
    let tag = std::env::var(TAG).expect("tag");
    let id = std::env::var(ID).expect("id");
    assert!(bus::use_test_pipes(&tag));
    match mode.as_str() {
        "client" => child_client(&id),
        "raw" => child_raw(&id),
        "long" => child_long(&id),
        "noop" => {}
        m => panic!("bilinmeyen kip {m}"),
    }
}

/// Gerçek istemci API'si: bağlan, ping yolla, kendi kimliğiyle pong bekle.
fn child_client(id: &str) {
    let (tx, rx) = mpsc::channel();
    let me = id.to_string();
    let bus = BusClient::connect(id, move |ev| {
        if let Event::Custom { name, data } = ev {
            if name == "pong" && data["id"] == me.as_str() {
                let _ = tx.send(());
            }
        }
    });
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if bus.is_connected() {
            bus.send(&ping(id));
        }
        if rx.recv_timeout(Duration::from_millis(250)).is_ok() {
            return;
        }
    }
    panic!("olay alışverişi tamamlanamadı");
}

/// Ham istemci: doğrulamasız açar, Hello + ping yazar, aşağıdan pong bekler.
fn child_raw(id: &str) {
    let (up, down) = bus::pipe_names();
    let down = OpenOptions::new().read(true).open(down).expect("aşağı açılmalı");
    let mut up = OpenOptions::new().write(true).open(up).expect("yukarı açılmalı");
    let mut lines = String::new();
    for ev in [Event::Hello { module: id.into() }, ping(id)] {
        lines += &serde_json::to_string(&ev).unwrap();
        lines.push('\n');
    }
    let _ = up.write_all(lines.as_bytes());
    // Sunucu reddederse aşağı boru kapanır: okuma hemen biter.
    let (tx, rx) = mpsc::channel();
    let id2 = id.to_string();
    std::thread::spawn(move || {
        for l in BufReader::new(down).lines() {
            let Ok(l) = l else { break };
            if l.contains("\"pong\"") && l.contains(&id2) {
                let _ = tx.send(());
                return;
            }
        }
    });
    if rx.recv_timeout(Duration::from_secs(4)).is_err() {
        panic!("olay alışverişi tamamlanamadı");
    }
}

/// Kabul edilen istemci sınırı aşan satır yollar; bağlantının kesildiğini görmeli.
fn child_long(_id: &str) {
    let (up, _) = bus::pipe_names();
    let mut up = OpenOptions::new().write(true).open(up).expect("yukarı açılmalı");
    let chunk = vec![b'x'; 64 * 1024];
    let end = Instant::now() + Duration::from_secs(6);
    // 256 KiB'tan fazlasını satır sonu olmadan yaz; sunucu kesince yazma hata verir.
    while Instant::now() < end {
        if up.write_all(&chunk).is_err() {
            return;
        }
    }
    panic!("uzun satıra rağmen bağlantı düşürülmedi");
}

/// Çocuğu başlatır, en çok `limit` bekler (aşarsa öldürür); başarılı mı.
fn run_child(exe: &Path, tag: &str, mode: &str, id: &str, limit: Duration) -> bool {
    let mut c = Command::new(exe)
        .args(["--exact", "child_entry", "--nocapture", "--test-threads=1"])
        .env(CHILD, mode)
        .env(TAG, tag)
        .env(ID, id)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("çocuk başlatılamadı");
    let end = Instant::now() + limit;
    loop {
        if let Ok(Some(s)) = c.try_wait() {
            return s.success();
        }
        if Instant::now() > end {
            let _ = c.kill();
            let _ = c.wait();
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn surecler_arasi_dogrulama() {
    if std::env::var(CHILD).is_ok() {
        return;
    }
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let tag = format!("{}-{nanos}", std::process::id());
    assert!(bus::use_test_pipes(&tag));

    // Sunucu: ping gelince aynı kimlikle pong yayınlar; gelen ping kimliklerini toplar.
    let seen: Arc<Mutex<HashSet<String>>> = Arc::default();
    let server: Arc<Mutex<Option<BusServer>>> = Arc::default();
    let (s2, srv2) = (seen.clone(), server.clone());
    let srv = BusServer::start(move |ev| {
        if let Event::Custom { name, data } = ev {
            if name == "ping" {
                let id = data["id"].as_str().unwrap_or_default().to_string();
                s2.lock().unwrap().insert(id.clone());
                if let Some(s) = srv2.lock().unwrap().as_ref() {
                    s.publish(&Event::Custom { name: "pong".into(), data: json!({ "id": id }) });
                }
            }
        }
    });
    *server.lock().unwrap() = Some(srv);

    let exe = std::env::current_exe().unwrap();
    let limit = Duration::from_secs(10);

    // 1. Olumlu: aynı klasördeki başka süreç iki yönde de konuşur.
    assert!(run_child(&exe, &tag, "client", "iyi-1", limit), "meşru istemci bağlanamadı");
    assert!(seen.lock().unwrap().contains("iyi-1"));
    // Ham istemci de aynı klasörden geçer: aşağıdaki ret ancak konumdan olabilir.
    assert!(run_child(&exe, &tag, "raw", "iyi-ham", limit), "meşru ham istemci bağlanamadı");

    // 2. Olumsuz: başka klasöre kopyalanmış exe.
    let dir = std::env::temp_dir().join(format!("carlui-bus-cross-{tag}"));
    std::fs::create_dir_all(&dir).unwrap();
    let copy = dir.join(exe.file_name().unwrap());
    std::fs::copy(&exe, &copy).unwrap();
    // Kopya çalışabiliyor olmalı; yoksa ret sahte başarı olurdu.
    let copy_runs = run_child(&copy, &tag, "noop", "kopya", limit);
    let raw_ok = run_child(&copy, &tag, "raw", "yabanci-ham", limit);
    let client_ok = run_child(&copy, &tag, "client", "yabanci-istemci", limit);
    // Silme, çocuk kapanır kapanmaz kilitli olabilir: birkaç kez dene.
    for _ in 0..20 {
        if std::fs::remove_dir_all(&dir).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(copy_runs, "kopyalanan test exe'si çalışmadı");
    assert!(!raw_ok, "yabancı ham istemci reddedilmedi");
    assert!(!client_ok, "yabancı klasördeki istemci alışverişi tamamladı");
    {
        let s = seen.lock().unwrap();
        assert!(!s.contains("yabanci-ham") && !s.contains("yabanci-istemci"), "sunucu yabancıdan olay aldı: {s:?}");
    }

    // 3. Uzun satır: istemci düşer, sunucu sonraki istemciye hizmet eder.
    assert!(run_child(&exe, &tag, "long", "uzun", limit), "uzun satırlı istemci düşürülmedi");
    assert!(run_child(&exe, &tag, "client", "iyi-2", limit), "uzun satırdan sonra sunucu hizmet etmiyor");
    assert!(seen.lock().unwrap().contains("iyi-2"));
}
