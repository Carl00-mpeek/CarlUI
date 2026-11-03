//! Bu modül "tarayıcı açmadan bir dosyayı indir ve ilerlemesini gör" sorununu
//! şu yöntemle çözüyor: Aramaya bağlantı yapıştırıp "İndir" seçilince
//! (ya da "indir <adres>") Windows'un kendi HTTP katmanı (WinHTTP; sistem
//! vekil sunucusu ve yönlendirmeler dahil) ile ayrı bir iş parçacığında
//! İndirilenler'e yazılır. Toplam boyut sunucudan geldiği için adanın hapında
//! gerçek yüzde ve hız görünür ("↓ %45 · 3,2 MB/sn"); bitince tıklanınca
//! dosyayı açan bildirim gelir. Yarım dosya `.carlui-part` adıyla durur ve
//! yalnızca tamamlanınca asıl adını alır; iptal/hata olursa silinir (kendi
//! yarım dosyamız, kullanıcı dosyası değil).
//!
//! Tarayıcının kendi indirmeleri ayrı izlenir (bkz. `downloads.rs`).

use crate::*;
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, AtomicU8},
};
use windows::Win32::Networking::WinHttp::*;

pub(crate) const TIMER_DL: usize = 31;
const PART: &str = ".carlui-part";

const RUNNING: u8 = 0;
const DONE: u8 = 1;
const FAILED: u8 = 2;
const CANCELLED: u8 = 3;

pub(crate) struct Job {
    url: String,
    /// Ek istek başlıkları ("Cookie: …\r\nReferer: …"): eklentiden gelen oturum.
    headers: String,
    /// Kullanıcının "Farklı kaydet" ile seçtiği tam yol.
    dest: Option<PathBuf>,
    name: Mutex<String>,
    path: Mutex<Option<PathBuf>>,
    error: Mutex<String>,
    got: AtomicU64,
    total: AtomicU64,
    /// Bayt/sn (f64 bitleri).
    speed: AtomicU64,
    state: AtomicU8,
    cancel: AtomicBool,
}

static JOBS: Mutex<Vec<Arc<Job>>> = Mutex::new(Vec::new());

fn jobs() -> Vec<Arc<Job>> {
    JOBS.lock().map(|v| v.clone()).unwrap_or_default()
}

fn running() -> Vec<Arc<Job>> {
    jobs().into_iter().filter(|j| j.state.load(Ordering::Relaxed) == RUNNING).collect()
}

/// Süren indirmeler: (ad, inen, toplam (0 = bilinmiyor), bayt/sn).
pub fn active() -> Vec<(String, u64, u64, f64)> {
    running()
        .iter()
        .map(|j| {
            let name = j.name.lock().map(|n| n.clone()).unwrap_or_default();
            (name, j.got.load(Ordering::Relaxed), j.total.load(Ordering::Relaxed), f64::from_bits(j.speed.load(Ordering::Relaxed)))
        })
        .collect()
}

/// Uzun dosya adını ortadan kısaltır, uzantıyı korur: "ubuntu-24.04…iso".
fn short_name(name: &str, max: usize) -> String {
    let n: Vec<char> = name.chars().collect();
    if n.len() <= max {
        return name.to_string();
    }
    let ext = name.rfind('.').map(|i| name[i + 1..].chars().count()).filter(|&e| e > 0 && e <= 5).unwrap_or(0);
    let head = max.saturating_sub(ext + 1).max(1);
    let tail: String = n[n.len() - ext..].iter().collect();
    format!("{}…{tail}", n[..head].iter().collect::<String>())
}

/// Kalan süre: "45 sn kaldı", "3 dk kaldı", "1 sa 5 dk kaldı".
fn eta(secs: u64) -> String {
    let (h, m) = (secs / 3600, (secs % 3600) / 60);
    if secs < 60 {
        t!(format!("{secs} sn kaldı"), format!("{secs}s left"))
    } else if h == 0 {
        t!(format!("{m} dk kaldı"), format!("{m} min left"))
    } else {
        t!(format!("{h} sa {m} dk kaldı"), format!("{h} h {m} min left"))
    }
}

