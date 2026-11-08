//! Bu modül "asistan uzun cümleleri nasıl anlar?" sorununu yerel bir dil modeliyle
//! çözüyor. llama.cpp sunucusu yalnızca bir komut boyunca açıktır: asistan
//! tuşuna basınca açılır, yanıt verince kapanır. Yalnızca 127.0.0.1'i dinler ve
//! rastgele anahtar ister. Model cümleyi `GRAMMAR`'daki kısa satırlara çevirir;
//! dil bilgisi dışında yazamaz, satırları da `carlui_launcher::voice_lines`
//! izinli eylemlere çevirir, yani modelin kendi başına çalıştırabildiği bir şey
//! yoktur. Uzun yönerge her açılışta yeniden işlenmesin diye işlenmiş hali
//! (`cache/prompt-*.bin`) bir kez kaydedilir, sonra saniyeler yerine
//! milisaniyede geri yüklenir. İşlemcide ya da (Vulkan ile) ekran kartında
//! çalışır. Ses ve metin bilgisayardan çıkmaz.

use crate::{catalog::Model, store};
use std::{
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    os::windows::{io::AsRawHandle, process::CommandExt},
    process::{Child, Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::{
        JobObjects::*,
        Threading::{BELOW_NORMAL_PRIORITY_CLASS, CREATE_NO_WINDOW},
    },
};

const PROMPT: &str = include_str!("brain_prompt.txt");
const GRAMMAR: &str = r#"root ::= line ("\n" line)? ("\n" line)?
line ::= "none" | "media|" ("next" | "prev" | "toggle") | "open|" t | "close|" t | "play|" t | "search|" w "|" t | "volume|" w? "|" n | "step|" w? "|" ("10" | "-10") | "mute|" w? "|" ("on" | "off") | "split|" t "|" d | "move|" t "|" d
t ::= [^|\n]{1,60}
w ::= [^|\n,0-9] [^|\n,]{0,39}
n ::= [0-9] | [1-9][0-9] | "100"
d ::= [0-6]
"#;
pub const ENGINE: &str = "llama-server.exe";
pub const WEIGHTS: &str = "model.gguf";
/// Yönerge ve en uzun cümle birlikte sığmalı.
const CONTEXT: &str = "3072";
/// Sunucunun açılması (yavaş diskte modelin okunması dahil) için en uzun bekleme.
const START_MAX: Duration = Duration::from_secs(120);
/// İlk istek tüm yönergeyi işler; sonrakiler onu önbellekten alır.
const WARM_MAX: Duration = Duration::from_secs(60);
const ASK_MAX: Duration = Duration::from_secs(8);
const MAX_INPUT: usize = 300;

pub struct Brain {
    child: Mutex<Child>,
    /// Bu süreç nasıl biterse bitsin sunucu kapanır (kapanınca iş nesnesi düşer).
    job: isize,
    port: u16,
    key: String,
}

fn free_port() -> Result<u16, String> {
    let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| e.to_string())?;
    Ok(l.local_addr().map_err(|e| e.to_string())?.port())
}

/// Her `RandomState` işletim sisteminden gelen rastgele sayıyla tohumlanır.
fn secret() -> String {
    use std::hash::{BuildHasher, Hasher, RandomState};
    (0..4).map(|_| format!("{:016x}", RandomState::new().build_hasher().finish())).collect()
}

/// Ana çekirdekler yeter; verimli çekirdeklere taşmak modeli yavaşlatır.
fn threads() -> usize {
    (std::thread::available_parallelism().map_or(4, |n| n.get()) / 2).clamp(2, 6)
}

fn tie_to_us(child: &Child) -> Result<isize, String> {
    unsafe {
        let job = CreateJobObjectW(None, None).map_err(|e| e.to_string())?;
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let size = std::mem::size_of_val(&info) as u32;
        let tied = SetInformationJobObject(job, JobObjectExtendedLimitInformation, &info as *const _ as _, size).and_then(|_| AssignProcessToJobObject(job, HANDLE(child.as_raw_handle() as _)));
        if let Err(e) = tied {
            let _ = CloseHandle(job);
            return Err(e.to_string());
        }
        Ok(job.0 as isize)
    }
}

/// Yönerge değişince eski kayıt kullanılmasın diye adı yönergenin özetini taşır.
/// İşlemci ve ekran kartı kayıtları ayrıdır.
fn cache_name(gpu: bool) -> String {
    // FNV-1a: sürümden sürüme aynı kalır.
    let h = PROMPT.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    format!("prompt-{h:016x}-{}.bin", if gpu { "gpu" } else { "cpu" })
}

