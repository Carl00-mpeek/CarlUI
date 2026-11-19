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
pub mod swap;

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

/// Sallayınca büyümenin kareleri: imleç boyutunun katları.
pub const GROW: [f32; 6] = [1.4, 1.8, 2.2, 2.6, 3.0, 3.4];

fn grow_sizes(base: u32) -> Vec<u32> {
    let mut v: Vec<u32> = GROW.iter().map(|k| ((base as f32 * k).round() as u32).min(256)).collect();
    v.dedup();
    v
}

/// CarlUI imlecinin büyüme kareleri, önceden üretilip önbellekte: her rol
/// için tek .cur, içinde her karenin boyutu ayrı görüntü. Eski ayarların
/// klasörleri silinir.
pub fn prepare_grow(cfg: &CursorConfig) -> Option<PathBuf> {
    let l = look(cfg);
    let base = size(cfg);
    let root = crate::paths::data_dir().join("cursors-grow");
    let name = key(&l, base);
    let dir = root.join(&name);
    let roles: Vec<Role> = Role::ALL.into_iter().filter(|r| !r.animated()).collect();
    if roles.iter().all(|r| dir.join(r.file()).exists()) {
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
    let mut images: Vec<Vec<curfile::Image>> = roles.iter().map(|_| Vec::new()).collect();
    for s in grow_sizes(base) {
        let r = draw::Renderer::new(s).ok()?;
        let k = s as f32 / 32.0;
        for (i, role) in roles.iter().enumerate() {
            let (hx, hy) = role.hot();
            images[i].push(curfile::Image { size: s, px: r.render(*role, 0, &l).ok()?, hot: ((hx * k).round() as u16, (hy * k).round() as u16) });
        }
    }
    for (role, imgs) in roles.iter().zip(images) {
        std::fs::write(dir.join(role.file()), curfile::encode(&imgs)).ok()?;
    }
    Some(dir)
}

/// Windows şemasındaki rol dosyası (kayıt defterindeki ad).
fn scheme_file(id: u32) -> Option<PathBuf> {
    let name = match id {
        32512 => w!("Arrow"),
        32513 => w!("IBeam"),
        32515 => w!("Crosshair"),
        32516 => w!("UpArrow"),
        32631 => w!("NWPen"),
        32642 => w!("SizeNWSE"),
        32643 => w!("SizeNESW"),
        32644 => w!("SizeWE"),
        32645 => w!("SizeNS"),
        32646 => w!("SizeAll"),
        32648 => w!("No"),
        32649 => w!("Hand"),
        32651 => w!("Help"),
        32671 => w!("Pin"),
        32672 => w!("Person"),
        _ => return None,
    };
    let mut buf = vec![0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    // RRF_RT_REG_SZ ortam değişkenlerini (%SystemRoot%) de açar.
    unsafe { RegGetValueW(HKEY_CURRENT_USER, w!("Control Panel\\Cursors"), name, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut len)).ok().ok()? };
    let s = String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]);
    (!s.trim().is_empty()).then(|| PathBuf::from(s.trim()))
}

/// .cur dosyasındaki en büyük görüntü (piksel); .ani ya da bozuksa `None`.
fn largest_frame(file: &PathBuf) -> Option<u32> {
    let b = std::fs::read(file).ok()?;
    if b.len() < 6 || b[0..4] != [0, 0, 2, 0] {
        return None;
    }
    let n = u16::from_le_bytes([b[4], b[5]]) as usize;
    (0..n).filter_map(|i| b.get(6 + 16 * i).map(|&d| if d == 0 { 256 } else { d as u32 })).max()
}

fn load(file: &PathBuf, s: u32) -> Option<HCURSOR> {
    let name = HSTRING::from(file.as_os_str());
    unsafe { LoadImageW(None, &name, IMAGE_CURSOR, s as i32, s as i32, LR_LOADFROMFILE).ok().map(|h| HCURSOR(h.0)) }
}

/// Görünen `cur` imlecinin (`id` rolü) büyüme kareleri, küçükten büyüğe.
/// Kaynak sırasıyla: CarlUI'nin önceden üretilmiş kareleri, Windows şemasının
/// yeterince büyük görüntüsü olan dosyası, yoksa imlecin kendisi yumuşak
/// büyütülerek. Kareleri çağıran `DestroyCursor` ile bırakır.
pub fn grow_frames(cfg: &CursorConfig, id: u32, cur: HCURSOR) -> Vec<HCURSOR> {
    let base = if cfg.enabled { size(cfg) } else { system_size() as u32 };
    let sizes = grow_sizes(base);
    let top = *sizes.last().unwrap_or(&base);
    let file = if cfg.enabled {
        Role::ALL.into_iter().find(|r| r.ocr() == id && !r.animated()).and_then(|r| prepare_grow(cfg).map(|d| d.join(r.file())))
    } else {
        scheme_file(id).filter(|f| largest_frame(f).is_some_and(|m| m * 10 >= top * 9))
    };
    if let Some(f) = file {
        let v: Vec<HCURSOR> = sizes.iter().filter_map(|&s| load(&f, s)).collect();
        if v.len() == sizes.len() {
            return v;
        }
        for h in v {
            unsafe {
                let _ = DestroyCursor(h);
            }
        }
    }
    let Some(p) = swap::pixels(cur) else { return Vec::new() };
    sizes
        .iter()
        .filter_map(|&s| {
            let k = s as f32 / base as f32;
            swap::make(&swap::scale(&p, (p.w as f32 * k).round() as usize, (p.h as f32 * k).round() as usize))
        })
        .collect()
}
