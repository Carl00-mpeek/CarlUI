//! Kaydın küçük hapı: alanın altında (yer yoksa üstünde) durur, odak almaz.
//! Kayıtta süre, dosya boyutu ve Durdur; bitince Aç, Klasörde göster, GIF yap.

use super::{Area, Phase, Shared};
use carlui_ui::{
    paint::{Align, Painter, GREEN, ORANGE, R, RED, TEXT},
    view::{self, Ctx, Opts, View},
};
use std::{
    os::windows::process::CommandExt,
    process::Command,
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        UI::{HiDpi::*, Shell::ShellExecuteW, WindowsAndMessaging::*},
    },
};

const W: f32 = 470.0;
const H: f32 = 52.0;
/// Bitince hap bu kadar sonra kendiliğinden kapanır.
const LINGER: Duration = Duration::from_secs(30);
const NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Copy, Debug)]
enum A {
    Nothing,
    Stop,
    Open,
    Show,
    Gif,
    Close,
}

struct Pill {
    shared: Arc<Shared>,
    key: String,
    started: Instant,
    /// Kayıt bittiğindeki süre.
    length: Option<Duration>,
    ended: Option<Instant>,
}

fn clock(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

fn size(bytes: u64) -> String {
    if bytes >= 1 << 30 {
        format!("{:.1} GB", bytes as f64 / (1u64 << 30) as f64)
    } else {
        format!("{:.1} MB", bytes as f64 / (1u64 << 20) as f64)
    }
}

impl Pill {
    fn bytes(&self) -> u64 {
        std::fs::metadata(&self.shared.path).map_or(0, |m| m.len())
    }

    fn elapsed(&self) -> Duration {
        self.length.unwrap_or_else(|| self.started.elapsed())
    }

    fn line(&self, p: &mut Painter<A>, x: f32, h: f32, w: f32, s: &str, color: u32) {
        p.text(s, R::new(x, 0.0, w, h), 13.0, false, color, 0.92, Align::Left);
    }
}

impl View for Pill {
    type A = A;

    fn paint(&mut self, p: &mut Painter<A>, w: f32, h: f32) {
        p.hit(R::new(0.0, 0.0, w, h), A::Nothing);
        let y = (h - 32.0) / 2.0;
        let x = 16.0;
        match self.shared.phase() {
            Phase::Recording => {
                // Yanıp sönen nokta kaydın sürdüğünü gösterir.
                let on = self.started.elapsed().as_millis() % 1000 < 600;
                p.round(R::new(x, h / 2.0 - 5.0, 10.0, 10.0), 5.0, RED, if on { 1.0 } else { 0.35 });
                p.text(&clock(self.elapsed()), R::new(x + 18.0, 0.0, 70.0, h), 14.0, true, TEXT, 0.95, Align::Left);
                let mut info = size(self.bytes());
                if self.shared.silent.load(Ordering::SeqCst) {
                    info = format!("{info} · {}", t!("ses yok", "no sound"));
                }
                p.text(&info, R::new(x + 88.0, 0.0, 130.0, h), 12.5, false, TEXT, 0.65, Align::Left);
                let label = if self.key.trim().is_empty() { t!("Durdur", "Stop").to_string() } else { format!("{} · {}", t!("Durdur", "Stop"), self.key) };
                p.button(R::new(w - 12.0 - 200.0, y, 200.0, 32.0), Some('\u{E71A}'), &label, A::Stop, true, true);
            }
            Phase::Saving => self.line(p, x, h, w - 32.0, t!("Kaydediliyor…", "Saving…"), TEXT),
            Phase::Done => {
                let s = format!("{} · {}", clock(self.elapsed()), size(self.bytes()));
                p.glyph('\u{E73E}', R::new(x, 0.0, 18.0, h), 13.0, GREEN, 1.0);
                self.line(p, x + 22.0, h, 130.0, &s, TEXT);
                let mut r = w - 12.0 - 32.0;
                p.button(R::new(r, y, 32.0, 32.0), Some('\u{E711}'), "", A::Close, true, false);
                for (bw, g, label, a) in [(76.0, '\u{E8FE}', t!("GIF", "GIF"), A::Gif), (88.0, '\u{E838}', t!("Klasör", "Folder"), A::Show), (68.0, '\u{E768}', t!("Aç", "Open"), A::Open)] {
                    r -= bw + 6.0;
                    p.button(R::new(r, y, bw, 32.0), Some(g), label, a, true, false);
                }
            }
            Phase::Failed(e) => {
                let s = format!("{} {e}", t!("Kayıt yapılamadı:", "Couldn't record:"));
                self.line(p, x, h, w - 70.0, &s, ORANGE);
                p.button(R::new(w - 44.0, y, 32.0, 32.0), Some('\u{E711}'), "", A::Close, true, false);
            }
        }
    }

    fn click(&mut self, a: A, ctx: &mut Ctx) {
        let path = self.shared.path.clone();
        match a {
            A::Nothing => {}
            A::Stop => self.shared.stop(),
            A::Open => unsafe {
                let file = HSTRING::from(path.as_os_str());
                ShellExecuteW(None, w!("open"), &file, None, None, SW_SHOWNORMAL);
                ctx.close = true;
            },
            A::Show => {
                let _ = Command::new("explorer.exe").raw_arg(format!("/select,\"{}\"", path.display())).creation_flags(NO_WINDOW).spawn();
                ctx.close = true;
            }
            A::Gif => {
                let exe = carlui_core::paths::sibling_exe("carlui-convert.exe");
                if let Err(e) = Command::new(exe).arg(&path).args(["--to", "gif"]).spawn() {
                    warn!("GIF dönüştürücü açılamadı: {e}");
                }
                ctx.close = true;
            }
            A::Close => ctx.close = true,
        }
    }

    fn tick(&mut self, ctx: &mut Ctx) {
        let phase = self.shared.phase();
        if phase != Phase::Recording && self.length.is_none() {
            self.length = Some(self.started.elapsed());
        }
        if matches!(phase, Phase::Done | Phase::Failed(_)) {
            let at = *self.ended.get_or_insert_with(Instant::now);
            if at.elapsed() > LINGER {
                ctx.close = true;
            }
        }
    }

    fn opened(&mut self, hwnd: HWND) {
        unsafe {
            let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
        }
        self.shared.pill.store(hwnd.0 as isize, Ordering::SeqCst);
    }
}

/// Hapın yeri: alanın altında ortalı; yer yoksa üstünde, o da yoksa alanın içinde.
unsafe fn spot(a: Area) -> (i32, i32) {
    let bottom = a.y + a.h as i32;
    let rect = RECT { left: a.x, top: a.y, right: a.x + a.w as i32, bottom };
    let m = MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST);
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    let _ = GetMonitorInfoW(m, &mut mi);
    let (mut dx, mut dy) = (96u32, 96u32);
    let _ = GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
    let s = dx.max(96) as f32 / 96.0;
    let (pw, ph) = ((W * s) as i32, (H * s) as i32);
    place(a, mi.rcWork, pw, ph)
}

