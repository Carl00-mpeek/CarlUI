//! Bu modül "kullanıcı kısayolu değiştirirken sistemi bozmasın" sorununu,
//! tek bir ortak doğrulayıcıyla çözüyor. Tüm modüller (ve ileride Ayarlar
//! penceresi) kısayolları buradan geçirir:
//! - Sol tık (Mouse1) asla kısayol olamaz. Diğer fare tuşları Mouse2–Mouse5 adıyla
//!   ana tuş olabilir; kaydı `hotkey` modülü yapar.
//! - Değiştiricisiz tek tuş olamaz (yazı yazarken tetiklenir); F13–F24 ve fare tuşları serbest.
//! - Windows'un kritik kısayolları ve kopyala/yapıştır gibi temel kısayollar alınamaz.
//! Geçersiz değer gelirse çağıran taraf varsayılana döner ve loga yazar.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

impl Mods {
    pub fn any(&self) -> bool {
        self.ctrl || self.alt || self.shift || self.win
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Combo {
    pub mods: Mods,
    /// Sanal tuş kodu (VK_*).
    pub vk: u32,
}

impl Combo {
    /// Ana tuş bir fare tuşu mu (Mouse2–Mouse5)?
    pub fn is_mouse(&self) -> bool {
        matches!(self.vk, 0x02 | 0x04 | 0x05 | 0x06)
    }
}

const MOUSE_WORDS: &[&str] = &[
    "click", "tık", "tik", "tıklama", "mouse", "fare", "lbutton", "rbutton", "mbutton", "xbutton", "lmb", "rmb", "mmb", "left", "right",
    "middle", "sol", "sağ", "sag", "orta", "wheel", "tekerlek", "button", "düğme", "dugme",
];

fn word_to_mod(w: &str, m: &mut Mods) -> bool {
    match w {
        "ctrl" | "control" | "ctl" => m.ctrl = true,
        "alt" => m.alt = true,
        "shift" => m.shift = true,
        "win" | "windows" | "super" => m.win = true,
        _ => return false,
    }
    true
}

fn word_to_vk(w: &str) -> Option<u32> {
    Some(match w {
        "space" | "boşluk" | "bosluk" => 0x20,
        "enter" | "return" => 0x0D,
        "tab" => 0x09,
        "esc" | "escape" => 0x1B,
        "backspace" => 0x08,
        "insert" | "ins" => 0x2D,
        "delete" | "del" => 0x2E,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" | "pgup" => 0x21,
        "pagedown" | "pgdn" => 0x22,
        "up" => 0x26,
        "down" => 0x28,
        "pause" => 0x13,
        "printscreen" | "prtsc" => 0x2C,
        "`" | "backtick" => 0xC0,
        "-" => 0xBD,
        "=" => 0xBB,
        "," => 0xBC,
        "." => 0xBE,
        "/" => 0xBF,
        k if k.len() > 1 && k.starts_with('f') && k[1..].parse::<u32>().is_ok_and(|n| (1..=24).contains(&n)) => 0x6F + k[1..].parse::<u32>().ok()?,
        k if k.chars().count() == 1 && k.chars().all(|c| c.is_ascii_alphanumeric()) => k.to_ascii_uppercase().as_bytes()[0] as u32,
        _ => return None,
    })
}

/// Kısayol olabilen fare tuşları; Mouse1 (sol tık) bilerek yok.
fn mouse_vk(w: &str) -> Option<u32> {
    Some(match w {
        "mouse2" => 0x02,
        "mouse3" => 0x04,
        "mouse4" => 0x05,
        "mouse5" => 0x06,
        _ => return None,
    })
}

fn is_mouse(w: &str) -> bool {
    MOUSE_WORDS.iter().any(|m| w == *m || w.contains(m) && w.len() > 3)
}

/// Yalnızca değiştiricilerden oluşan bir kısayol ("Win", "Ctrl+Alt"), ör. uygulama numaraları için.
pub fn parse_mods(s: &str) -> Result<Mods, String> {
    let mut m = Mods::default();
    for w in s.split('+').map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty()) {
        if is_mouse(&w) {
            return Err(t!("burada fare tuşu kullanılamaz", "mouse buttons can't be used here").into());
        }
        if !word_to_mod(&w, &mut m) {
            return Err(t!(format!("'{w}' bir değiştirici tuş değil (Ctrl, Alt, Shift, Win)"), format!("'{w}' is not a modifier key (Ctrl, Alt, Shift, Win)")));
        }
    }
    if !m.any() {
        return Err(t!("en az bir değiştirici tuş gerekli", "at least one modifier key needed").into());
    }
    if m == (Mods { shift: true, ..Default::default() }) {
        return Err(t!("yalnızca Shift olamaz (büyük harf/sembol yazarken tetiklenir)", "Shift alone isn't allowed (fires while typing capitals/symbols)").into());
    }
    Ok(m)
}

/// "Ctrl+Space", "Alt+Shift+F2" gibi tam bir kısayol.
pub fn parse_combo(s: &str) -> Result<Combo, String> {
    let mut m = Mods::default();
    let mut vk = None;
    for w in s.split('+').map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty()) {
        if is_mouse(&w) && mouse_vk(&w).is_none() {
            return Err(t!("sol tık kısayol olamaz; fare için Mouse2–Mouse5 kullan", "left click can't be a shortcut; use Mouse2–Mouse5 for the mouse").into());
        }
        if word_to_mod(&w, &mut m) {
            continue;
        }
        if vk.is_some() {
            return Err(t!("kısayolda yalnızca bir ana tuş olabilir", "a shortcut can have only one main key").into());
        }
        vk = Some(mouse_vk(&w).or_else(|| word_to_vk(&w)).ok_or_else(|| t!(format!("'{w}' tanınmayan bir tuş"), format!("'{w}' is an unknown key")))?);
    }
    let vk = vk.ok_or(t!("kısayolda bir ana tuş olmalı (ör. Ctrl+Space)", "a shortcut needs a main key (e.g. Ctrl+Space)"))?;
    let c = Combo { mods: m, vk };
    // F13–F24, PrintScreen ve fare tuşları yazı yazmaz; tek başına kullanılabilir.
    let free_alone = (0x7C..=0x87).contains(&vk) || vk == 0x2C || c.is_mouse();
    if !m.any() && !free_alone {
        return Err(t!("değiştiricisiz tek tuş olamaz (yazı yazarken tetiklenir)", "a single key without modifiers isn't allowed (fires while typing)").into());
    }
    if m == (Mods { shift: true, ..Default::default() }) && !free_alone {
        return Err(t!("yalnızca Shift ile olamaz (yazı yazarken tetiklenir)", "Shift alone isn't allowed (fires while typing)").into());
    }
    let only = |ctrl, alt, shift, win| m == Mods { ctrl, alt, shift, win };
    let reserved = (only(true, true, false, false) && vk == 0x2E) // Ctrl+Alt+Del
        || (only(false, true, false, false) && matches!(vk, 0x09 | 0x73 | 0x1B | 0x20)) // Alt+Tab, Alt+F4, Alt+Esc, Alt+Space
        || (only(true, false, true, false) && vk == 0x1B) // Ctrl+Shift+Esc
        || (only(true, false, false, false) && matches!(vk, 0x41 | 0x43 | 0x56 | 0x58 | 0x59 | 0x5A | 0x53 | 0x1B)) // Ctrl+A/C/V/X/Y/Z/S, Ctrl+Esc
        || (only(false, false, false, true) && matches!(vk, 0x4C | 0x44 | 0x09)); // Win+L, Win+D, Win+Tab
    if reserved {
        return Err(t!("bu kısayol Windows'un temel bir kısayolu; alınamaz", "this is a core Windows shortcut; it can't be taken").into());
    }
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combos() {
        assert_eq!(parse_combo("Ctrl+Space").unwrap().vk, 0x20);
        assert!(parse_combo("ctrl + alt + f2").is_ok());
        assert!(parse_combo("F13").is_ok());
        assert!(parse_combo("Sol tık").is_err());
        assert!(parse_combo("Ctrl+LeftClick").is_err());
        assert!(parse_combo("Mouse1").is_err());
        assert!(parse_combo("A").is_err());
        assert!(parse_combo("Space").is_err());
        assert!(parse_combo("Shift+A").is_err());
        assert!(parse_combo("Ctrl+C").is_err());
        assert!(parse_combo("Alt+Tab").is_err());
        assert!(parse_combo("Win+L").is_err());
        assert!(parse_combo("Ctrl+Alt+Del").is_err());
        assert!(parse_combo("Ctrl").is_err());
        assert!(parse_combo("Ctrl+A+B").is_err());
    }

