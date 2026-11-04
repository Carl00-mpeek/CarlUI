//! Pano. Sorun: kasadan kopyalanan bir parola panoda ya da pano geçmişinde
//! (CarlUI'nin ya da Windows'un) durmamalı, başka uygulamalar okuyamamalı.
//! - `offer` (Aşama 2, varsayılan): panoya değer değil, tek kullanımlık
//!   belirteç konur (`carlui_core::vaultlink`). CarlUI panosu bunu Win+V'de
//!   "kasa kaydı" olarak gösterir; seçilince değer hedef pencereye kasadan
//!   yazılır. Başka uygulamalar panoda düz metin bulamaz.
//! - `copy_secret`: değer "geçmişe alma / buluta gönderme" işaretleriyle
//!   doğrudan konur (kurtarma anahtarı ve `vault.token_paste` kapalıyken).
//!
//! Her ikisinde de süre dolunca, hâlâ panodaysa silinir.

use windows::{
    core::HSTRING,
    Win32::{
        Foundation::*,
        System::{
            DataExchange::*,
            Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
        },
    },
};
use zeroize::Zeroizing;

const CF_UNICODETEXT: u32 = 13;

unsafe fn put(format: u32, bytes: &[u8]) -> bool {
    let Ok(g) = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)) else { return false };
    let p = GlobalLock(g) as *mut u8;
    if p.is_null() {
        let _ = GlobalFree(g);
        return false;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
    let _ = GlobalUnlock(g);
    if SetClipboardData(format, HANDLE(g.0)).is_err() {
        let _ = GlobalFree(g);
        return false;
    }
    true
}

unsafe fn open(hwnd: HWND) -> bool {
    for _ in 0..8 {
        if OpenClipboard(hwnd).is_ok() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    false
}

/// Gizli değeri panoya koyar; pano sıra numarasını döner (silmek için).
pub fn copy_secret(hwnd: HWND, text: &str) -> Option<u32> {
    unsafe {
        if !open(hwnd) {
            return None;
        }
        let _ = EmptyClipboard();
        let data: Zeroizing<Vec<u8>> = Zeroizing::new(text.encode_utf16().chain([0]).flat_map(|u| u.to_le_bytes()).collect());
        let ok = put(CF_UNICODETEXT, &data);
        let reg = |n: &str| RegisterClipboardFormatW(&HSTRING::from(n));
        put(reg("ExcludeClipboardContentFromMonitorProcessing"), &[0]);
        put(reg("CanIncludeInClipboardHistory"), &0u32.to_le_bytes());
        put(reg("CanUploadToCloudClipboard"), &0u32.to_le_bytes());
        let _ = CloseClipboard();
        ok.then(|| GetClipboardSequenceNumber())
    }
}

/// Panoya yalnızca belirteç teklifini koyar (düz metin yok); sıra numarasını döner.
/// Geçmiş/bulut işaretleri de konur: Windows panosu ve diğer pano yöneticileri
/// almasın. CarlUI panosu teklif biçimini bu işaretlerden önce tanır.
pub fn offer(hwnd: HWND, o: &carlui_core::vaultlink::Offer) -> Option<u32> {
    let json = serde_json::to_vec(o).ok()?;
    unsafe {
        if !open(hwnd) {
            return None;
        }
        let _ = EmptyClipboard();
        let reg = |n: &str| RegisterClipboardFormatW(&HSTRING::from(n));
        let ok = put(reg(carlui_core::vaultlink::FORMAT), &json);
        put(reg("ExcludeClipboardContentFromMonitorProcessing"), &[0]);
        put(reg("CanIncludeInClipboardHistory"), &0u32.to_le_bytes());
        put(reg("CanUploadToCloudClipboard"), &0u32.to_le_bytes());
        let _ = CloseClipboard();
        ok.then(|| GetClipboardSequenceNumber())
    }
}

/// Pano hâlâ bizim koyduğumuz değerse boşaltır.
pub fn clear_if(hwnd: HWND, seq: u32) {
    unsafe {
        if GetClipboardSequenceNumber() == seq && open(hwnd) {
            let _ = EmptyClipboard();
            let _ = CloseClipboard();
        }
    }
}

pub fn paste(hwnd: HWND) -> Option<Zeroizing<String>> {
    unsafe {
        if !open(hwnd) {
            return None;
        }
        let out = (|| {
            let h: HANDLE = GetClipboardData(CF_UNICODETEXT).ok()?;
            let g = HGLOBAL(h.0);
            let p = GlobalLock(g) as *const u16;
            if p.is_null() {
                return None;
            }
            let mut n = 0;
            while *p.add(n) != 0 && n < 16 * 1024 {
                n += 1;
            }
            let s = Zeroizing::new(String::from_utf16_lossy(std::slice::from_raw_parts(p, n)));
            let _ = GlobalUnlock(g);
            Some(s)
        })();
        let _ = CloseClipboard();
        out
    }
}