fn place(a: Area, work: RECT, pw: i32, ph: i32) -> (i32, i32) {
    let gap = 10;
    let bottom = a.y + a.h as i32;
    let x = (a.x + a.w as i32 / 2 - pw / 2).clamp(work.left + 8, (work.right - pw - 8).max(work.left + 8));
    let y = if bottom + gap + ph <= work.bottom - 8 {
        bottom + gap
    } else if a.y - gap - ph >= work.top + 8 {
        a.y - gap - ph
    } else {
        (bottom - ph - gap).clamp(work.top + 8, (work.bottom - ph - 8).max(work.top + 8))
    };
    (x, y)
}

pub fn run(area: Area, key: String, shared: Arc<Shared>) -> Result<()> {
    let at = unsafe { spot(area) };
    let v = Pill { shared, key, started: Instant::now(), length: None, ended: None };
    view::run(v, Opts { title: t!("Alan kaydı", "Area recording").into(), w: W, h: H, at: Some(at), popup: true, noactivate: true, timer_ms: 300, ..Default::default() })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: RECT = RECT { left: 0, top: 0, right: 1920, bottom: 1040 };

    #[test]
    fn pill_goes_below_then_above_then_inside() {
        assert_eq!(place(Area { x: 500, y: 100, w: 400, h: 300 }, WORK, 400, 50), (500, 410));
        assert_eq!(place(Area { x: 500, y: 700, w: 400, h: 330 }, WORK, 400, 50), (500, 640));
        let (_, y) = place(Area { x: 0, y: 0, w: 1920, h: 1040 }, WORK, 400, 50);
        assert_eq!(y, 980);
    }

    #[test]
    fn pill_stays_on_the_screen() {
        assert_eq!(place(Area { x: 1800, y: 100, w: 100, h: 100 }, WORK, 400, 50).0, 1512);
    }

    #[test]
    fn clock_and_size_read_well() {
        assert_eq!(clock(Duration::from_secs(65)), "01:05");
        assert_eq!(clock(Duration::from_secs(3725)), "1:02:05");
        assert_eq!(size(5 << 20), "5.0 MB");
    }
}
