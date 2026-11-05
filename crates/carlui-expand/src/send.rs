//! Bu modül "kısaltmayı silip yerine açılımı yazmak" sorununu çözüyor: önce
//! kısaltmanın harf sayısı kadar Backspace, ardından metin `KEYEVENTF_UNICODE`
//! ile karakter karakter gönderilir (klavye düzeninden bağımsız). Satır sonu
//! Enter, sekme Tab tuşu olarak gider ki çok satırlı alanlar doğru dolsun.

use windows::Win32::UI::Input::KeyboardAndMouse::*;

/// Kendi gönderdiğimiz tuşların işareti; kanca bunları saymaz.
pub const MARK: usize = 0xC417_E8;

fn vk(v: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: v, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, dwExtraInfo: MARK, ..Default::default() } },
    }
}

fn unit(u: u16, up: bool) -> INPUT {
    let mut f = KEYEVENTF_UNICODE;
    if up {
        f |= KEYEVENTF_KEYUP;
    }
    INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wScan: u, dwFlags: f, dwExtraInfo: MARK, ..Default::default() } } }
}

/// Gönderilecek tuş dizisi (test edilebilsin diye ayrı).
pub fn events(erase: usize, text: &str) -> Vec<INPUT> {
    let mut v = Vec::with_capacity(erase * 2 + text.len() * 2);
    for _ in 0..erase {
        v.push(vk(VK_BACK, false));
        v.push(vk(VK_BACK, true));
    }
    let mut buf = [0u16; 2];
    for c in text.chars() {
        match c {
            '\n' => {
                v.push(vk(VK_RETURN, false));
                v.push(vk(VK_RETURN, true));
            }
            '\t' => {
                v.push(vk(VK_TAB, false));
                v.push(vk(VK_TAB, true));
            }
            '\r' => {}
            _ => {
                for &u in c.encode_utf16(&mut buf).iter() {
                    v.push(unit(u, false));
                    v.push(unit(u, true));
                }
            }
        }
    }
    v
}

pub fn send(erase: usize, text: &str) {
    let ev = events(erase, text);
    // Çok uzun metinler parça parça gider; hedef uygulamanın kuyruğu taşmasın.
    for chunk in ev.chunks(400) {
        let n = unsafe { SendInput(chunk, std::mem::size_of::<INPUT>() as i32) };
        if (n as usize) < chunk.len() {
            carlui_core::warn!("metin genişletici: tuşların bir kısmı gönderilemedi (yönetici penceresi olabilir)");
            return;
        }
        if ev.len() > 400 {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence() {
        let e = events(2, "a\r\nğ😀");
        // 2 silme + a + Enter + ğ + 😀 (iki UTF-16 birimi) = (2+1+1+1+2) * 2
        assert_eq!(e.len(), 14);
        unsafe {
            assert_eq!(e[0].Anonymous.ki.wVk, VK_BACK);
            assert_eq!(e[4].Anonymous.ki.wScan, 'a' as u16);
            assert_eq!(e[6].Anonymous.ki.wVk, VK_RETURN);
            assert!(e[9].Anonymous.ki.dwFlags.contains(KEYEVENTF_KEYUP));
        }
    }
}