/// Haptaki metin; hangi parçaların görüneceği ayarlardan seçilir
/// ("↓ ubuntu…iso · %12 · 420 MB / 3,5 GB · 48 MB/sn · 1 dk kaldı").
pub fn pill(c: &carlui_core::config::DownloadConfig) -> Option<String> {
    // Kendi indirmelerimiz + eklentinin bildirdiği tarayıcı indirmeleri.
    let mut a = active();
    a.extend(crate::bridge::browser());
    if a.is_empty() {
        // Başka bir programın indirmesi (tahmin; Steam'de toplam da bilinir).
        a.extend(crate::netwatch::current().filter(|_| c.others));
    }
    if a.is_empty() {
        return None;
    }
    let got: u64 = a.iter().map(|d| d.1).sum();
    let total: u64 = a.iter().map(|d| d.2).sum();
    let speed: f64 = a.iter().map(|d| d.3).sum();
    let known = a.iter().all(|d| d.2 > 0) && total > 0;
    let mut parts = Vec::new();
    match a.as_slice() {
        [one] if c.name && !one.0.is_empty() => parts.push(short_name(&one.0, 26)),
        [_] => {}
        _ => parts.push(t!(format!("{} dosya", a.len()), format!("{} files", a.len()))),
    }
    if known && c.percent {
        parts.push(carlui_core::lang::pct((got * 100 / total).min(99) as f32));
    }
    if known && c.size {
        parts.push(format!("{} / {}", carlui_sys::bytes(got as f64), carlui_sys::bytes(total as f64)));
    } else if !known || !c.percent {
        parts.push(carlui_sys::bytes(got as f64));
    }
    if speed > 1.0 && c.speed {
        parts.push(format!("{}{}", carlui_sys::bytes(speed), t!("/sn", "/s")));
    }
    if speed > 1.0 && known && c.eta {
        parts.push(eta(((total.saturating_sub(got)) as f64 / speed).ceil() as u64));
    }
    Some(format!("↓ {}", parts.join(" · ")))
}

/// "%20" gibi kodları çözer.
fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Windows'ta geçersiz karakterleri ayıklar.
fn clean(name: &str) -> String {
    let s: String = name.chars().map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c }).collect();
    let s = s.trim().trim_end_matches('.').to_string();
    if s.is_empty() { t!("indirilen", "download").into() } else { s }
}

/// Adresten dosya adı tahmini ("…/setup.exe?x=1" → "setup.exe").
pub fn name_from_url(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let after = path.split_once("://").map(|p| p.1).unwrap_or(path);
    let last = after.split('/').skip(1).filter(|s| !s.is_empty()).last().unwrap_or("");
    let name = decode(last);
    if name.is_empty() { clean(after.split('/').next().unwrap_or(t!("indirilen", "download"))) + ".html" } else { clean(&name) }
}

/// `Content-Disposition: attachment; filename="x.zip"` (filename* önce).
fn name_from_disposition(h: &str) -> Option<String> {
    let lower = h.to_lowercase();
    if let Some(i) = lower.find("filename*=") {
        let v = h[i + 10..].split(';').next()?.trim().trim_matches('"');
        let v = v.split_once("''").map(|p| p.1).unwrap_or(v);
        return Some(clean(&decode(v)));
    }
    let i = lower.find("filename=")?;
    let v = h[i + 9..].split(';').next()?.trim().trim_matches('"');
    (!v.is_empty()).then(|| clean(v))
}

