//! Kayıt defteri yardımcıları. Her işlev kök anahtarı ve yolu parametre olarak
//! alır; böylece testler gerçek başlangıç girdilerine hiç dokunmadan geçici bir
//! anahtar (HKCU\Software\CarlUI-Test-…) üzerinde çalışır.

use super::Fail;
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{Registry::*, SystemInformation::GetSystemTimeAsFileTime},
    },
};

pub fn hkey(h: isize) -> HKEY {
    HKEY(h as _)
}

/// Erişim reddi mi (yönetici izniyle yeniden denemeye değer mi).
pub fn denied(e: WIN32_ERROR) -> bool {
    e == ERROR_ACCESS_DENIED || e == ERROR_PRIVILEGE_NOT_HELD
}

fn fail(e: WIN32_ERROR) -> Fail {
    if denied(e) {
        Fail::Denied
    } else {
        Fail::Other(Error::from(e.to_hresult()).message())
    }
}

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn open(root: HKEY, path: &str, access: REG_SAM_FLAGS) -> std::result::Result<Key, WIN32_ERROR> {
    let mut k = HKEY::default();
    let r = unsafe { RegOpenKeyExW(root, &HSTRING::from(path), 0, access, &mut k) };
    if r.is_ok() { Ok(Key(k)) } else { Err(r) }
}

fn create(root: HKEY, path: &str) -> std::result::Result<Key, WIN32_ERROR> {
    let mut k = HKEY::default();
    let r = unsafe { RegCreateKeyExW(root, &HSTRING::from(path), 0, None, REG_OPTION_NON_VOLATILE, KEY_SET_VALUE | KEY_QUERY_VALUE, None, &mut k, None) };
    if r.is_ok() { Ok(Key(k)) } else { Err(r) }
}

/// Anahtarın bütün değerleri: (ad, veri, tür). Değer boyu sınırsız: önce en
/// uzun ad ve veri sorulur (eskiden 8 KB'tan uzun tek bir değer, anahtarın
/// geri kalanını listeden düşürüyordu).
pub fn values(root: HKEY, path: &str) -> Vec<(String, Vec<u8>, u32)> {
    let mut out = Vec::new();
    let Ok(k) = open(root, path, KEY_READ) else { return out };
    unsafe {
        let (mut max_name, mut max_data) = (0u32, 0u32);
        let _ = RegQueryInfoKeyW(k.0, PWSTR::null(), None, None, None, None, None, None, Some(&mut max_name), Some(&mut max_data), None, None);
        let mut i = 0u32;
        let mut tries = 0;
        while tries < 4 {
            let mut name = vec![0u16; max_name as usize + 2];
            let mut nlen = name.len() as u32;
            let mut data = vec![0u8; max_data as usize + 16];
            let mut dlen = data.len() as u32;
            let mut ty = 0u32;
            let r = RegEnumValueW(k.0, i, PWSTR(name.as_mut_ptr()), &mut nlen, None, Some(&mut ty), Some(data.as_mut_ptr()), Some(&mut dlen));
            if r == ERROR_MORE_DATA {
                // Biz okurken değer büyüdü: boyları büyütüp aynı sırayı yeniden dene.
                max_name = max_name.max(nlen).saturating_mul(2).max(256);
                max_data = max_data.max(dlen).saturating_mul(2).max(4096);
                tries += 1;
                continue;
            }
            if r.is_err() {
                break;
            }
            data.truncate(dlen as usize);
            out.push((String::from_utf16_lossy(&name[..nlen as usize]), data, ty));
            i += 1;
            tries = 0;
        }
    }
    out
}

/// Tek değer: (tür, veri). Yoksa `None`.
pub fn value(root: HKEY, path: &str, name: &str) -> Option<(u32, Vec<u8>)> {
    let k = open(root, path, KEY_QUERY_VALUE).ok()?;
    let n = HSTRING::from(name);
    unsafe {
        let mut size = 0u32;
        let mut ty = REG_VALUE_TYPE::default();
        if RegQueryValueExW(k.0, &n, None, Some(&mut ty), None, Some(&mut size)).is_err() {
            return None;
        }
        loop {
            let mut data = vec![0u8; size as usize + 2];
            let mut len = data.len() as u32;
            let r = RegQueryValueExW(k.0, &n, None, Some(&mut ty), Some(data.as_mut_ptr()), Some(&mut len));
            if r == ERROR_MORE_DATA {
                size = len.max(size * 2);
                continue;
            }
            if r.is_err() {
                return None;
            }
            data.truncate(len as usize);
            return Some((ty.0, data));
        }
    }
}

