//! Bu modül geçmişteki bir kaydı panoya geri koyup yapıştırma sorununu çözüyor.
//! Resim hem "PNG" hem CF_DIB olarak yazılır ki WhatsApp, Word, Paint gibi
//! farklı uygulamaların hepsi alabilsin. Dosyalar CF_HDROP olarak yazılır.
//! Kendi yazdığımız kopyayı geçmişe tekrar almamak için dönen pano sıra
//! numarası saklanır (GetClipboardSequenceNumber).

use crate::image;
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{
            DataExchange::*,
            Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
        },
        UI::Input::KeyboardAndMouse::*,
    },
};

const CF_UNICODETEXT: u32 = 13;
const CF_DIB: u32 = 8;
const CF_HDROP: u32 = 15;

pub enum Data<'a> {
    Text(&'a str),
    Png(&'a [u8]),
    Files(&'a [String]),
}

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

fn utf16z(s: &str) -> Vec<u8> {
    s.encode_utf16().chain([0]).flat_map(|u| u.to_le_bytes()).collect()
}

/// Panoya yazar. Başarılıysa yeni pano sıra numarasını döndürür.
pub fn set(hwnd: HWND, data: Data) -> Option<u32> {
    unsafe {
        let mut opened = false;
        for _ in 0..8 {
            if OpenClipboard(hwnd).is_ok() {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        if !opened {
            return None;
        }
        let _ = EmptyClipboard();
        let ok = match data {
            Data::Text(t) => put(CF_UNICODETEXT, &utf16z(t)),
            Data::Png(png) => {
                let fmt = RegisterClipboardFormatW(&HSTRING::from("PNG"));
                let a = put(fmt, png);
                let b = image::wic().and_then(|w| image::png_to_dib(&w, png)).is_some_and(|dib| put(CF_DIB, &dib));
                a || b
            }
            Data::Files(list) => {
                // DROPFILES: pFiles=20, pt=0,0, fNC=0, fWide=1; ardından çift sıfırla biten yollar.
                let mut b = Vec::new();
                b.extend_from_slice(&20u32.to_le_bytes());
                b.extend_from_slice(&[0u8; 12]);
                b.extend_from_slice(&1u32.to_le_bytes());
                for f in list {
                    b.extend_from_slice(&utf16z(f));
                }
                b.extend_from_slice(&[0, 0]);
                put(CF_HDROP, &b)
            }
        };
        let _ = CloseClipboard();
        ok.then(|| GetClipboardSequenceNumber())
    }
}

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, time: 0, dwExtraInfo: 0 },
        },
    }
}

/// Odaktaki pencereye Ctrl+V gönderir. Basılı kalmış Win/Alt/Shift önce bırakılır.
pub fn paste() {
    unsafe {
        let mut seq = Vec::new();
        for vk in [VK_LWIN, VK_RWIN, VK_MENU, VK_SHIFT] {
            if GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000 != 0 {
                // Win/Alt tek başına bırakılınca Başlat/menü açılmasın: önce atanmamış bir tuş.
                seq.push(key(VIRTUAL_KEY(0xE8), false));
                seq.push(key(VIRTUAL_KEY(0xE8), true));
                seq.push(key(vk, true));
            }
        }
        seq.extend([key(VK_CONTROL, false), key(VK_V, false), key(VK_V, true), key(VK_CONTROL, true)]);
        SendInput(&seq, std::mem::size_of::<INPUT>() as i32);
    }
}
