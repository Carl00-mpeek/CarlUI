//! Bu modül "kopyalanan şey hangi biçimdeyse yakalansın" sorununu çözüyor.
//! Uygulamalar resmi farklı biçimlerde koyar: Chrome/WhatsApp çoğu zaman
//! yalnızca "PNG" ya da dosya (CF_HDROP), Paint klasik bitmap (CF_DIB), bazı
//! web uygulamaları HTML içinde base64 resim. Sırayla hepsine bakılır:
//! PNG → DIBv5/DIB → tek resim dosyası → metin → dosyalar → HTML'deki resim.
//!
//! Şifre yöneticileri gibi uygulamaların "geçmişe alma" işaretine uyulur.
//! Pano başka bir uygulamada kilitliyse birkaç kez kısa aralıkla yeniden denenir.

use crate::image;
use carlui_core::{info, warn};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{
            DataExchange::*,
            Memory::{GlobalLock, GlobalSize, GlobalUnlock},
        },
        UI::Shell::{DragQueryFileW, HDROP},
    },
};

const CF_UNICODETEXT: u32 = 13;
const CF_DIB: u32 = 8;
const CF_DIBV5: u32 = 17;
const CF_HDROP: u32 = 15;
/// Ayarlar: resimler alınsın mı, en büyük resim (bayt), hariç tutulan uygulamalar.
static IMAGES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
static MAX_IMAGE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(25 * 1024 * 1024);
static EXCLUDED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Pano ayarlarını uygular (ayar değişince yeniden çağrılır).
pub fn set_options(images: bool, max_image_mb: u32, excluded: &[String]) {
    use std::sync::atomic::Ordering;
    IMAGES.store(images, Ordering::Relaxed);
    MAX_IMAGE.store(max_image_mb.clamp(1, 200) as usize * 1024 * 1024, Ordering::Relaxed);
    if let Ok(mut e) = EXCLUDED.lock() {
        *e = excluded.to_vec();
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    Text(String),
    /// PNG baytları + boyut.
    Image { png: Vec<u8>, w: u32, h: u32 },
    Files(Vec<String>),
    /// Carl Vault belirteci (değer değil); geçmişe yazılmaz.
    Vault(carlui_core::vaultlink::Offer),
}

fn reg(name: &str) -> u32 {
    unsafe { RegisterClipboardFormatW(&HSTRING::from(name)) }
}

fn format_name(f: u32) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetClipboardFormatNameW(f, &mut buf) };
    if n > 0 {
        String::from_utf16_lossy(&buf[..n as usize])
    } else {
        format!("#{f}")
    }
}

/// Panodaki biçimin ham baytları.
unsafe fn bytes_of(f: u32) -> Option<Vec<u8>> {
    let h = GetClipboardData(f).ok()?;
    let g = HGLOBAL(h.0);
    let size = GlobalSize(g);
    let p = GlobalLock(g) as *const u8;
    if p.is_null() || size == 0 {
        return None;
    }
    let v = std::slice::from_raw_parts(p, size).to_vec();
    let _ = GlobalUnlock(g);
    Some(v)
}

unsafe fn text() -> Option<String> {
    let b = bytes_of(CF_UNICODETEXT)?;
    let units: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&u| u != 0).collect();
    let s = String::from_utf16_lossy(&units);
    (!s.trim().is_empty()).then_some(s)
}

