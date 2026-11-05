//! FFmpeg: video, ses ve WIC'in yazamadığı resim biçimleri (WebP, AVIF,
//! JPEG XL) için. Bilgisayarda bulunan bir ffmpeg.exe kullanılır; yoksa
//! kullanıcı isterse resmi derlemesi (Gyan, GPL; sabit sürüm ve SHA-256) `%LOCALAPPDATA%\CarlUI\ffmpeg`
//! altına indirilir. İlerleme stderr'deki "Duration:" ve "time=" satırlarından
//! okunur; iptal edilince süreç sonlandırılır ve yarım dosya silinir.

use crate::kinds::Kind;
use std::{
    io::Read,
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

const NO_WINDOW: u32 = 0x0800_0000;
/// Sabit sürüm (Gyan, GPL tam derleme: libjxl ve libaom dahil). Güncellenirken
/// adres, boyut ve SHA-256 birlikte değiştirilir.
pub const DOWNLOAD_URL: &str = "https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-full_build.zip";
pub const DOWNLOAD_SIZE: u64 = 257_736_868;
pub const DOWNLOAD_SHA256: &str = "759d0a9831c436a0eb331ad36f236c06cb04aaa0005da46571f0e9d3d9206f6b";

fn own_dir() -> PathBuf {
    PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default()).join("CarlUI").join("ffmpeg")
}

/// Bilinen yerlerde ffmpeg.exe arar.
pub fn find() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf()));
    let mut c: Vec<PathBuf> = Vec::new();
    if let Some(d) = exe_dir {
        c.push(d.join("ffmpeg.exe"));
    }
    c.push(own_dir().join("ffmpeg.exe"));
    if let Ok(path) = std::env::var("PATH") {
        c.extend(std::env::split_paths(&path).map(|p| p.join("ffmpeg.exe")));
    }
    let local = PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default());
    c.push(local.join("Microsoft").join("WinGet").join("Links").join("ffmpeg.exe"));
    for pf in [std::env::var("ProgramFiles").unwrap_or_default(), std::env::var("ProgramFiles(x86)").unwrap_or_default()] {
        if !pf.is_empty() {
            c.push(PathBuf::from(&pf).join("Studio Editors").join("Video Studio").join("ffmpeg.exe"));
            c.push(PathBuf::from(&pf).join("ffmpeg").join("bin").join("ffmpeg.exe"));
        }
    }
    c.push(PathBuf::from(r"C:\ffmpeg\bin\ffmpeg.exe"));
    c.into_iter().find(|p| p.is_file())
}

/// Resmi derlemeyi indirip açar: sabit sürüm, boyut ve SHA-256 doğrulanır
/// (`carlui_ai::net`), arşiv güvenlik denetiminden sonra açılır.
pub fn download(cancel: &AtomicBool) -> Result<PathBuf, String> {
    let dir = own_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let zip = dir.join("ffmpeg.zip");
    carlui_ai::net::fetch(DOWNLOAD_URL, &zip, DOWNLOAD_SIZE, DOWNLOAD_SHA256, cancel, &|_| {})?;
    let x = dir.join("x");
    let _ = std::fs::remove_dir_all(&x);
    let _ = std::fs::create_dir_all(&x);
    carlui_ai::net::unzip(&zip, &x, None)?;
    // ffmpeg-9.0.2-full_build\bin\{ffmpeg,ffprobe}.exe → dir
    let bin = std::fs::read_dir(&x).ok().into_iter().flatten().flatten().map(|e| e.path().join("bin")).find(|p| p.join("ffmpeg.exe").is_file()).ok_or(t!("Arşivde ffmpeg.exe yok", "ffmpeg.exe not found in the archive"))?;
    for name in ["ffmpeg.exe", "ffprobe.exe"] {
        let _ = std::fs::remove_file(dir.join(name));
        let _ = std::fs::rename(bin.join(name), dir.join(name));
    }
    let _ = std::fs::remove_dir_all(&x);
    let exe = dir.join("ffmpeg.exe");
    exe.is_file().then_some(exe).ok_or_else(|| t!("ffmpeg.exe çıkarılamadı", "couldn't extract ffmpeg.exe").into())
}