/// Var olan dosyanın üstüne yazmamak için "ad (1).uz".
fn unique(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    if !p.exists() && !dir.join(format!("{name}{PART}")).exists() {
        return p;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (1..1000).map(|n| dir.join(format!("{stem} ({n}){ext}"))).find(|p| !p.exists()).unwrap_or(p)
}

struct H(*mut std::ffi::c_void);

impl Drop for H {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn header(req: &H, level: u32) -> Option<String> {
    let mut buf = vec![0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    let mut idx = 0u32;
    unsafe { WinHttpQueryHeaders(req.0, level, PCWSTR::null(), Some(buf.as_mut_ptr() as _), &mut len, &mut idx) }.ok()?;
    Some(String::from_utf16_lossy(&buf[..len as usize / 2]))
}

/// Açık bir istek; alanlar bu sırayla kapanır (istek → bağlantı → oturum).
struct Conn {
    req: H,
    _con: H,
    _ses: H,
}

/// Başlıkları gönderip yanıtı bekler; 2xx değilse hata.
fn request(url: &str, headers: &str) -> std::result::Result<Conn, String> {
    let (secure, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        return Err(t!("yalnızca http/https adresleri", "only http/https addresses").into());
    };
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() => (h, p.parse().unwrap_or(443)),
        _ => (hostport, if secure { 443 } else { 80 }),
    };
    let path = path.split('#').next().unwrap_or(path);
    unsafe {
        let ses = H(WinHttpOpen(w!("CarlUI"), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0));
        if ses.0.is_null() {
            return Err(t!("ağ açılamadı", "could not open the network").into());
        }
        let host_w = wide(host);
        let con = H(WinHttpConnect(ses.0, PCWSTR(host_w.as_ptr()), port, 0));
        if con.0.is_null() {
            return Err(t!(format!("{host} bağlanılamadı"), format!("could not connect to {host}")));
        }
        let path_w = wide(path);
        let req = H(WinHttpOpenRequest(con.0, w!("GET"), PCWSTR(path_w.as_ptr()), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), if secure { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) }));
        if req.0.is_null() {
            return Err(t!("istek kurulamadı", "could not create the request").into());
        }
        let hdr: Vec<u16> = headers.encode_utf16().collect();
        WinHttpSendRequest(req.0, (!hdr.is_empty()).then_some(hdr.as_slice()), None, 0, 0, 0).map_err(|e| format!("{}: {}", t!("gönderilemedi", "send failed"), e.message()))?;
        WinHttpReceiveResponse(req.0, std::ptr::null_mut()).map_err(|e| format!("{}: {}", t!("yanıt yok", "no response"), e.message()))?;
        let status: u32 = header(&req, WINHTTP_QUERY_STATUS_CODE).and_then(|s| s.trim().parse().ok()).unwrap_or(0);
        if !(200..300).contains(&status) {
            return Err(t!(format!("sunucu {status} döndü"), format!("server returned {status}")));
        }
        Ok(Conn { req, _con: con, _ses: ses })
    }
}

/// İndirmeden önce dosyanın adını ve boyutunu sorar (yalnızca başlıklar okunur).
pub fn probe(url: &str, headers: &str) -> (Option<String>, u64) {
    match request(url, headers) {
        Ok(c) => (
            header(&c.req, WINHTTP_QUERY_CONTENT_DISPOSITION).and_then(|h| name_from_disposition(&h)),
            header(&c.req, WINHTTP_QUERY_CONTENT_LENGTH).and_then(|s| s.trim().parse().ok()).unwrap_or(0),
        ),
        Err(e) => {
            carlui_core::warn!("indirme bilgisi alınamadı: {e}");
            (None, 0)
        }
    }
}

/// İptal edilen işin iç hata işareti (kullanıcıya gösterilmez).
const CANCEL: &str = "cancelled";

