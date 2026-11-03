//! Bu modül, CarlUI'ye özel imleç setini (Windows'un 17 imleç rolünün hepsi:
//! ok, metin, meşgul, boyutlandırma, el…) şu yöntemle sağlıyor: seçilen
//! stil/renk/kenar/ışıma ve gereken piksel boyutuna göre .cur (meşgul ve arka
//! plan çalışması için animasyonlu .ani) dosyaları üretilip
//! `%LOCALAPPDATA%\CarlUI\cursors` altında önbelleğe alınır, sonra
//! `SetSystemCursor` ile yalnızca bu oturum için sistem imleçlerinin yerine
//! konur. Kayıt defterine dokunulmaz: `restore` (SPI_SETCURSORS) kullanıcının
//! kendi şemasını geri yükler, CarlUI çökse bile oturum kapanınca her şey
//! eski hâline döner.

mod curfile;
pub mod draw;
pub mod shapes;

use crate::config::{CursorConfig, CursorStyle};
use shapes::Role;
use std::path::PathBuf;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::System::Registry::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Önbellek sürümü: çizim değişirse eski dosyalar yeniden üretilir.
const VERSION: u32 = 4;

pub fn parse_color(s: &str) -> Option<(f32, f32, f32)> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some((((v >> 16) & 0xFF) as f32 / 255.0, ((v >> 8) & 0xFF) as f32 / 255.0, (v & 0xFF) as f32 / 255.0))
}

pub fn look(cfg: &CursorConfig) -> draw::Look {
    let style = cfg.style;
    let fallback = match style {
        CursorStyle::Neon => (1.0, 0.31, 0.85),
        CursorStyle::Ink => (0.11, 0.11, 0.13),
        CursorStyle::Soft => (0.97, 0.97, 0.97),
        CursorStyle::Zen => (0.74, 0.84, 1.0),
    };
    let color = parse_color(cfg.style_color()).unwrap_or(fallback);
    let edge = if cfg.edge == "auto" { None } else { parse_color(&cfg.edge) };
    let glow = cfg.glow && matches!(style, CursorStyle::Neon | CursorStyle::Zen);
    draw::Look { style, color, glow, edge }
}

fn key(l: &draw::Look, size: u32) -> String {
    let c = |v: f32| (v * 255.0).round() as u8;
    format!(
        "{}-{:02x}{:02x}{:02x}{}{}-{size}px-v{VERSION}",
        format!("{:?}", l.style).to_lowercase(),
        c(l.color.0),
        c(l.color.1),
        c(l.color.2),
        if l.glow { "-glow" } else { "" },
        l.edge.map(|e| format!("-e{:02x}{:02x}{:02x}", c(e.0), c(e.1), c(e.2))).unwrap_or_default()
    )
}

/// Tek bir rolün dosya içeriği (.cur ya da .ani).
pub fn encode(r: &draw::Renderer, role: Role, look: &draw::Look, size: u32) -> Option<Vec<u8>> {
    let k = size as f32 / 32.0;
    let (hx, hy) = role.hot();
    let hot = ((hx * k).round() as u16, (hy * k).round() as u16);
    let frames = if role.animated() { shapes::FRAMES } else { 1 };
    let mut images = Vec::new();
    for f in 0..frames {
        images.push(curfile::Image { size, px: r.render(role, f, look).ok()?, hot });
    }
    Some(if role.animated() { curfile::encode_ani(&images, shapes::JIFFIES) } else { curfile::encode(&images) })
}

/// Bu ayarın `size` piksellik imleç klasörü; eksik dosyaları üretir. Eski
/// ayarların klasörleri silinir (önbellek büyümesin).
pub fn files(cfg: &CursorConfig, size: u32) -> Option<PathBuf> {
    let l = look(cfg);
    let root = crate::paths::data_dir().join("cursors");
    let name = key(&l, size);
    let dir = root.join(&name);
    if Role::ALL.iter().all(|r| dir.join(r.file()).exists()) {
        return Some(dir);
    }
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten() {
            if e.file_name() != name.as_str() {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    std::fs::create_dir_all(&dir).ok()?;
    let r = draw::Renderer::new(size).ok()?;
    for role in Role::ALL {
        let data = encode(&r, role, &l, size)?;
        std::fs::write(dir.join(role.file()), data).ok()?;
    }
    Some(dir)
}

fn reg_dword(path: PCWSTR, name: PCWSTR) -> Option<u32> {
    let mut v = 0u32;
    let mut len = 4u32;
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, path, name, RRF_RT_REG_DWORD, None, Some(&mut v as *mut u32 as *mut _), Some(&mut len)) };
    r.is_ok().then_some(v)
}

/// Windows'un imleç için kullandığı piksel boyutu (erişilebilirlik boyutu × ölçek).
pub fn system_size() -> i32 {
    let base = reg_dword(w!("Control Panel\\Cursors"), w!("CursorBaseSize")).unwrap_or(32).clamp(32, 256);
    let dpi = reg_dword(w!("Control Panel\\Desktop\\WindowMetrics"), w!("AppliedDPI")).unwrap_or(96).clamp(96, 480);
    (base * dpi / 96) as i32
}

/// Bu ayarla kullanılacak piksel boyutu.
pub fn size(cfg: &CursorConfig) -> u32 {
    ((system_size() as f32 * cfg.scale.clamp(0.5, 2.0)).round() as u32).max(16)
}

fn set(file: &PathBuf, id: u32, size: i32) -> bool {
    unsafe {
        let name = HSTRING::from(file.as_os_str());
        let Ok(h) = LoadImageW(None, &name, IMAGE_CURSOR, size, size, LR_LOADFROMFILE) else { return false };
        SetSystemCursor(HCURSOR(h.0), SYSTEM_CURSOR_ID(id)).is_ok()
    }
}

/// CarlUI imleç setini uygular; uygulanan rol sayısını döner.
pub fn apply(cfg: &CursorConfig) -> usize {
    let size = size(cfg);
    let Some(dir) = files(cfg, size) else { return 0 };
    Role::ALL.iter().filter(|r| set(&dir.join(r.file()), r.ocr(), size as i32)).count()
}

/// Kullanıcının kendi imleç şemasını geri yükler.
pub fn restore() {
    unsafe {
        let _ = SystemParametersInfoW(SPI_SETCURSORS, 0, None, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
    }
}