/// Hedef biçim için kodlayıcı ayarları (CarlHub'daki ayarlarla aynı).
pub fn args_for(kind: Kind, ext: &str) -> Vec<String> {
    let even = r#"scale=trunc(iw/2)*2:trunc(ih/2)*2"#;
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    match (kind, ext) {
        (Kind::Image, "webp") => s(&["-frames:v", "1", "-c:v", "libwebp", "-quality", "90"]),
        // AVIF saydamlığı ayrı bir alfa akışı olarak taşır.
        (Kind::Image, "avif") => s(&["-filter_complex", "[0:v]scale=trunc(iw/2)*2:trunc(ih/2)*2,format=yuva444p,split[c][a];[a]alphaextract[a]", "-map", "[c]", "-map", "[a]", "-frames:v", "1", "-c:v", "libaom-av1", "-still-picture", "1", "-crf", "28", "-cpu-used", "6"]),
        (Kind::Image, "gif") => s(&["-frames:v", "1", "-vf", "split[a][b];[a]palettegen=reserve_transparent=1[p];[b][p]paletteuse=alpha_threshold=128"]),
        (Kind::Image, "jxl") => s(&["-frames:v", "1", "-c:v", "libjxl", "-distance", "1"]),
        (Kind::Image, _) => s(&["-frames:v", "1"]),
        (_, "mp3") => s(&["-vn", "-c:a", "libmp3lame", "-q:a", "2"]),
        (_, "m4a") => s(&["-vn", "-c:a", "aac", "-b:a", "192k"]),
        (_, "wav") => s(&["-vn", "-c:a", "pcm_s16le"]),
        (_, "flac") => s(&["-vn", "-c:a", "flac"]),
        (_, "ogg") => s(&["-vn", "-c:a", "libvorbis", "-q:a", "5"]),
        (_, "opus") => s(&["-vn", "-c:a", "libopus", "-b:a", "128k"]),
        (_, "gif") => s(&["-vf", "fps=15,scale='min(720,iw)':-2:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse", "-loop", "0", "-an"]),
        (_, "webm") => s(&["-c:v", "libvpx-vp9", "-crf", "32", "-b:v", "0", "-row-mt", "1", "-c:a", "libopus", "-b:a", "128k"]),
        (_, "avi") => s(&["-c:v", "mpeg4", "-q:v", "3", "-c:a", "libmp3lame", "-q:a", "2"]),
        (_, "mov") | (_, "mp4") => s(&["-vf", even, "-c:v", "libx264", "-preset", "medium", "-crf", "20", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"]),
        _ => s(&["-vf", even, "-c:v", "libx264", "-preset", "medium", "-crf", "20", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "192k"]),
    }
}

fn seconds(s: &str) -> Option<f64> {
    let mut it = s.trim().split(':');
    let (h, m, sec) = (it.next()?.parse::<f64>().ok()?, it.next()?.parse::<f64>().ok()?, it.next()?.parse::<f64>().ok()?);
    Some(h * 3600.0 + m * 60.0 + sec)
}

/// `ffmpeg -i src <args> dst`. `progress` 0..1 ile çağrılır.
pub fn run(exe: &PathBuf, src: &str, args: &[String], dst: &str, cancel: &Arc<AtomicBool>, progress: &dyn Fn(f32)) -> Result<(), String> {
    let mut child = Command::new(exe)
        .args(["-hide_banner", "-nostdin", "-y", "-stats", "-i"])
        .arg(src)
        .args(args)
        .arg(dst)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .creation_flags(NO_WINDOW)
        .spawn()
        .map_err(|e| format!("{}: {e}", t!("FFmpeg başlatılamadı", "Couldn't start FFmpeg")))?;
    let mut err = child.stderr.take().unwrap();
    let mut duration = None;
    let mut buf = [0u8; 4096];
    let mut line = Vec::new();
    let mut tail: Vec<String> = Vec::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(dst);
            return Err("Durduruldu".into());
        }
        let n = match err.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        for &b in &buf[..n] {
            if b != b'\r' && b != b'\n' {
                line.push(b);
                continue;
            }
            let l = String::from_utf8_lossy(&line).to_string();
            line.clear();
            if l.trim().is_empty() {
                continue;
            }
            if duration.is_none() {
                if let Some(i) = l.find("Duration: ") {
                    duration = seconds(l[i + 10..].split(',').next().unwrap_or_default()).filter(|d| *d > 0.0);
                }
            }
            if let (Some(d), Some(i)) = (duration, l.find("time=")) {
                if let Some(t) = seconds(l[i + 5..].split_whitespace().next().unwrap_or_default()) {
                    progress((t / d).clamp(0.0, 1.0) as f32);
                }
            }
            tail.push(l);
            if tail.len() > 6 {
                tail.remove(0);
            }
        }
    }
    let ok = child.wait().map(|s| s.success()).unwrap_or(false);
    if ok && std::path::Path::new(dst).is_file() {
        progress(1.0);
        Ok(())
    } else {
        let _ = std::fs::remove_file(dst);
        let last = tail.iter().rev().find(|l| !l.starts_with("frame=") && !l.starts_with("size=")).cloned().unwrap_or_default();
        Err(if last.is_empty() { t!("FFmpeg hata verdi", "FFmpeg reported an error").into() } else { last })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn download_is_pinned() {
        assert!(carlui_ai::net::allowed(super::DOWNLOAD_URL));
        assert!(carlui_ai::net::valid_sha(super::DOWNLOAD_SHA256));
        assert!(!super::DOWNLOAD_URL.contains("latest"));
    }

    /// Elle çalıştırılır (ağ gerekir): sabit sürümün boyutu değişmemiş mi?
    #[test]
    #[ignore]
    fn link_unchanged() {
        assert_eq!(carlui_ai::net::probe(super::DOWNLOAD_URL).0, Some(super::DOWNLOAD_SIZE));
    }
}
