//! Bu modül asistan sürecini adanın arama paneline bağlıyor. Mikrofon ve model
//! burada kalır; ada olay yolundan gelen `assist` olaylarıyla şeritleri ve
//! söylenen yazıyı gösterir, onayı `assist-reply` ile geri yollar. Ada yoksa,
//! tam ekran oyundaysa ya da arama kapalıysa `open` boş döner ve asistan eski
//! küçük kartına düşer.

use carlui_core::bus::{BusClient, Event};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{channel, Receiver, RecvTimeoutError},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use windows::Win32::{Foundation::*, System::Threading::WaitForSingleObject};

/// Adanın yanıtı için en uzun bekleme; süre dolarsa kart gösterilir.
const CONNECT: Duration = Duration::from_millis(300);
const READY: Duration = Duration::from_millis(400);
/// Olay yolunu yormamak için ses seviyesi en sık bu aralıkla gider.
const LEVEL_EVERY: Duration = Duration::from_millis(80);
/// Yanıtsız onay bu süre sonunda bırakılır.
const REVIEW_MAX: Duration = Duration::from_secs(300);

pub struct Panel {
    bus: BusClient,
    rx: Mutex<Receiver<Value>>,
    last_level: Mutex<Instant>,
    /// Ada kendi başına kapanacak ya da çoktan kapandı.
    released: AtomicBool,
}

impl Panel {
    pub fn open() -> Option<Arc<Self>> {
        let (tx, rx) = channel();
        let bus = BusClient::connect("assist", move |ev| {
            if let Event::Custom { name, data } = ev {
                if name == "assist-reply" {
                    let _ = tx.send(data);
                }
            }
        });
        let t0 = Instant::now();
        while !bus.is_connected() {
            if t0.elapsed() > CONNECT {
                return None;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let p = Self { bus, rx: Mutex::new(rx), last_level: Mutex::new(Instant::now()), released: AtomicBool::new(false) };
        p.send(json!({ "op": "open" }));
        let answer = p.rx.lock().ok()?.recv_timeout(READY).ok()?;
        if answer["op"] == "ready" {
            return Some(Arc::new(p));
        }
        p.released.store(true, Ordering::SeqCst);
        None
    }

    fn send(&self, data: Value) {
        self.bus.send(&Event::Custom { name: "assist".into(), data });
    }

    pub fn level(&self, v: f32) {
        if let Ok(mut t) = self.last_level.lock() {
            if t.elapsed() >= LEVEL_EVERY {
                *t = Instant::now();
                self.send(json!({ "op": "level", "v": v }));
            }
        }
    }

    pub fn text(&self, text: &str) {
        self.send(json!({ "op": "text", "text": text }));
    }

    pub fn thinking(&self) {
        self.send(json!({ "op": "thinking" }));
    }

    /// Sonucu gösterir; ada kısa süre sonra kendisi kapanır.
    pub fn done(&self, text: &str, status: &str, failed: bool) {
        self.released.store(true, Ordering::SeqCst);
        self.send(json!({ "op": "done", "text": text, "status": status, "failed": failed }));
    }

    /// Kullanıcı dinleme sürerken aramayı kapattı mı?
    pub fn cancelled(&self) -> bool {
        let Ok(rx) = self.rx.lock() else { return false };
        let gone = rx.try_iter().any(|a| a["op"] == "cancel");
        if gone {
            self.released.store(true, Ordering::SeqCst);
        }
        gone
    }

    /// Yazıyı onaya sunar ve karar bekler. Gönderilen (belki düzeltilmiş) yazıyı
    /// döndürür; vazgeçildiyse ya da kısayola yeniden basıldıysa `None`.
    pub fn review(&self, text: &str, stop: HANDLE) -> Option<String> {
        self.send(json!({ "op": "review", "text": text }));
        let rx = self.rx.lock().ok()?;
        let t0 = Instant::now();
        while t0.elapsed() < REVIEW_MAX {
            if unsafe { WaitForSingleObject(stop, 0) } == WAIT_OBJECT_0 {
                break;
            }
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(a) if a["op"] == "send" => {
                    self.released.store(true, Ordering::SeqCst);
                    return a["text"].as_str().map(str::to_string);
                }
                Ok(a) if a["op"] == "cancel" => {
                    self.released.store(true, Ordering::SeqCst);
                    return None;
                }
                Ok(_) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        None
    }
}

impl Drop for Panel {
    fn drop(&mut self) {
        if !self.released.load(Ordering::SeqCst) {
            self.send(json!({ "op": "close" }));
        }
    }
}
