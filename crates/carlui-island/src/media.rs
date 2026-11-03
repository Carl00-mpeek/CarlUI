//! Bu modül "şu an ne çalıyor ve onu nasıl kontrol ederiz?" sorununu
//! Windows'un medya oturumları API'siyle (GlobalSystemMediaTransportControls)
//! çözüyor. Spotify, tarayıcılar (YouTube), Medya Oynatıcı gibi uygulamalar
//! çaldıklarını buraya bildirir; klavyedeki medya tuşlarının kullandığı yol budur.
//! Komutlar (oynat/duraklat, ileri, geri, sar) doğrudan o uygulamaya gider:
//! YouTube'da açılan şarkı YouTube'dan, Spotify'da açılan Spotify'dan sarılır.
//!
//! Arka plandaki bir iş parçacığı oturum olaylarına abone olur; yalnızca
//! anlamlı bir değişiklik olduğunda (şarkı, durum, sarma) arayüze haber verir.
//! Çalma ilerlemesi arayüzde compositor animasyonuyla akar, saniye saniye
//! güncelleme gerekmez.

use carlui_core::{info, warn};
use std::{
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Foundation::TypedEventHandler,
    Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session,
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    },
    Storage::Streams::{DataReader, IRandomAccessStreamReference},
    Win32::{
        Foundation::*,
        System::{SystemInformation::GetSystemTimeAsFileTime, WinRT::*},
        UI::WindowsAndMessaging::PostMessageW,
    },
};

#[derive(Clone, Debug, Default)]
pub struct Snap {
    /// Kaynak uygulamanın kimliği (ör. "Spotify.exe", "Chrome", Store AUMID).
    pub app: String,
    pub title: String,
    pub artist: String,
    pub playing: bool,
    /// Anlık görüntü alındığındaki konum ve toplam süre (sn). Süre 0 ise bilinmiyor.
    pub pos: f64,
    pub end: f64,
    pub at: Option<Instant>,
    pub can_prev: bool,
    pub can_next: bool,
    pub can_seek: bool,
    /// Kapak görseli (PNG/JPEG baytları).
    pub art: Option<Arc<Vec<u8>>>,
    art_key: String,
}

impl Snap {
    /// Şu anki tahmini konum (sn).
    pub fn position(&self) -> f64 {
        let mut p = self.pos;
        if self.playing {
            if let Some(at) = self.at {
                p += at.elapsed().as_secs_f64();
            }
        }
        if self.end > 0.0 { p.clamp(0.0, self.end) } else { p.max(0.0) }
    }

    /// Arayüzün yeniden çizilmesini gerektiren bir fark var mı?
    fn differs(&self, o: &Snap) -> bool {
        self.app != o.app
            || self.title != o.title
            || self.artist != o.artist
            || self.playing != o.playing
            || (self.end - o.end).abs() > 0.5
            || self.art_key != o.art_key
            || self.art.is_some() != o.art.is_some()
            || self.can_next != o.can_next
            || self.can_prev != o.can_prev
            || (self.position() - o.position()).abs() > 2.0
    }
}

fn filetime_now() -> i64 {
    let ft = unsafe { GetSystemTimeAsFileTime() };
    ((ft.dwHighDateTime as i64) << 32) | ft.dwLowDateTime as i64
}

/// WinRT işlemini en fazla `ms` bekler. `.get()` süresiz bekler: cevap vermeyen
/// bir kaynak (ör. başka uygulamanın logosu) iş parçacığını kalıcı kilitlerdi.
pub fn await_op<T: RuntimeType + 'static>(op: Result<windows::Foundation::IAsyncOperation<T>>, ms: u64, what: &str) -> Option<T> {
    let op = match op {
        Ok(op) => op,
        Err(e) => {
            warn!("{what} başlatılamadı: {e}");
            return None;
        }
    };
    let end = Instant::now() + Duration::from_millis(ms);
    loop {
        match op.Status() {
            Ok(windows::Foundation::AsyncStatus::Started) if Instant::now() < end => std::thread::sleep(Duration::from_millis(5)),
            Ok(windows::Foundation::AsyncStatus::Completed) => return op.GetResults().ok(),
            Ok(windows::Foundation::AsyncStatus::Started) => {
                warn!("{what} {ms} ms içinde bitmedi, vazgeçildi");
                let _ = op.Cancel();
                return None;
            }
            _ => {
                warn!("{what} başarısız");
                return None;
            }
        }
    }
}

