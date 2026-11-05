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
//! `--dictate`: sesle yazma (bkz. `dictate`); ana süreç kısayolla başlatır.
//! `--speak`: seçili metni sesli okur (bkz. `speak`); ana süreç kısayolla başlatır.
//! `--translate <dosya>`: ekran çevirisi (bkz. `translate`); ada başlatır.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
extern crate carlui_core;

mod ai;
mod dictate;
mod license;
mod ffmpeg;
mod image;
mod kinds;
mod progress;
mod select;
mod speak;
mod translate;

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

/// Dosyayı varsayılan programıyla açar.
pub fn open(path: &str) {
    unsafe {
        ShellExecuteW(HWND::default(), w!("open"), &HSTRING::from(path), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
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

/// Menüden seçilen iş.
enum Pick {
    Convert(Kind, Target),
    Ai(ai::Op),
}

const AI_OPS: [ai::Op; 4] = [ai::Op::Background, ai::Op::Upscale, ai::Op::Transcript, ai::Op::Subtitles];
const AI_BASE: usize = 900;

/// Seçim menüsü; seçilen iş döner.
fn choose(owner: HWND, groups: &[(Kind, Vec<String>)], skipped: usize, accent: Option<u32>) -> Option<Pick> {
    let total: usize = groups.iter().map(|g| g.1.len()).sum();
    let mut rows = vec![Row::Label(if total == 1 { format!("{} · {}", t!("Dönüştür", "Convert"), file_name(&groups[0].1[0])) } else { t!(format!("Dönüştür · {total} dosya"), format!("Convert · {total} files")) }, Icon::Glyph('\u{E895}', None))];
    let mut shown = Vec::new();
    for (ki, (k, files)) in groups.iter().enumerate() {
        let k_index = KINDS.iter().position(|x| x == k).unwrap_or(ki);
        rows.push(Row::Header(if groups.len() > 1 || files.len() > 1 { format!("{} ({})", k.title(), files.len()) } else { k.title().to_string() }));
        for (ti, t) in k.targets().iter().enumerate() {
            // Hepsi zaten bu biçimdeyse gösterme.
            if files.iter().all(|f| kinds::same(&kinds::ext(f), t.ext)) {
                continue;
            }
            rows.push(Row::Item { id: k_index * 100 + ti, text: t.name().to_string(), icon: Icon::Glyph(glyph(*k), None) });
        }
        // Bu türe uyan yapay zekâ işleri (ses+video seçiliyse yalnızca bir kez).
        let ops: Vec<usize> = (0..AI_OPS.len()).filter(|&i| AI_OPS[i].kinds().contains(k) && !shown.contains(&i)).collect();
        if !ops.is_empty() {
            rows.push(Row::Header(t!("Yapay zekâ", "AI").into()));
            for i in ops {
                shown.push(i);
                rows.push(Row::Item { id: AI_BASE + i, text: AI_OPS[i].name(), icon: Icon::Glyph(AI_OPS[i].glyph(), None) });
            }
        }
    }
    if skipped > 0 {
        rows.push(Row::Sep);
        rows.push(Row::Label(t!(format!("{skipped} dosya desteklenmiyor"), format!("{skipped} files not supported")), Icon::Glyph('\u{E7BA}', None)));
    }
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    let id = glassmenu::run(owner, rows, accent, (pt.x, pt.y - 8))?;
    if id >= AI_BASE {
        return AI_OPS.get(id - AI_BASE).map(|o| Pick::Ai(*o));
    }
    let k = KINDS[id / 100];
    Some(Pick::Convert(k, k.targets()[id % 100]))
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
    let mut files: Vec<String> = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && !(*i > 0 && (args[i - 1] == "--hwnd" || args[i - 1] == "--to" || args[i - 1] == "--translate" || args[i - 1] == "--fetch"))).map(|(_, a)| a.clone()).collect();
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    if args.iter().any(|a| a == "--dictate") {
        let cfg = carlui_core::config::load();
        let accent = cfg.dock.menu_accent_color(glassmenu::accent);
        let owner = unsafe { CreateWindowExW(WS_EX_TOOLWINDOW, w!("Static"), w!(""), WS_POPUP, 0, 0, 0, 0, None, None, None, None)? };
        return dictate::run(owner, accent, fg.unwrap_or_else(|| unsafe { GetForegroundWindow() }));
    }
    if args.iter().any(|a| a == "--speak") {
        let accent = carlui_core::config::load().dock.menu_accent_color(glassmenu::accent);
        let owner = unsafe { CreateWindowExW(WS_EX_TOOLWINDOW, w!("Static"), w!(""), WS_POPUP, 0, 0, 0, 0, None, None, None, None)? };
        return speak::run(owner, accent, fg.unwrap_or_else(|| unsafe { GetForegroundWindow() }));
    }
    // `--fetch <model>`: Ayarlar → Yapay zekâ'daki "İndir" düğmesi.
    if let Some(m) = args.iter().position(|a| a == "--fetch").and_then(|i| args.get(i + 1)).and_then(|id| carlui_ai::catalog::get(id)) {
        let accent = carlui_core::config::load().dock.menu_accent_color(glassmenu::accent);
        let owner = unsafe { CreateWindowExW(WS_EX_TOOLWINDOW, w!("Static"), w!(""), WS_POPUP, 0, 0, 0, 0, None, None, None, None)? };
        if !license::ask(owner, accent, m, None) {
            return Ok(());
        }
        let shared = progress::Shared::new();
        shared.update(|s| s.title = t!("Hazırlanıyor", "Preparing").into());
        let worker = shared.clone();
        return progress::run(shared, accent.unwrap_or(0x4CC2FF), move || {
            std::thread::spawn(move || {
                if ai::ensure(&worker, m) {
                    worker.update(|s| {
                        s.title = t!(format!("{} hazır", m.name), format!("{} is ready", m.name));
                        s.file = m.license.into();
                        s.frac = 1.0;
                        s.done = true;
                    });
                }
            });
        });
    }
    if let Some(file) = args.iter().position(|a| a == "--translate").and_then(|i| args.get(i + 1)) {
        let cfg = carlui_core::config::load();
        let accent = cfg.dock.menu_accent_color(glassmenu::accent);
        let owner = unsafe { CreateWindowExW(WS_EX_TOOLWINDOW, w!("Static"), w!(""), WS_POPUP, 0, 0, 0, 0, None, None, None, None)? };
        return translate::run(owner, accent, file);
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
        let text = if files.is_empty() { t!("Dönüştürmek için Gezgin'de dosya seç", "Select files in Explorer to convert") } else { t!("Seçili dosyalar dönüştürülemiyor", "The selected files can't be converted") };
        glassmenu::run(owner, vec![Row::Label(text.into(), Icon::Glyph('\u{E946}', None))], accent, cursor());
        return Ok(());
    }
    // `--to webp`: menüsüz (test ve betikler için).
    let to = args.iter().position(|a| a == "--to").and_then(|i| args.get(i + 1)).map(|s| s.to_lowercase());
    let picked = match to.as_deref() {
        Some("nobg") => Some(Pick::Ai(ai::Op::Background)),
        Some("upscale") => Some(Pick::Ai(ai::Op::Upscale)),
        Some("txt") => Some(Pick::Ai(ai::Op::Transcript)),
        Some("srt") => Some(Pick::Ai(ai::Op::Subtitles)),
        Some(ext) => groups.iter().find_map(|(k, _)| k.targets().iter().find(|t| t.ext == ext).map(|t| Pick::Convert(*k, *t))),
        None => choose(owner, &groups, skipped, accent),
    };
    let (kind, target) = match picked {
        None => return Ok(()),
        Some(Pick::Ai(op)) => return run_ai(owner, accent, op, groups.iter().filter(|g| op.kinds().contains(&g.0)).flat_map(|g| g.1.clone()).collect(), args.iter().any(|a| a == "--yes")),
        Some(Pick::Convert(k, t)) => (k, t),
    };
    let list = groups.iter().find(|g| g.0 == kind).map(|g| g.1.clone()).unwrap_or_default();

    let mut ff = ffmpeg::find();
    let mut download = false;
    if ff.is_none() && needs_ffmpeg(kind, &target, &list) {
        if !ask_ffmpeg(owner, accent) {
            return Ok(());
        }
        download = true;
    }
    let shared = progress::Shared::new();
    shared.update(|s| s.title = t!("Hazırlanıyor", "Preparing").into());
    let worker = shared.clone();
    let color = accent.unwrap_or(0x4CC2FF);
    progress::run(shared, color, move || {
        std::thread::spawn(move || {
            // WIC bu iş parçacığında da COM ister.
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            if download && !fetch_ffmpeg(&worker, &mut ff) {
                return;
            }
            convert_all(&worker, kind, target, &list, ff.as_ref());
        });
    })?;
    Ok(())
}

/// Yapay zekâ işi: model/motor eksikse önce sorar, sonra ilerleme kartında çalışır.
fn run_ai(owner: HWND, accent: Option<u32>, op: ai::Op, list: Vec<String>, yes: bool) -> Result<()> {
    if list.is_empty() {
        return Ok(());
    }
    let m = op.model();
    let need = ai::missing_bytes(m);
    if need > 0 && !yes && !ask_model(owner, accent, m, need) {
        return Ok(());
    }
    let mut ff = ffmpeg::find();
    // Ses/videodan ses çıkarmak için (WAV hariç) FFmpeg gerekir.
    let download = ff.is_none() && op.is_speech() && list.iter().any(|f| !ai::readable_without_ffmpeg(f));
    if download && !yes && !ask_ffmpeg(owner, accent) {
        return Ok(());
    }
    let shared = progress::Shared::new();
    shared.update(|s| s.title = t!("Hazırlanıyor", "Preparing").into());
    let worker = shared.clone();
    progress::run(shared, accent.unwrap_or(0x4CC2FF), move || {
        std::thread::spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            if download && !fetch_ffmpeg(&worker, &mut ff) {
                return;
            }
            ai::run_all(&worker, op, &list, ff.as_ref());
        });
    })
}

