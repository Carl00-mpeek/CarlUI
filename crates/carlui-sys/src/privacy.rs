//! Bu modül "mikrofonum/kameram şu an açık mı, hangi uygulama kullanıyor?"
//! sorununu şu yöntemle çözüyor: Windows, mikrofon ve kamerayı kullanan her
//! uygulama için gizlilik kaydında (CapabilityAccessManager\ConsentStore)
//! başlangıç/bitiş zamanı tutar; bitişi 0 olan kayıt şu an kullanıyor demektir
//! (görev çubuğundaki mikrofon simgesiyle aynı kaynak). Yalnızca okunur.
//! Discord gibi uygulamaların kendi içindeki "sustur" düğmesi buradan görünmez;
//! görünen, mikrofonun bir uygulama tarafından açık tutulduğudur.

use windows::{core::HSTRING, Win32::System::Registry::*};

const BASE: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore";

fn open(path: &str) -> Option<HKEY> {
    let mut k = HKEY::default();
    unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(path), 0, KEY_READ, &mut k).ok().ok()? };
    Some(k)
}

fn subkeys(k: HKEY) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let r = unsafe { RegEnumKeyExW(k, i, windows::core::PWSTR(buf.as_mut_ptr()), &mut len, None, windows::core::PWSTR::null(), None, None) };
        if r.is_err() {
            break;
        }
        out.push(String::from_utf16_lossy(&buf[..len as usize]));
        i += 1;
    }
    out
}

fn qword(k: HKEY, sub: &str, name: &str) -> Option<u64> {
    let mut v = 0u64;
    let mut size = 8u32;
    unsafe { RegGetValueW(k, &HSTRING::from(sub), &HSTRING::from(name), RRF_RT_REG_QWORD, None, Some(&mut v as *mut u64 as _), Some(&mut size)).ok().ok()? };
    Some(v)
}

/// "C:#Program Files#Discord#Discord.exe" → "Discord"; paket adı → ilk parça.
fn app_name(key: &str) -> String {
    let last = key.rsplit('#').next().unwrap_or(key);
    let stem = last.trim_end_matches(".exe").trim_end_matches(".EXE");
    let stem = stem.split('_').next().unwrap_or(stem);
    stem.rsplit('.').next().filter(|s| !s.is_empty()).unwrap_or(stem).to_string()
}

fn in_use(k: HKEY, out: &mut Vec<String>) {
    for sub in subkeys(k) {
        if sub == "NonPackaged" {
            continue;
        }
        if qword(k, &sub, "LastUsedTimeStop") == Some(0) && qword(k, &sub, "LastUsedTimeStart").is_some_and(|t| t > 0) {
            let n = app_name(&sub);
            if !out.contains(&n) {
                out.push(n);
            }
        }
    }
}

/// `capability`: "microphone" ya da "webcam". Şu an kullanan uygulamaların adları.
pub fn users(capability: &str) -> Vec<String> {
    let mut out = Vec::new();
    let base = format!("{BASE}\\{capability}");
    for path in [base.clone(), format!("{base}\\NonPackaged")] {
        if let Some(k) = open(&path) {
            in_use(k, &mut out);
            unsafe {
                let _ = RegCloseKey(k);
            }
        }
    }
    out
}