/// Panodaki düz metin (yapıştırma alanları ve metin genişletici için).
pub fn plain_text() -> Option<String> {
    unsafe {
        for _ in 0..8 {
            if OpenClipboard(HWND::default()).is_ok() {
                let r = text();
                let _ = CloseClipboard();
                return r;
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        None
    }
}

unsafe fn files() -> Vec<String> {
    let Ok(h) = GetClipboardData(CF_HDROP) else { return Vec::new() };
    let drop = HDROP(h.0);
    let n = DragQueryFileW(drop, u32::MAX, None);
    (0..n)
        .filter_map(|i| {
            let len = DragQueryFileW(drop, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            DragQueryFileW(drop, i, Some(&mut buf));
            let s = String::from_utf16_lossy(&buf[..len]);
            (!s.is_empty()).then_some(s)
        })
        .collect()
}

fn is_image_path(p: &str) -> bool {
    let l = p.to_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".bmp", ".webp", ".jfif", ".tif", ".tiff", ".heic"].iter().any(|e| l.ends_with(e))
}

/// HTML içindeki ilk resmi bulur: data:image/…;base64 ya da file:/// yolu.
fn html_image(html: &str) -> Option<Vec<u8>> {
    let lower = html.to_lowercase();
    let at = lower.find("<img")?;
    let rest = &html[at..];
    let src_at = rest.to_lowercase().find("src=")? + 4;
    let quote = rest[src_at..].chars().next()?;
    let body = &rest[src_at + 1..];
    let src = if quote == '"' || quote == '\'' { body.split(quote).next()? } else { body.split([' ', '>']).next()? };
    if let Some(data) = src.strip_prefix("data:") {
        let (_, b64) = data.split_once("base64,")?;
        return image::base64_decode(b64);
    }
    if let Some(path) = src.strip_prefix("file:///") {
        return std::fs::read(path.replace("%20", " ").replace('/', "\\")).ok();
    }
    None
}

fn img(wic: &windows::Win32::Graphics::Imaging::IWICImagingFactory, raw: Vec<u8>, from: &str) -> Option<Content> {
    if !IMAGES.load(std::sync::atomic::Ordering::Relaxed) {
        info!("pano: resim kaydı kapalı, {from} resmi atlandı");
        return None;
    }
    if raw.len() > MAX_IMAGE.load(std::sync::atomic::Ordering::Relaxed) {
        warn!("pano: resim çok büyük ({} MB), geçmişe alınmadı", raw.len() / 1024 / 1024);
        return None;
    }
    match image::to_png(wic, &raw) {
        Some((png, w, h)) => Some(Content::Image { png, w, h }),
        None => {
            warn!("pano: {from} biçimindeki resim çözülemedi");
            None
        }
    }
}

/// Panoyu okur. `hwnd`: panoyu açacak pencere. Geçmişe alınmaması istenen
/// içerikte ya da okunamazsa None.
pub fn read(hwnd: HWND) -> Option<Content> {
    unsafe {
        let mut opened = false;
        for _ in 0..8 {
            if OpenClipboard(hwnd).is_ok() {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(40));
        }
        if !opened {
            warn!("pano: başka bir uygulama panoyu bırakmadı, bu kopya atlandı");
            return None;
        }
        let r = read_open();
        let _ = CloseClipboard();
        r
    }
}

unsafe fn read_open() -> Option<Content> {
    let mut formats = Vec::new();
    let mut f = 0;
    loop {
        f = EnumClipboardFormats(f);
        if f == 0 {
            break;
        }
        formats.push(f);
    }
    let has = |f: u32| formats.contains(&f);
    // Carl Vault belirteci: geçmiş işaretlerinden önce tanınır (yalnızca bellekte tutulur).
    let vault = reg(carlui_core::vaultlink::FORMAT);
    if has(vault) {
        let offer = bytes_of(vault).and_then(|b| {
            let n = b.iter().position(|&x| x == 0).unwrap_or(b.len());
            serde_json::from_slice::<carlui_core::vaultlink::Offer>(&b[..n]).ok()
        });
        if offer.is_none() {
            warn!("pano: Carl Vault belirteci okunamadı");
        }
        return offer.map(Content::Vault);
    }
    // Şifre yöneticileri vb.: geçmişe alma.
    let exclude = reg("ExcludeClipboardContentFromMonitorProcessing");
    let no_history = reg("CanIncludeInClipboardHistory");
    if has(exclude) || (has(no_history) && bytes_of(no_history).is_some_and(|b| b.iter().all(|&x| x == 0))) {
        info!("pano: uygulama bu kopyanın geçmişe alınmamasını istedi");
        return None;
    }
    // Kullanıcının hariç tuttuğu uygulamalar (panoyu koyan pencerenin exe'si).
    let skip = EXCLUDED.lock().map(|e| !e.is_empty() && {
        let owner = GetClipboardOwner().unwrap_or_default();
        let exe = if owner.0.is_null() { String::new() } else { carlui_core::win::window_exe(owner) };
        carlui_core::config::exe_listed(&e, &exe)
    });
    if skip.unwrap_or(false) {
        info!("pano: hariç tutulan uygulamanın kopyası geçmişe alınmadı");
        return None;
    }
    let names: Vec<String> = formats.iter().map(|&f| format_name(f)).collect();
    // Tanı için: hangi uygulama hangi biçimleri koyuyor (WhatsApp gibi sorunlarda bakılır).
    info!("pano biçimleri: {}", names.join(", "));

    let wic = image::wic()?;
    let png_fmt = formats.iter().copied().find(|&f| matches!(format_name(f).as_str(), "PNG" | "image/png"));
    if let Some(f) = png_fmt {
        if let Some(c) = bytes_of(f).and_then(|b| img(&wic, b, "PNG")) {
            return Some(c);
        }
    }
    for f in [CF_DIBV5, CF_DIB] {
        if has(f) {
            if let Some(c) = bytes_of(f).and_then(|b| image::dib_to_bmp(&b)).and_then(|b| img(&wic, b, "DIB")) {
                return Some(c);
            }
        }
    }
    let list = if has(CF_HDROP) { files() } else { Vec::new() };
    if list.len() == 1 && is_image_path(&list[0]) {
        if let Some(c) = std::fs::read(&list[0]).ok().and_then(|b| img(&wic, b, "dosya")) {
            return Some(c);
        }
    }
    if has(CF_UNICODETEXT) {
        if let Some(t) = text() {
            return Some(Content::Text(t));
        }
    }
    if !list.is_empty() {
        return Some(Content::Files(list));
    }
    let html_fmt = reg("HTML Format");
    if has(html_fmt) {
        if let Some(c) = bytes_of(html_fmt).and_then(|b| html_image(&String::from_utf8_lossy(&b))).and_then(|b| img(&wic, b, "HTML")) {
            return Some(c);
        }
    }
    info!("pano: desteklenmeyen içerik ({})", names.join(", "));
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn html() {
        let h = r#"Version:0.9<html><body><img alt="x" src="data:image/png;base64,aGVsbG8="></body>"#;
        assert_eq!(super::html_image(h).unwrap(), b"hello");
        assert!(super::is_image_path(r"C:\a\B.JPG"));
    }
}