/// Model (ve motor) eksikken indirilsin mi diye sorar; lisans ve kaynak görünür.
pub fn ask_model(owner: HWND, accent: Option<u32>, m: &carlui_ai::catalog::Model, need: u64) -> bool {
    if !carlui_ai::store::accepted(m) {
        return license::ask(owner, accent, m, Some(need));
    }
    let rows = vec![
        Row::Label(t!(format!("{} modeli kurulu değil", m.name), format!("The {} model isn't installed", m.name)), Icon::Glyph('\u{E946}', None)),
        Row::Label(format!("{} · {}", m.license, m.about()), Icon::Glyph('\u{E946}', None)),
        Row::Item { id: 1, text: t!(format!("İndir ({}, bir kez)", ai::mb(need)), format!("Download ({}, once)", ai::mb(need))), icon: Icon::Glyph('\u{E896}', None) },
        Row::Item { id: 2, text: t!("Vazgeç", "Cancel").into(), icon: Icon::Glyph('\u{E711}', None) },
    ];
    glassmenu::run(owner, rows, accent, cursor()) == Some(1)
}

/// FFmpeg eksikken indirilsin mi diye sorar.
fn ask_ffmpeg(owner: HWND, accent: Option<u32>) -> bool {
    let rows = vec![
        Row::Label(t!("Bu iş için FFmpeg gerekli", "This needs FFmpeg").into(), Icon::Glyph('\u{E946}', None)),
        Row::Item { id: 1, text: t!("FFmpeg'i indir (~110 MB, bir kez)", "Download FFmpeg (~110 MB, once)").into(), icon: Icon::Glyph('\u{E896}', None) },
        Row::Item { id: 2, text: t!("Vazgeç", "Cancel").into(), icon: Icon::Glyph('\u{E711}', None) },
    ];
    glassmenu::run(owner, rows, accent, cursor()) == Some(1)
}

