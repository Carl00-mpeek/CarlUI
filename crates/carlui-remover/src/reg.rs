//! Kayıt defterine `HKLM\…` / `HKCU\…` biçimindeki tam yollarla erişim.

use windows::{
    core::{HSTRING, PCWSTR, PWSTR},
    Win32::{Foundation::*, System::Registry::*},
};

fn split(path: &str) -> Option<(HKEY, &str)> {
    let (root, rest) = path.split_once('\\')?;
    let hive = match root.to_ascii_uppercase().as_str() {
        "HKLM" => HKEY_LOCAL_MACHINE,
        "HKCU" => HKEY_CURRENT_USER,
        _ => return None,
    };
    Some((hive, rest))
}

pub struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

impl Key {
    pub fn open(path: &str) -> Option<Key> {
        let (hive, sub) = split(path)?;
        let mut h = HKEY::default();
        let r = unsafe { RegOpenKeyExW(hive, &HSTRING::from(sub), 0, KEY_READ | KEY_WOW64_64KEY, &mut h) };
        (r == ERROR_SUCCESS).then_some(Key(h))
    }

    /// Metin değeri; REG_EXPAND_SZ açılmış olarak gelir.
    pub fn string(&self, name: &str) -> Option<String> {
        let name = HSTRING::from(name);
        let mut size = 0u32;
        unsafe {
            if RegGetValueW(self.0, None, PCWSTR(name.as_ptr()), RRF_RT_REG_SZ, None, None, Some(&mut size)) != ERROR_SUCCESS {
                return None;
            }
            let mut buf = vec![0u16; size as usize / 2 + 1];
            if RegGetValueW(self.0, None, PCWSTR(name.as_ptr()), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as _), Some(&mut size)) != ERROR_SUCCESS {
                return None;
            }
            let n = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
            let s = String::from_utf16_lossy(&buf[..n]);
            let s = s.trim();
            (!s.is_empty()).then(|| s.to_string())
        }
    }

    pub fn dword(&self, name: &str) -> Option<u32> {
        let name = HSTRING::from(name);
        let (mut v, mut size) = (0u32, 4u32);
        let r = unsafe { RegGetValueW(self.0, None, PCWSTR(name.as_ptr()), RRF_RT_REG_DWORD, None, Some(&mut v as *mut u32 as _), Some(&mut size)) };
        (r == ERROR_SUCCESS).then_some(v)
    }

    /// Anahtarın son yazılma zamanı (FILETIME, 1601'den beri 100 ns).
    pub fn written(&self) -> Option<u64> {
        let mut t = FILETIME::default();
        let r = unsafe { RegQueryInfoKeyW(self.0, PWSTR::null(), None, None, None, None, None, None, None, None, None, Some(&mut t)) };
        (r == ERROR_SUCCESS).then_some((t.dwHighDateTime as u64) << 32 | t.dwLowDateTime as u64)
    }

    pub fn subkeys(&self) -> Vec<String> {
        let mut out = Vec::new();
        for i in 0.. {
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            if unsafe { RegEnumKeyExW(self.0, i, PWSTR(buf.as_mut_ptr()), &mut len, None, PWSTR::null(), None, None) } != ERROR_SUCCESS {
                break;
            }
            out.push(String::from_utf16_lossy(&buf[..len as usize]));
        }
        out
    }
}

pub fn exists(path: &str) -> bool {
    Key::open(path).is_some()
}

/// Anahtarı alt anahtarlarıyla siler.
pub fn delete_tree(path: &str) -> bool {
    let Some((hive, sub)) = split(path) else { return false };
    unsafe { RegDeleteTreeW(hive, &HSTRING::from(sub)) == ERROR_SUCCESS }
}
