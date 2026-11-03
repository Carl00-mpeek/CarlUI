//! Tanı aracı: Taskband\Favorites blob'unu ayrıştırıp sabitlenmiş öğeleri yazdırır.
use windows::{
    core::*,
    Win32::{
        System::{Com::*, Registry::*},
        UI::Shell::{Common::ITEMIDLIST, *},
    },
};

fn main() -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let mut size = 0u32;
        let key = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Taskband");
        RegGetValueW(HKEY_CURRENT_USER, key, w!("Favorites"), RRF_RT_REG_BINARY, None, None, Some(&mut size)).ok()?;
        let mut buf = vec![0u8; size as usize];
        RegGetValueW(HKEY_CURRENT_USER, key, w!("Favorites"), RRF_RT_REG_BINARY, None, Some(buf.as_mut_ptr() as _), Some(&mut size)).ok()?;
        println!("blob {} bayt, ilk bayt {:02X}", buf.len(), buf[0]);
        let mut pos = 1usize;
        while pos + 4 <= buf.len() {
            let n = u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap()) as usize;
            let start = pos + 4;
            if n == 0 || start + n > buf.len() {
                println!("@{pos}: n={n} -> bitti");
                break;
            }
            let pidl = buf[start..].as_ptr() as *const ITEMIDLIST;
            let item: Result<IShellItem> = SHCreateItemFromIDList(pidl);
            let desc = match item {
                Ok(it) => {
                    let p = it.GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING).map(|s| s.to_string().unwrap_or_default());
                    let d = it.GetDisplayName(SIGDN_NORMALDISPLAY).map(|s| s.to_string().unwrap_or_default());
                    format!("{:?} | {:?}", d, p)
                }
                Err(e) => format!("hata {e}"),
            };
            // IDList'in gerçek uzunluğu (cb zinciri)
            let mut q = start;
            while q + 2 <= buf.len() {
                let cb = u16::from_le_bytes([buf[q], buf[q + 1]]) as usize;
                if cb == 0 { q += 2; break; }
                q += cb;
            }
            println!("@{pos}: n={n} idlist_len={} sonraki_bayt={:02X?} -> {desc}", q - start, &buf.get(start + n..start + n + 6));
            pos = start + n + 1;
        }
    }
    Ok(())
}
