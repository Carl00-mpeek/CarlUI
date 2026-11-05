//! Bu modül "bildirim hem adada hem Windows'un sağ alttaki balonunda çıkıyor"
//! sorununu Windows'un uygulama başına bildirim ayarıyla çözüyor: her uygulamanın
//! `Notifications\Settings\<uygulama>` anahtarına `ShowBanner = 0` yazılır
//! (Ayarlar → Bildirimler → uygulama → "Bildirim başlıklarını göster" kapalı).
//! Bildirimler Bildirim Merkezi'ne düşmeye devam eder, dinleyici onları görür.
//!
//! Yalnızca kendi kapattıklarımız `CarlUIBanner = 1` ile işaretlenir; ayar
//! kapatılınca ya da ada kapanınca yalnızca onlar eski hâline döner, kullanıcının
//! kendi kapattığı uygulamalara dokunulmaz. Hiç bildirim göndermemiş bir uygulamanın
//! anahtarı ilk bildirimde oluştuğu için o ilk balon Windows'ta görünebilir.

use carlui_core::{info, warn};
use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{Foundation::*, System::Registry::*},
};

const ROOT: &str = r"Software\Microsoft\Windows\CurrentVersion\Notifications\Settings";
const MARK: &str = "CarlUIBanner";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn read_dword(key: HKEY, name: &str) -> Option<u32> {
    let name = wide(name);
    let mut v = 0u32;
    let mut size = 4u32;
    let r = unsafe { RegGetValueW(key, None, PCWSTR(name.as_ptr()), RRF_RT_REG_DWORD, None, Some(&mut v as *mut u32 as *mut _), Some(&mut size)) };
    (r == ERROR_SUCCESS).then_some(v)
}

fn write_dword(key: HKEY, name: &str, v: u32) -> bool {
    let name = wide(name);
    unsafe { RegSetValueExW(key, PCWSTR(name.as_ptr()), 0, REG_DWORD, Some(&v.to_le_bytes())) == ERROR_SUCCESS }
}

fn delete(key: HKEY, name: &str) {
    let name = wide(name);
    unsafe {
        let _ = RegDeleteValueW(key, PCWSTR(name.as_ptr()));
    }
}

fn subkeys(h: HKEY) -> Vec<Vec<u16>> {
    let mut out = Vec::new();
    for i in 0.. {
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        if unsafe { RegEnumKeyExW(h, i, PWSTR(buf.as_mut_ptr()), &mut len, None, PWSTR::null(), None, None) } != ERROR_SUCCESS {
            break;
        }
        out.push(buf[..=len as usize].to_vec());
    }
    out
}

/// Uygulama kimliği ters bölü içerebilir ({GUID}\WindowsPowerShell\…\powershell.exe):
/// Windows onu iç içe anahtarlara böler. Ayar yalnızca yaprak anahtarda geçerlidir.
fn walk(h: HKEY, path: &str, hide: bool, changed: &mut usize) {
    let children = subkeys(h);
    let leaf = children.is_empty();
    let ours = read_dword(h, MARK) == Some(1);
    if hide && leaf && read_dword(h, "ShowBanner") != Some(0) {
        if write_dword(h, "ShowBanner", 0) && write_dword(h, MARK, 1) {
            *changed += 1;
        } else {
            warn!("bildirim balonu kapatılamadı: {path}");
        }
    } else if ours && (!hide || !leaf) {
        delete(h, "ShowBanner");
        delete(h, MARK);
        *changed += 1;
    }
    for name in children {
        let mut sub = HKEY::default();
        if unsafe { RegOpenKeyExW(h, PCWSTR(name.as_ptr()), 0, KEY_READ | KEY_SET_VALUE, &mut sub) } == ERROR_SUCCESS {
            walk(sub, &format!("{path}\\{}", String::from_utf16_lossy(&name[..name.len() - 1])), hide, changed);
            unsafe {
                let _ = RegCloseKey(sub);
            }
        }
    }
}

/// `hide`: balonları kapat; değilse yalnızca bizim kapattıklarımızı geri aç.
/// Değişiklik yoksa hiçbir şey yazılmaz (kayıt izleyicisi boşuna uyanmasın).
pub fn sync(hide: bool) {
    let root = wide(ROOT);
    let mut h = HKEY::default();
    if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(root.as_ptr()), 0, KEY_READ, &mut h) } != ERROR_SUCCESS {
        return;
    }
    let mut changed = 0;
    for name in subkeys(h) {
        let mut app = HKEY::default();
        if unsafe { RegOpenKeyExW(h, PCWSTR(name.as_ptr()), 0, KEY_READ | KEY_SET_VALUE, &mut app) } == ERROR_SUCCESS {
            walk(app, &String::from_utf16_lossy(&name[..name.len() - 1]), hide, &mut changed);
            unsafe {
                let _ = RegCloseKey(app);
            }
        }
    }
    unsafe {
        let _ = RegCloseKey(h);
    }
    if changed > 0 {
        info!("Windows bildirim balonları: {} anahtarda {}", changed, if hide { "kapatıldı" } else { "geri açıldı" });
    }
}

const SOUND: &str = "NOC_GLOBAL_SETTING_ALLOW_NOTIFICATION_SOUND";
const SOUND_MARK: &str = "CarlUISound";

/// Bildirim sesi ayarı: `mute` ise Windows'un bildirim seslerini kapatır
/// (Ayarlar → Bildirimler → "Bildirimlerin ses çalmasına izin ver"). Kapatmayı
/// biz yaptıysak işaretlenir; `mute` false olunca yalnızca o zaman geri açılır.
pub fn sync_sound(mute: bool) {
    let root = wide(ROOT);
    let mut h = HKEY::default();
    if unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(root.as_ptr()), 0, KEY_READ | KEY_SET_VALUE, &mut h) } != ERROR_SUCCESS {
        return;
    }
    let ours = read_dword(h, SOUND_MARK) == Some(1);
    if mute && read_dword(h, SOUND) != Some(0) {
        if write_dword(h, SOUND, 0) && write_dword(h, SOUND_MARK, 1) {
            info!("Windows bildirim sesleri kapatıldı");
        }
    } else if !mute && ours {
        delete(h, SOUND);
        delete(h, SOUND_MARK);
        info!("Windows bildirim sesleri geri açıldı");
    }
    unsafe {
        let _ = RegCloseKey(h);
    }
}