fn run(job: &Job) -> std::result::Result<(), String> {
    let c = request(&job.url, &job.headers)?;
    let req = &c.req;
    unsafe {
        let total: u64 = header(req, WINHTTP_QUERY_CONTENT_LENGTH).and_then(|s| s.trim().parse().ok()).unwrap_or(0);
        job.total.store(total, Ordering::Relaxed);
        if let Some(n) = header(req, WINHTTP_QUERY_CONTENT_DISPOSITION).and_then(|h| name_from_disposition(&h)) {
            if job.dest.is_none() {
                if let Ok(mut g) = job.name.lock() {
                    *g = n;
                }
            }
        }
        let name = job.name.lock().map(|n| n.clone()).unwrap_or_default();
        let dest = match &job.dest {
            Some(p) => p.clone(),
            None => unique(&carlui_core::win::downloads_dir().ok_or(t!("İndirilenler klasörü bulunamadı", "Downloads folder not found"))?, &name),
        };
        if let Ok(mut g) = job.name.lock() {
            *g = dest.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or(name);
        }
        let part = PathBuf::from(format!("{}{PART}", dest.display()));
        let mut file = std::fs::File::create(&part).map_err(|e| format!("{}: {e}", t!("dosya açılamadı", "could not open the file")))?;
        let mut buf = vec![0u8; 256 * 1024];
        let (mut mark, mut mark_got) = (Instant::now(), 0u64);
        let result = loop {
            if job.cancel.load(Ordering::Relaxed) {
                break Err(CANCEL.to_string());
            }
            let mut n = 0u32;
            if let Err(e) = WinHttpReadData(req.0, buf.as_mut_ptr() as _, buf.len() as u32, &mut n) {
                break Err(format!("{}: {}", t!("bağlantı koptu", "connection lost"), e.message()));
            }
            if n == 0 {
                break Ok(());
            }
            if let Err(e) = file.write_all(&buf[..n as usize]) {
                break Err(format!("{}: {e}", t!("yazılamadı", "write failed")));
            }
            let got = job.got.fetch_add(n as u64, Ordering::Relaxed) + n as u64;
            let el = mark.elapsed().as_secs_f64();
            if el >= 0.7 {
                job.speed.store(((got - mark_got) as f64 / el).to_bits(), Ordering::Relaxed);
                (mark, mark_got) = (Instant::now(), got);
            }
        };
        drop(file);
        let short = total > 0 && job.got.load(Ordering::Relaxed) < total;
        match result {
            Ok(()) if !short => {
                std::fs::rename(&part, &dest).map_err(|e| format!("{}: {e}", t!("yeniden adlandırılamadı", "rename failed")))?;
                if let Ok(mut g) = job.path.lock() {
                    *g = Some(dest);
                }
                Ok(())
            }
            r => {
                // Yalnızca kendi yarım dosyamız silinir.
                let _ = std::fs::remove_file(&part);
                Err(r.err().unwrap_or_else(|| t!("eksik indi", "incomplete download").into()))
            }
        }
    }
}

impl App {
    pub(crate) fn dl_start(&mut self, url: String) {
        self.dl_start_with(url, String::new(), None, None);
    }