/// Eski yönergelerin kayıtlarını siler (her biri yüz MB'ı bulabilir).
fn forget_old(cache: &std::path::Path) {
    let keep = [cache_name(false), cache_name(true)];
    for e in std::fs::read_dir(cache).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with("prompt-") && !keep.contains(&name) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Yanıtın gövdesi; yalnızca 200 kabul edilir.
fn body_of(reply: &str) -> Option<&str> {
    let (head, body) = reply.split_once("\r\n\r\n")?;
    head.split_whitespace().nth(1).filter(|c| *c == "200")?;
    Some(&body[body.find('{')?..=body.rfind('}')?])
}

impl Brain {
    /// Sunucuyu açar, hazır olmasını bekler ve işlenmiş yönergeyi yükler (yoksa
    /// bir kez işleyip kaydeder). `gpu`: ekran kartında çalış. Ayrı iş parçacığında
    /// çağrılır; konuşma sürerken açılır.
    pub fn start(m: &Model, gpu: bool) -> Result<Brain, String> {
        let dir = store::dir(m);
        let exe = store::find(&dir, ENGINE).ok_or("llama-server yok")?;
        let weights = store::find(&dir, WEIGHTS).ok_or("model dosyası yok")?;
        let cache = dir.join("cache");
        std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
        let (port, key) = (free_port()?, secret());
        let device: &[&str] = if gpu { &["-dev", "Vulkan0", "-ngl", "99"] } else { &["-dev", "none", "-ngl", "0"] };
        let child = Command::new(&exe)
            .current_dir(exe.parent().unwrap_or(&dir))
            .arg("-m")
            .arg(&weights)
            .args(["--host", "127.0.0.1", "--port", &port.to_string(), "--api-key", &key, "-c", CONTEXT, "-np", "1", "-t", &threads().to_string()])
            .args(device)
            .arg("--slot-save-path")
            .arg(&cache)
            // Yeniden paketleme ağırlıkların ikinci kopyasını çıkarır (+1,7 GB), hız kazandırmaz.
            .args(["--no-repack", "--cache-ram", "0", "--no-ui"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags((CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS).0)
            .spawn()
            .map_err(|e| e.to_string())?;
        let job = tie_to_us(&child).unwrap_or_else(|e| {
            warn!("asistan: sunucu sürece bağlanamadı: {e}");
            0
        });
        let me = Brain { child: Mutex::new(child), job, port, key };
        let t0 = Instant::now();
        while !me.healthy() {
            if !me.alive() {
                return Err("sunucu kapandı".into());
            }
            if t0.elapsed() > START_MAX {
                return Err("sunucu açılamadı".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let saved = cache_name(gpu);
        let restored = cache.join(&saved).is_file() && me.slot("restore", &saved).is_some_and(|v| v["n_restored"].as_u64().unwrap_or(0) > 0);
        if !restored {
            // İlk açılış: yönerge bir kez işlenir ve sonraki açılışlar için kaydedilir.
            me.request("sesi 30 yap", WARM_MAX).ok_or("ilk istek yanıtlanmadı")?;
            if me.slot("save", &saved).is_some() {
                forget_old(&cache);
            }
        }
        info!("asistan: dil modeli hazır ({}, {} ms{})", if gpu { "ekran kartı" } else { "işlemci" }, t0.elapsed().as_millis(), if restored { "" } else { ", yönerge işlendi" });
        Ok(me)
    }

    /// İşlenmiş yönergeyi kaydeder ya da geri yükler (`what`: "save" | "restore").
    fn slot(&self, what: &str, file: &str) -> Option<serde_json::Value> {
        self.post(&format!("/slots/0?action={what}"), &serde_json::json!({ "filename": file }).to_string(), WARM_MAX)
    }

    fn post(&self, path: &str, body: &str, wait: Duration) -> Option<serde_json::Value> {
        let msg = format!(
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            self.key,
            body.len()
        );
        let reply = self.talk(&msg, wait)?;
        serde_json::from_str(body_of(&reply)?).ok()
    }

    pub fn alive(&self) -> bool {
        self.child.lock().unwrap_or_else(|e| e.into_inner()).try_wait().is_ok_and(|s| s.is_none())
    }

    fn talk(&self, msg: &str, wait: Duration) -> Option<String> {
        let at = SocketAddr::from((Ipv4Addr::LOCALHOST, self.port));
        let mut s = TcpStream::connect_timeout(&at, Duration::from_secs(1)).ok()?;
        s.set_read_timeout(Some(wait)).ok()?;
        s.set_write_timeout(Some(wait)).ok()?;
        s.write_all(msg.as_bytes()).ok()?;
        let mut out = Vec::new();
        s.read_to_end(&mut out).ok()?;
        Some(String::from_utf8_lossy(&out).into_owned())
    }

    fn healthy(&self) -> bool {
        self.talk("GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n", Duration::from_secs(1)).is_some_and(|r| body_of(&r).is_some_and(|b| b.contains("\"ok\"")))
    }

    fn request(&self, text: &str, wait: Duration) -> Option<String> {
        let text: String = text.chars().take(MAX_INPUT).collect();
        let body = serde_json::json!({
            "messages": [{ "role": "system", "content": PROMPT }, { "role": "user", "content": text }],
            "temperature": 0,
            "max_tokens": 120,
            "cache_prompt": true,
            "grammar": GRAMMAR,
        })
        .to_string();
        let v = self.post("/v1/chat/completions", &body, wait)?;
        v["choices"][0]["message"]["content"].as_str().map(str::to_string)
    }

    /// Cümlenin satırları; sunucu yanıt vermezse `None`, çağıran kurallara döner.
    pub fn ask(&self, text: &str) -> Option<String> {
        self.request(text, ASK_MAX)
    }
}

impl Drop for Brain {
    fn drop(&mut self) {
        let c = self.child.get_mut().unwrap_or_else(|e| e.into_inner());
        let _ = c.kill();
        let _ = c.wait();
        if self.job != 0 {
            let _ = unsafe { CloseHandle(HANDLE(self.job as _)) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_good_replies() {
        assert_eq!(body_of("HTTP/1.1 200 OK\r\nA: b\r\n\r\n{\"status\":\"ok\"}"), Some("{\"status\":\"ok\"}"));
        assert_eq!(body_of("HTTP/1.1 200 OK\r\n\r\n1a\r\n{\"x\":1}\r\n0\r\n\r\n"), Some("{\"x\":1}"));
        assert_eq!(body_of("HTTP/1.1 401 Unauthorized\r\n\r\n{\"error\":1}"), None);
        assert_eq!(body_of("HTTP/1.1 503 Busy\r\n\r\n{}"), None);
        assert_eq!(body_of("yarım"), None);
    }

    #[test]
    fn secrets_differ() {
        assert_eq!(secret().len(), 64);
        assert_ne!(secret(), secret());
    }
}
