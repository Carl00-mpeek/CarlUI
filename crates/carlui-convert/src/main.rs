//! Bu modül CarlUI'nin hızlı dönüştürücüsü (`carlui-convert.exe`). Sorun:
//! bir resmi ya da videoyu başka biçime çevirmek için ayrı program açıp dosyayı
//! içine sürüklemek gerekiyor. Çözüm: Gezgin'de (ya da masaüstünde) dosyaları
//! seç, Win+Alt'a bas (ayarlanabilir, `shortcuts.converter`); dock bu akoru
//! yakalar ve bu programı başlatır. İmlecin yanında cam bir menü açılır; her
//! tür yalnızca kendine uygun biçimleri gösterir (resim → resim, ses → ses,
//! video → video/GIF/ses). Seçilen biçimde yeni dosya orijinalin yanına yazılır,
//! orijinale dokunulmaz. İlerleme dock'un üstündeki küçük kartta görünür.
//!
//! Test/elle kullanım: `carlui-convert.exe dosya1 dosya2 …` (seçim yerine).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ffmpeg;
mod image;
mod kinds;
mod progress;
mod select;

use carlui_core::{info, log};
use carlui_ui::glassmenu::{self, Icon, Row};
use kinds::{Kind, Target};
use std::{path::PathBuf, sync::atomic::Ordering};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::Com::*,
        UI::{HiDpi::*, Shell::ShellExecuteW, WindowsAndMessaging::*},
    },
};