    pub(crate) fn dl_start_with(&mut self, url: String, headers: String, name: Option<String>, dest: Option<PathBuf>) {
        let name = dest.as_ref().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().to_string()).or(name).unwrap_or_else(|| name_from_url(&url));
        let job = Arc::new(Job {
            name: Mutex::new(name),
            url,
            headers,
            dest,
            path: Mutex::new(None),
            error: Mutex::new(String::new()),
            got: AtomicU64::new(0),
            total: AtomicU64::new(0),
            speed: AtomicU64::new(0),
            state: AtomicU8::new(RUNNING),
            cancel: AtomicBool::new(false),
        });
        info!("indirme başladı: {}", job.url);
        if let Ok(mut v) = JOBS.lock() {
            v.push(job.clone());
        }
        let hwnd = self.island.hwnd.0 as isize;
        std::thread::Builder::new()
            .name("download".into())
            .spawn(move || {
                let r = run(&job);
                let state = match &r {
                    Ok(()) => DONE,
                    Err(e) if e == CANCEL => CANCELLED,
                    Err(e) => {
                        if let Ok(mut g) = job.error.lock() {
                            *g = e.clone();
                        }
                        FAILED
                    }
                };
                job.state.store(state, Ordering::Relaxed);
                unsafe {
                    let _ = PostMessageW(HWND(hwnd as _), WM_TIMER, WPARAM(TIMER_DL), LPARAM(0));
                }
            })
            .ok();
        unsafe {
            SetTimer(self.island.hwnd, TIMER_DL, 1000, None);
        }
        self.refresh(true);
    }

    /// Süren indirmeleri iptal eder (arama: "indirmeyi iptal et").
    pub(crate) fn dl_cancel(&mut self) {
        for j in running() {
            j.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub(crate) fn dl_tick(&mut self) {
        let finished: Vec<Arc<Job>> = {
            let Ok(mut v) = JOBS.lock() else { return };
            let (done, keep): (Vec<_>, Vec<_>) = v.drain(..).partition(|j| j.state.load(Ordering::Relaxed) != RUNNING);
            *v = keep;
            done
        };
        for j in finished {
            let name = j.name.lock().map(|n| n.clone()).unwrap_or_default();
            let note = match j.state.load(Ordering::Relaxed) {
                DONE => {
                    let path = j.path.lock().ok().and_then(|p| p.clone());
                    info!("indirme bitti: {name} ({} bayt)", j.got.load(Ordering::Relaxed));
                    Note {
                        app: t!("İndirmeler", "Downloads").into(),
                        title: t!("İndirme tamamlandı", "Download complete").into(),
                        body: format!("{name} · {}", carlui_sys::bytes(j.got.load(Ordering::Relaxed) as f64)),
                        file: path.map(|p| p.display().to_string()),
                        ..Default::default()
                    }
                }
                FAILED => {
                    let e = j.error.lock().map(|e| e.clone()).unwrap_or_default();
                    carlui_core::warn!("indirme başarısız: {} — {e}", j.url);
                    Note { app: t!("İndirmeler", "Downloads").into(), title: t!("İndirilemedi", "Download failed").into(), body: format!("{name} · {e}"), ..Default::default() }
                }
                _ => {
                    info!("indirme iptal edildi: {name}");
                    Note { app: t!("İndirmeler", "Downloads").into(), title: t!("İndirme iptal edildi", "Download cancelled").into(), body: name, ..Default::default() }
                }
            };
            self.push_note(note);
        }
        unsafe {
            if running().is_empty() && crate::bridge::browser().is_empty() {
                let _ = KillTimer(self.island.hwnd, TIMER_DL);
            } else {
                // Tarayıcı bildirmeyi bırakınca hap da bir tık sonra kapansın.
                SetTimer(self.island.hwnd, TIMER_DL, 1000, None);
            }
        }
        if matches!(self.island.view, View::Clock | View::Media | View::Date | View::Hidden | View::Hub) {
            self.refresh(true);
        }
    }
}

/// Tıklanınca dosyayı açar.
pub fn open(path: &str) {
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        windows::Win32::UI::Shell::ShellExecuteW(HWND::default(), w!("open"), &HSTRING::from(path), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_names() {
        assert_eq!(short_name("rapor.pdf", 26), "rapor.pdf");
        assert_eq!(short_name("ubuntu-24.04.3-desktop-amd64.iso", 20), "ubuntu-24.04.3-d…iso");
        assert_eq!(short_name("çok-uzun-bir-dosya-adı-uzantısız", 10), "çok-uzun-…");
    }

    #[test]
    fn etas() {
        carlui_core::lang::apply("tr");
        assert_eq!(eta(45), "45 sn kaldı");
        assert_eq!(eta(185), "3 dk kaldı");
        assert_eq!(eta(3900), "1 sa 5 dk kaldı");
    }

    #[test]
    fn names() {
        assert_eq!(name_from_url("https://x.com/a/setup.exe?v=2"), "setup.exe");
        assert_eq!(name_from_url("https://x.com/dosya%20adi.pdf"), "dosya adi.pdf");
        assert_eq!(name_from_url("https://example.com/"), "example.com.html");
        assert_eq!(name_from_disposition("attachment; filename=\"rapor.zip\""), Some("rapor.zip".into()));
        assert_eq!(name_from_disposition("attachment; filename*=UTF-8''%C3%B6dev.pdf"), Some("ödev.pdf".into()));
        assert_eq!(decode("a%2"), "a%2");
    }
}