    #[test]
    fn mouse() {
        for (s, vk) in [("Mouse2", 0x02), ("Mouse3", 0x04), ("Mouse4", 0x05), ("mouse5", 0x06)] {
            let c = parse_combo(s).unwrap();
            assert_eq!(c.vk, vk);
            assert!(c.is_mouse());
        }
        assert!(parse_combo("Ctrl+Mouse4").is_ok_and(|c| c.mods.ctrl && c.is_mouse()));
        assert!(parse_combo("Shift+Mouse5").is_ok());
        assert!(!parse_combo("Ctrl+Space").unwrap().is_mouse());
        // Mouse1 hiçbir biçimde olmaz.
        for s in ["Mouse1", "Ctrl+Mouse1", "Win+Alt+Mouse1", "Sağ tık", "LeftClick", "Mouse6", "Mouse0"] {
            assert!(parse_combo(s).is_err(), "{s}");
        }
        assert!(parse_combo("Mouse4+Mouse5").is_err());
        assert!(parse_combo("A+Mouse4").is_err());
        assert!(parse_mods("Ctrl+Mouse4").is_err());
    }

    #[test]
    fn mods() {
        assert_eq!(parse_mods("Win").unwrap(), Mods { win: true, ..Default::default() });
        assert!(parse_mods("Ctrl+Alt").is_ok());
        assert!(parse_mods("Shift").is_err());
        assert!(parse_mods("Win+1").is_err());
        assert!(parse_mods("sol tık").is_err());
        assert!(parse_mods("").is_err());
    }
}
