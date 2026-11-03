//! Bu bölüm, `wp ad mesaj` için kişilerin ayar dosyasına elle yazılması
//! sorununu şu yöntemle çözüyor: Arama sayfasında aç/kapa anahtarı ve
//! "İçe aktar" düğmesi; seçilen rehber dosyası (.vcf) `carlui_core::vcard`
//! ile okunup kişi listesine eklenir. Kapalıysa ya da kişi yoksa `wp`
//! yalnızca WhatsApp'ı açar.

use crate::ui::{edit, Flow, P};
use carlui_core::config::Config;
use windows::core::{w, PWSTR};
use windows::Win32::UI::Controls::Dialogs::*;
use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;

/// Windows dosya seçicisi (.vcf); iptal edilirse None.
fn pick_vcf() -> Option<std::path::PathBuf> {
    let mut buf = [0u16; 1024];
    unsafe {
        let mut ofn = OPENFILENAMEW {
            lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: GetActiveWindow(),
            lpstrFilter: if carlui_core::lang::is_tr() { w!("Rehber dosyası (*.vcf)\0*.vcf\0Tüm dosyalar\0*.*\0\0") } else { w!("Contacts file (*.vcf)\0*.vcf\0All files\0*.*\0\0") },
            lpstrFile: PWSTR(buf.as_mut_ptr()),
            nMaxFile: buf.len() as u32,
            lpstrTitle: if carlui_core::lang::is_tr() { w!("WhatsApp kişileri için rehber dosyası") } else { w!("Contacts file for WhatsApp") },
            Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
            ..Default::default()
        };
        if !GetOpenFileNameW(&mut ofn).as_bool() {
            return None;
        }
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]).into())
}

/// Rehberi okuyup kişilere ekler (aynı ad varsa numarası güncellenir).
fn import(cfg: &mut Config) {
    let Some(path) = pick_vcf() else { return };
    let text = match std::fs::read(&path) {
        Ok(b) => String::from_utf8_lossy(&b).into_owned(),
        Err(e) => {
            carlui_core::warn!("rehber okunamadı {}: {e}", path.display());
            return;
        }
    };
    let found = carlui_core::vcard::parse(&text);
    carlui_core::info!("rehber içe aktarıldı: {} kişi ({})", found.len(), path.display());
    if !found.is_empty() {
        cfg.launcher.whatsapp.extend(found);
        cfg.launcher.whatsapp_contacts = true;
    }
}

pub fn paint(cfg: &Config, p: &mut P, f: &mut Flow) {
    let c = &cfg.launcher;
    f.toggle(
        p,
        t!("WhatsApp kişileri", "WhatsApp contacts"),
        t!("\"wp ad mesaj\" kişinin sohbetini mesaj hazır açar. Kapalıysa \"wp\" yalnızca WhatsApp'ı açar.", "\"wp name message\" opens the chat with the message ready. When off, \"wp\" only opens WhatsApp."),
        c.whatsapp_contacts,
        |c| c.launcher.whatsapp_contacts ^= true,
    );
    if !c.whatsapp_contacts {
        return;
    }
    let n = c.whatsapp.len();
    let desc = if n == 0 {
        t!("Telefonda Kişiler › Dışa aktar ile .vcf dosyası al, buradan seç. Dosya yalnızca bu bilgisayarda okunur.", "Export a .vcf file from Contacts on your phone and pick it here. The file is only read on this PC.").to_string()
    } else {
        t!(format!("{n} kişi kayıtlı. Yeni dosya eklenir; aynı adlar güncellenir."), format!("{n} contacts saved. A new file adds to them; same names are updated."))
    };
    let items = [(t!("İçe aktar…", "Import…"), edit(import)), (t!("Temizle", "Clear"), edit(|c| c.launcher.whatsapp.clear()))];
    f.choice(p, t!("Rehber", "Contacts"), &desc, if n == 0 { &items[..1] } else { &items }, usize::MAX, 104.0);
}
