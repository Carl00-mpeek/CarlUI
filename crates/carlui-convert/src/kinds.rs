//! Dosya türleri ve her türün dönüştürülebileceği biçimler. Resim yalnızca
//! resme, ses yalnızca sese; video videoya, GIF'e ya da sese (sesi ayıklama)
//! dönüştürülür. Uzantıya göre karar verilir.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Image,
    Video,
    Audio,
}

impl Kind {
    pub fn of(path: &str) -> Option<Kind> {
        let ext = ext(path);
        if IMAGE_IN.contains(&ext.as_str()) {
            Some(Kind::Image)
        } else if VIDEO_IN.contains(&ext.as_str()) {
            Some(Kind::Video)
        } else if AUDIO_IN.contains(&ext.as_str()) {
            Some(Kind::Audio)
        } else {
            None
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Kind::Image => t!("Resim", "Image"),
            Kind::Video => "Video",
            Kind::Audio => t!("Ses", "Audio"),
        }
    }

    /// Bu türün çıkabileceği biçimler (menü sırası).
    pub fn targets(self) -> &'static [Target] {
        match self {
            Kind::Image => IMAGE_OUT,
            Kind::Video => VIDEO_OUT,
            Kind::Audio => AUDIO_OUT,
        }
    }
}

/// Bir çıkış biçimi: uzantı + menüde görünen açıklama.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Target {
    pub ext: &'static str,
    pub label: &'static str,
}

impl Target {
    /// Menüde görünen ad (seçili dilde).
    pub fn name(&self) -> &'static str {
        if carlui_core::lang::is_tr() {
            return self.label;
        }
        match self.label {
            "ICO (simge)" => "ICO (icon)",
            "DDS (doku)" => "DDS (texture)",
            "GIF (hareketli)" => "GIF (animated)",
            "MP3 (yalnızca ses)" => "MP3 (audio only)",
            "M4A (yalnızca ses)" => "M4A (audio only)",
            "WAV (yalnızca ses)" => "WAV (audio only)",
            l => l,
        }
    }
}

const fn t(ext: &'static str, label: &'static str) -> Target {
    Target { ext, label }
}

const IMAGE_IN: &[&str] = &["png", "jpg", "jpeg", "jfif", "webp", "bmp", "tif", "tiff", "gif", "heic", "heif", "avif", "psd", "dds", "tga", "ico", "svg", "jxl", "jxr", "wdp", "exr", "hdr", "qoi", "pcx", "ppm", "pgm"];
const VIDEO_IN: &[&str] = &["mp4", "mkv", "avi", "mov", "webm", "wmv", "flv", "m4v", "mpg", "mpeg", "ts", "mts", "m2ts", "3gp", "ogv"];
const AUDIO_IN: &[&str] = &["mp3", "wav", "flac", "ogg", "oga", "m4a", "aac", "wma", "opus", "aiff", "aif", "amr", "ac3", "mka"];

const IMAGE_OUT: &[Target] = &[
    t("png", "PNG"),
    t("jpg", "JPG"),
    t("webp", "WebP"),
    t("avif", "AVIF"),
    t("bmp", "BMP"),
    t("tiff", "TIFF"),
    t("gif", "GIF"),
    t("ico", "ICO (simge)"),
    t("dds", "DDS (doku)"),
    t("tga", "TGA"),
    t("jxl", "JPEG XL"),
];
const VIDEO_OUT: &[Target] = &[
    t("mp4", "MP4 (H.264)"),
    t("mkv", "MKV"),
    t("webm", "WebM (VP9)"),
    t("mov", "MOV"),
    t("avi", "AVI"),
    t("gif", "GIF (hareketli)"),
    t("mp3", "MP3 (yalnızca ses)"),
    t("m4a", "M4A (yalnızca ses)"),
    t("wav", "WAV (yalnızca ses)"),
];
const AUDIO_OUT: &[Target] = &[t("mp3", "MP3"), t("m4a", "M4A (AAC)"), t("wav", "WAV"), t("flac", "FLAC"), t("ogg", "OGG (Vorbis)"), t("opus", "Opus")];

pub fn ext(path: &str) -> String {
    std::path::Path::new(path).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Aynı biçim mi? (jpg = jpeg = jfif, tif = tiff …)
pub fn same(a: &str, b: &str) -> bool {
    let n = |e: &str| match e {
        "jpeg" | "jfif" => "jpg".to_string(),
        "tif" => "tiff".to_string(),
        "heif" => "heic".to_string(),
        "aif" => "aiff".to_string(),
        e => e.to_string(),
    };
    n(a) == n(b)
}

/// Yanına yazılacak dosya adı; varsa "ad (1).uzantı", "ad (2).uzantı" …
pub fn output_path(src: &str, ext: &str) -> String {
    let p = std::path::Path::new(src);
    let dir = p.parent().map(|d| d.to_path_buf()).unwrap_or_default();
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "dosya".into());
    let mut out = dir.join(format!("{stem}.{ext}"));
    let mut n = 1;
    while out.exists() {
        out = dir.join(format!("{stem} ({n}).{ext}"));
        n += 1;
    }
    out.to_string_lossy().to_string()
}