pub fn read_stream(r: &IRandomAccessStreamReference) -> Option<Vec<u8>> {
    let stream = await_op(r.OpenReadAsync(), 2000, "görsel okuma")?;
    let size = stream.Size().ok()? as u32;
    if size == 0 || size > 8 * 1024 * 1024 {
        return None;
    }
    let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0).ok()?).ok()?;
    let n: u32 = await_op(reader.LoadAsync(size).and_then(|op| op.cast()), 2000, "görsel yükleme")?;
    let mut buf = vec![0u8; n as usize];
    reader.ReadBytes(&mut buf).ok()?;
    Some(buf)
}

/// Gösterilecek oturum: çalan varsa o, yoksa sistemin "geçerli" saydığı.
fn pick(mgr: &Manager) -> Option<Session> {
    let sessions: Vec<Session> = mgr.GetSessions().ok()?.into_iter().collect();
    let status = |s: &Session| s.GetPlaybackInfo().and_then(|i| i.PlaybackStatus()).unwrap_or(Status::Closed);
    if let Some(s) = sessions.iter().find(|s| status(s) == Status::Playing) {
        return Some(s.clone());
    }
    mgr.GetCurrentSession().ok().filter(|s| matches!(status(s), Status::Paused | Status::Playing | Status::Changing))
}

fn snapshot(mgr: &Manager, prev: &Option<Snap>) -> Option<Snap> {
    let s = pick(mgr)?;
    let props = s.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
    let title = props.Title().map(|t| t.to_string()).unwrap_or_default();
    if title.is_empty() {
        return None;
    }
    let artist = props.Artist().map(|t| t.to_string()).unwrap_or_default();
    let info = s.GetPlaybackInfo().ok()?;
    let playing = info.PlaybackStatus().ok() == Some(Status::Playing);
    let controls = info.Controls().ok();
    let flag = |f: &dyn Fn(&windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackControls) -> Result<bool>| {
        controls.as_ref().and_then(|c| f(c).ok()).unwrap_or(false)
    };
    let (mut pos, mut end) = (0.0, 0.0);
    if let Ok(tl) = s.GetTimelineProperties() {
        let start = tl.StartTime().map(|t| t.Duration).unwrap_or(0);
        end = (tl.EndTime().map(|t| t.Duration).unwrap_or(0) - start) as f64 / 1e7;
        pos = (tl.Position().map(|t| t.Duration).unwrap_or(0) - start) as f64 / 1e7;
        // Konum son güncelleme anına göredir; çalıyorsa aradan geçen süreyi ekle.
        if playing {
            if let Ok(t) = tl.LastUpdatedTime() {
                let ago = (filetime_now() - t.UniversalTime) as f64 / 1e7;
                if (0.0..3600.0).contains(&ago) {
                    pos += ago;
                }
            }
        }
    }
    let thumb = props.Thumbnail().ok();
    let app = s.SourceAppUserModelId().map(|t| t.to_string()).unwrap_or_default();
    let art_key = format!("{app}|{title}|{artist}|{}", thumb.is_some());
    // Kapak bazen başlıktan sonra hazır olur: yoksa bir sonraki kontrolde yeniden dene.
    let art = match prev {
        Some(p) if p.art_key == art_key && p.art.is_some() => p.art.clone(),
        _ => thumb.as_ref().and_then(read_stream).map(Arc::new),
    };
    Some(Snap {
        app,
        title,
        artist,
        playing,
        pos: pos.max(0.0),
        end: end.max(0.0),
        at: Some(Instant::now()),
        can_prev: flag(&|c| c.IsPreviousEnabled()),
        can_next: flag(&|c| c.IsNextEnabled()),
        can_seek: flag(&|c| c.IsPlaybackPositionEnabled()) && end > 0.0,
        art,
        art_key,
    })
}