pub fn reveal(path: &str) {
    unsafe {
        ShellExecuteW(HWND::default(), w!("open"), w!("explorer.exe"), &HSTRING::from(format!("/select,\"{path}\"")), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

fn file_name(p: &str) -> String {
    std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

const KINDS: [Kind; 3] = [Kind::Image, Kind::Video, Kind::Audio];

fn glyph(k: Kind) -> char {
    match k {
        Kind::Image => '\u{EB9F}',
        Kind::Video => '\u{E714}',
        Kind::Audio => '\u{E8D6}',
    }
}

/// Seçim menüsü; seçilen (tür, hedef) döner.
fn choose(owner: HWND, groups: &[(Kind, Vec<String>)], skipped: usize, accent: Option<u32>) -> Option<(Kind, Target)> {
    let total: usize = groups.iter().map(|g| g.1.len()).sum();
    let mut rows = vec![Row::Label(if total == 1 { format!("Dönüştür · {}", file_name(&groups[0].1[0])) } else { format!("Dönüştür · {total} dosya") }, Icon::Glyph('\u{E895}', None))];
    for (ki, (k, files)) in groups.iter().enumerate() {
        let k_index = KINDS.iter().position(|x| x == k).unwrap_or(ki);
        rows.push(Row::Header(if groups.len() > 1 || files.len() > 1 { format!("{} ({})", k.title(), files.len()) } else { k.title().to_string() }));
        for (ti, t) in k.targets().iter().enumerate() {
            // Hepsi zaten bu biçimdeyse gösterme.
            if files.iter().all(|f| kinds::same(&kinds::ext(f), t.ext)) {
                continue;
            }
            rows.push(Row::Item { id: k_index * 100 + ti, text: t.label.to_string(), icon: Icon::Glyph(glyph(*k), None) });
        }
    }
    if skipped > 0 {
        rows.push(Row::Sep);
        rows.push(Row::Label(format!("{skipped} dosya desteklenmiyor"), Icon::Glyph('\u{E7BA}', None)));
    }
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    let id = glassmenu::run(owner, rows, accent, (pt.x, pt.y - 8))?;
    let k = KINDS[id / 100];
    Some((k, k.targets()[id % 100]))
}

fn needs_ffmpeg(kind: Kind, t: &Target, files: &[String]) -> bool {
    match kind {
        Kind::Image => matches!(t.ext, "webp" | "avif" | "jxl") || files.iter().any(|f| matches!(kinds::ext(f).as_str(), "psd" | "tga" | "svg" | "jxl" | "exr" | "hdr" | "qoi" | "pcx" | "ppm" | "pgm")),
        _ => true,
    }
}

fn main() -> Result<()> {
    log::init("convert");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let fg = args.iter().position(|a| a == "--hwnd").and_then(|i| args.get(i + 1)).and_then(|s| s.parse::<isize>().ok()).map(|h| HWND(h as _));
    let mut files: Vec<String> = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && !(*i > 0 && (args[i - 1] == "--hwnd" || args[i - 1] == "--to"))).map(|(_, a)| a.clone()).collect();
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    if files.is_empty() {
        let fg = fg.unwrap_or_else(|| unsafe { GetForegroundWindow() });
        files = select::selection(fg);
    }
    info!("dönüştürücü: {} dosya", files.len());
    let cfg = carlui_core::config::load();
    let accent = cfg.dock.menu_accent_color(glassmenu::accent);
    // Menü için görünmeyen sahip pencere.
    let owner = unsafe { CreateWindowExW(WS_EX_TOOLWINDOW, w!("Static"), w!(""), WS_POPUP, 0, 0, 0, 0, None, None, None, None)? };

    let mut groups: Vec<(Kind, Vec<String>)> = Vec::new();
    let mut skipped = 0;
    for f in &files {
        match Kind::of(f) {
            Some(k) => match groups.iter_mut().find(|g| g.0 == k) {
                Some(g) => g.1.push(f.clone()),
                None => groups.push((k, vec![f.clone()])),
            },
            None => skipped += 1,
        }
    }
    groups.sort_by_key(|g| KINDS.iter().position(|k| *k == g.0));
    if groups.is_empty() {
        let text = if files.is_empty() { "Dönüştürmek için Gezgin'de dosya seç" } else { "Seçili dosyalar dönüştürülemiyor" };
        glassmenu::run(owner, vec![Row::Label(text.into(), Icon::Glyph('\u{E946}', None))], accent, cursor());
        return Ok(());
    }
    // `--to webp`: menüsüz (test ve betikler için).
    let to = args.iter().position(|a| a == "--to").and_then(|i| args.get(i + 1)).map(|s| s.to_lowercase());
    let picked = match to {
        Some(ext) => groups.iter().find_map(|(k, _)| k.targets().iter().find(|t| t.ext == ext).map(|t| (*k, *t))),
        None => choose(owner, &groups, skipped, accent),
    };
    let Some((kind, target)) = picked else { return Ok(()) };
    let list = groups.iter().find(|g| g.0 == kind).map(|g| g.1.clone()).unwrap_or_default();

    let mut ff = ffmpeg::find();
    let mut download = false;
    if ff.is_none() && needs_ffmpeg(kind, &target, &list) {
        let rows = vec![
            Row::Label("Bu dönüşüm için FFmpeg gerekli".into(), Icon::Glyph('\u{E946}', None)),
            Row::Item { id: 1, text: "FFmpeg'i indir (~110 MB, bir kez)".into(), icon: Icon::Glyph('\u{E896}', None) },
            Row::Item { id: 2, text: "Vazgeç".into(), icon: Icon::Glyph('\u{E711}', None) },
        ];
        if glassmenu::run(owner, rows, accent, cursor()) != Some(1) {
            return Ok(());
        }
        download = true;
    }
    let shared = progress::Shared::new();
    shared.update(|s| s.title = "Hazırlanıyor".into());
    let worker = shared.clone();
    let color = accent.unwrap_or(0x4CC2FF);
    progress::run(shared, color, move || {
        std::thread::spawn(move || {
            // WIC bu iş parçacığında da COM ister.
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            let cancel = worker.cancel.clone();
            if download {
                worker.update(|s| {
                    s.title = "FFmpeg indiriliyor".into();
                    s.file = "github.com/BtbN/FFmpeg-Builds".into();
                });
                match ffmpeg::download(&cancel) {
                    Ok(p) => ff = Some(p),
                    Err(e) => {
                        worker.update(|s| {
                            s.title = e;
                            s.failed = true;
                            s.done = true;
                        });
                        return;
                    }
                }
            }
            convert_all(&worker, kind, target, &list, ff.as_ref());
        });
    })?;
    Ok(())
}

fn cursor() -> (i32, i32) {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    (pt.x, pt.y - 8)
}

fn convert_all(w: &progress::Shared, kind: Kind, t: Target, files: &[String], ff: Option<&PathBuf>) {
    let n = files.len();
    let (mut ok, mut same, mut errors) = (0, 0, Vec::new());
    let mut last = None;
    for (i, src) in files.iter().enumerate() {
        if w.cancel.load(Ordering::SeqCst) {
            break;
        }
        let name = file_name(src);
        w.update(|s| {
            s.title = if n > 1 { format!("Dönüştürülüyor · {}/{n}", i + 1) } else { "Dönüştürülüyor".into() };
            s.file = format!("{name} → {}", t.label);
            s.frac = i as f32 / n as f32;
        });
        if kinds::same(&kinds::ext(src), t.ext) {
            same += 1;
            continue;
        }
        let dst = kinds::output_path(src, t.ext);
        let r = match kind {
            Kind::Image => image::convert(src, &dst, t.ext, ff),
            _ => match ff {
                Some(ff) => ffmpeg::run(ff, src, &ffmpeg::args_for(kind, t.ext), &dst, &w.cancel, &|f| w.update(|s| s.frac = (i as f32 + f) / n as f32)),
                None => Err("FFmpeg bulunamadı".into()),
            },
        };
        match r {
            Ok(()) => {
                ok += 1;
                info!("dönüştürüldü: {src} → {dst}");
                last = Some(dst);
            }
            Err(_) if w.cancel.load(Ordering::SeqCst) => break,
            Err(e) => {
                carlui_core::warn!("dönüştürülemedi: {src}: {e}");
                errors.push(format!("{name}: {e}"));
            }
        }
    }
    let stopped = w.cancel.load(Ordering::SeqCst);
    w.update(|s| {
        s.done = true;
        s.frac = 1.0;
        s.failed = ok == 0 && (!errors.is_empty() || stopped);
        s.title = if stopped && ok == 0 {
            "Durduruldu".into()
        } else if stopped {
            format!("Durduruldu · {ok} dosya dönüştürüldü")
        } else if errors.is_empty() {
            if ok == 1 { "Dönüştürüldü".into() } else if ok == 0 && same > 0 { "Zaten bu biçimde".into() } else { format!("{ok} dosya dönüştürüldü") }
        } else if ok > 0 {
            format!("{ok} dosya dönüştürüldü · {} hata", errors.len())
        } else {
            "Dönüştürülemedi".into()
        };
        s.file = match (&errors.first(), &last) {
            (Some(e), _) => e.to_string(),
            (None, Some(l)) => format!("{} · göstermek için tıkla", file_name(l)),
            _ => String::new(),
        };
        s.reveal = last.clone();
    });
}