pub fn set_value(root: HKEY, path: &str, name: &str, ty: u32, data: &[u8]) -> std::result::Result<(), Fail> {
    let k = create(root, path).map_err(fail)?;
    let r = unsafe { RegSetValueExW(k.0, &HSTRING::from(name), 0, REG_VALUE_TYPE(ty), Some(data)) };
    if r.is_ok() { Ok(()) } else { Err(fail(r)) }
}

pub fn delete_value(root: HKEY, path: &str, name: &str) -> std::result::Result<(), Fail> {
    let k = match open(root, path, KEY_SET_VALUE) {
        Ok(k) => k,
        Err(e) if e == ERROR_FILE_NOT_FOUND => return Ok(()),
        Err(e) => return Err(fail(e)),
    };
    let r = unsafe { RegDeleteValueW(k.0, &HSTRING::from(name)) };
    if r.is_ok() || r == ERROR_FILE_NOT_FOUND { Ok(()) } else { Err(fail(r)) }
}

pub fn reg_string(data: &[u8]) -> String {
    let w: Vec<u16> = data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
    String::from_utf16_lossy(&w)
}

pub fn dword(data: &[u8]) -> Option<u32> {
    data.get(..4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// StartupApproved kaydı: yoksa etkin; ilk baytın en düşük biti 1 ise devre dışı.
pub fn approved(root: HKEY, base: &str, key: &str, name: &str) -> bool {
    values(root, &format!("{base}{key}")).iter().find(|(n, ..)| n.eq_ignore_ascii_case(name)).map(|(_, d, _)| d.first().map(|b| b & 1 == 0).unwrap_or(true)).unwrap_or(true)
}

/// StartupApproved'a yazar. Var olan değerin adı (büyük/küçük harf) korunur.
pub fn set_approved(root: HKEY, base: &str, key: &str, name: &str, on: bool) -> std::result::Result<(), Fail> {
    let path = format!("{base}{key}");
    let name = values(root, &path).into_iter().map(|(n, ..)| n).find(|n| n.eq_ignore_ascii_case(name)).unwrap_or_else(|| name.to_string());
    // 02 = etkin, 03 = devre dışı; ardından devre dışı bırakılma zamanı (FILETIME).
    // Windows Görev Yöneticisi de aynı biçimi yazar.
    let mut data = [0u8; 12];
    data[0] = if on { 2 } else { 3 };
    if !on {
        let ft = unsafe { GetSystemTimeAsFileTime() };
        data[4..8].copy_from_slice(&ft.dwLowDateTime.to_le_bytes());
        data[8..12].copy_from_slice(&ft.dwHighDateTime.to_le_bytes());
    }
    set_value(root, &path, &name, REG_BINARY.0, &data)
}

/// Değeri `from` anahtarından `to` anahtarına türü ve baytlarıyla birebir taşır.
/// Önce kopyalanır ve kopya doğrulanır; asıl değer ancak ondan sonra silinir,
/// yani yarıda kalırsa girdi kaybolmaz. Hedefte aynı adlı değer zaten varsa:
/// `overwrite` ise üzerine yazılır (kapatırken yedek tazelenir), değilse
/// hedefteki korunur (açarken uygulamanın kendi yeniden eklediği girdi kalır).
pub fn move_value(root: HKEY, from: &str, to: &str, name: &str, overwrite: bool) -> std::result::Result<(), Fail> {
    let Some((ty, data)) = value(root, from, name) else {
        return if value(root, to, name).is_some() { Ok(()) } else { Err(Fail::Other(t!("Girdi bulunamadı", "Entry not found").into())) };
    };
    match value(root, to, name) {
        Some(_) if !overwrite => {}
        _ => {
            set_value(root, to, name, ty, &data)?;
            if value(root, to, name) != Some((ty, data)) {
                let _ = delete_value(root, to, name);
                return Err(Fail::Other(t!("Girdi kopyalanamadı", "Couldn't copy the entry").into()));
            }
        }
    }
    delete_value(root, from, name)
}