/// İzleyiciyi başlatır. Her anlamlı değişiklikte `shared` güncellenir ve
/// `notify` penceresine `msg` gönderilir.
pub fn start(notify: isize, msg: u32, shared: Arc<Mutex<Option<Snap>>>) {
    std::thread::Builder::new()
        .name("media".into())
        .spawn(move || {
            let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
            let mgr = loop {
                match Manager::RequestAsync().and_then(|op| op.get()) {
                    Ok(m) => break m,
                    Err(e) => {
                        warn!("medya oturumları alınamadı: {e}; 5 sn sonra yeniden denenecek");
                        std::thread::sleep(Duration::from_secs(5));
                    }
                }
            };
            let (tx, rx) = mpsc::channel::<()>();
            let t1 = tx.clone();
            let _ = mgr.SessionsChanged(&TypedEventHandler::new(move |_, _| {
                let _ = t1.send(());
                Ok(())
            }));
            let t2 = tx.clone();
            let _ = mgr.CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
                let _ = t2.send(());
                Ok(())
            }));
            let mut subscribed: Vec<Session> = Vec::new();
            let mut last: Option<Snap> = None;
            info!("medya izleyicisi başladı");
            loop {
                // Yeni oturumların olaylarına abone ol (tekrar abone olma).
                let current: Vec<Session> = mgr.GetSessions().map(|v| v.into_iter().collect()).unwrap_or_default();
                for s in &current {
                    if subscribed.contains(s) {
                        continue;
                    }
                    let (a, b, c) = (tx.clone(), tx.clone(), tx.clone());
                    let _ = s.MediaPropertiesChanged(&TypedEventHandler::new(move |_, _| {
                        let _ = a.send(());
                        Ok(())
                    }));
                    let _ = s.PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| {
                        let _ = b.send(());
                        Ok(())
                    }));
                    let _ = s.TimelinePropertiesChanged(&TypedEventHandler::new(move |_, _| {
                        let _ = c.send(());
                        Ok(())
                    }));
                    subscribed.push(s.clone());
                }
                subscribed.retain(|s| current.contains(s));

                let snap = snapshot(&mgr, &last);
                let changed = match (&snap, &last) {
                    (Some(a), Some(b)) => a.differs(b),
                    (None, None) => false,
                    _ => true,
                };
                if changed {
                    if let Ok(mut g) = shared.lock() {
                        *g = snap.clone();
                    }
                    unsafe {
                        let _ = PostMessageW(HWND(notify as _), msg, WPARAM(0), LPARAM(0));
                    }
                }
                last = snap;

                // Olay bekle; art arda gelenleri birleştir. Kapak bazen başlıktan
                // biraz sonra gelir, bu yüzden 2 sn'de bir de kontrol et (yalnızca bir şey çalarken).
                let wait = if last.is_some() { Duration::from_secs(2) } else { Duration::from_secs(3600) };
                match rx.recv_timeout(wait) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
                std::thread::sleep(Duration::from_millis(60));
                while rx.try_recv().is_ok() {}
            }
        })
        .ok();
}

#[derive(Clone, Copy, Debug)]
pub enum Cmd {
    PlayPause,
    Next,
    Prev,
    Seek(f64),
}

/// Komutu, `app` kimlikli oturuma (yoksa geçerli oturuma) gönderir.
pub fn command(app: String, cmd: Cmd) {
    std::thread::spawn(move || {
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        let Ok(mgr) = Manager::RequestAsync().and_then(|op| op.get()) else { return };
        let session = mgr
            .GetSessions()
            .ok()
            .and_then(|v| v.into_iter().find(|s| s.SourceAppUserModelId().map(|i| i.to_string() == app).unwrap_or(false)))
            .or_else(|| mgr.GetCurrentSession().ok());
        let Some(s) = session else { return };
        let r = match cmd {
            Cmd::PlayPause => s.TryTogglePlayPauseAsync().and_then(|op| op.get()),
            Cmd::Next => s.TrySkipNextAsync().and_then(|op| op.get()),
            Cmd::Prev => s.TrySkipPreviousAsync().and_then(|op| op.get()),
            Cmd::Seek(sec) => {
                let start = s.GetTimelineProperties().and_then(|t| t.StartTime()).map(|t| t.Duration).unwrap_or(0);
                s.TryChangePlaybackPositionAsync(start + (sec * 1e7) as i64).and_then(|op| op.get())
            }
        };
        match r {
            Ok(true) => {}
            Ok(false) => warn!("medya komutu reddedildi: {cmd:?} ({app})"),
            Err(e) => warn!("medya komutu başarısız: {cmd:?}: {e}"),
        }
    });
}