/// FFmpeg'i ilerleme kartında indirir; başarısızsa kartı hata ile kapatır.
fn fetch_ffmpeg(w: &progress::Shared, ff: &mut Option<PathBuf>) -> bool {
    w.update(|s| {
        s.title = t!("FFmpeg indiriliyor", "Downloading FFmpeg").into();
        s.file = "github.com/BtbN/FFmpeg-Builds".into();
    });
    match ffmpeg::download(&w.cancel) {
        Ok(p) => {
            *ff = Some(p);
            true
        }
        Err(e) => {
            w.update(|s| {
                s.title = e;
                s.failed = true;
                s.done = true;
            });
            false
        }
    }
}

pub fn cursor() -> (i32, i32) {
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
            s.title = if n > 1 { format!("{} · {}/{n}", t!("Dönüştürülüyor", "Converting"), i + 1) } else { t!("Dönüştürülüyor", "Converting").into() };
            s.file = format!("{name} → {}", t.name());
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
                None => Err(t!("FFmpeg bulunamadı", "FFmpeg not found").into()),
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
            t!("Durduruldu", "Stopped").into()
        } else if stopped {
            t!(format!("Durduruldu · {ok} dosya dönüştürüldü"), format!("Stopped · {ok} files converted"))
        } else if errors.is_empty() {
            if ok == 1 {
                t!("Dönüştürüldü", "Converted").into()
            } else if ok == 0 && same > 0 {
                t!("Zaten bu biçimde", "Already in this format").into()
            } else {
                t!(format!("{ok} dosya dönüştürüldü"), format!("{ok} files converted"))
            }
        } else if ok > 0 {
            t!(format!("{ok} dosya dönüştürüldü · {} hata", errors.len()), format!("{ok} files converted · {} errors", errors.len()))
        } else {
            t!("Dönüştürülemedi", "Conversion failed").into()
        };
        s.file = match (&errors.first(), &last) {
            (Some(e), _) => e.to_string(),
            (None, Some(l)) => format!("{} · {}", file_name(l), t!("göstermek için tıkla", "click to show")),
            _ => String::new(),
        };
        s.reveal = last.clone();
    });
}
