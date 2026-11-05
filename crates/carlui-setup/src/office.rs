//! Bu modül "Carl Office isteğe bağlı kurulsun ve Excel/Word/PowerPoint/PDF
//! dosyaları onunla açılabilsin" sorununu yönetici izni istemeden çözüyor:
//! dosya türleri HKCU\Software\Classes altına ProgID olarak yazılır, her
//! uzantının "Birlikte aç" listesine eklenir ve Windows'un Varsayılan
//! uygulamalar sayfasına (RegisteredApplications) tanıtılır. Kullanıcının
//! seçtiği varsayılan uygulama ezilmez; yalnızca uzantının hiç açanı yoksa
//! Carl Office varsayılan olur. Kaldırırken yalnızca kendi yazdıkları silinir.

use crate::shell::{self, Val};
use std::path::Path;
use windows::Win32::{
    System::Registry::{HKEY_CLASSES_ROOT, HKEY_CURRENT_USER},
    UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST},
};

pub const EXE: &str = "carlui-office.exe";
pub const LINK: &str = "Carl Office.lnk";
/// Seçimin hatırlandığı değer (Programlar kaydında): güncelleme (--update) seçimi bozmasın.
const CHOICE: &str = "OfficeModule";
const CAPS: &str = r"Software\CarlUI\Office\Capabilities";
const REGISTERED: &str = r"Software\RegisteredApplications";
const APP_NAME: &str = "Carl Office";

/// (uzantı, ProgID, Türkçe ad, İngilizce ad)
const TYPES: [(&str, &str, &str, &str); 6] = [
    (".xlsx", "CarlUI.Office.Sheet", "Excel çalışma kitabı", "Excel workbook"),
    (".xls", "CarlUI.Office.Sheet", "Excel çalışma kitabı", "Excel workbook"),
    (".csv", "CarlUI.Office.Csv", "CSV dosyası", "CSV file"),
    (".docx", "CarlUI.Office.Doc", "Word belgesi", "Word document"),
    (".pptx", "CarlUI.Office.Slides", "PowerPoint sunusu", "PowerPoint presentation"),
    (".pdf", "CarlUI.Office.Pdf", "PDF belgesi", "PDF document"),
];

fn classes(sub: &str) -> String {
    format!(r"Software\Classes\{sub}")
}

/// Kullanıcı Office'i daha önce seçmiş miydi? Kayıt yoksa (ilk kurulum ya da
/// Office'ten önceki bir sürüm) evet.
pub fn wanted() -> bool {
    shell::reg_get(HKEY_CURRENT_USER, shell::UNINSTALL_KEY, CHOICE).is_none_or(|v| v != "0")
}

pub fn remember(on: bool) {
    let _ = shell::reg_set(shell::UNINSTALL_KEY, &[(CHOICE, Val::Str(if on { "1" } else { "0" }))]);
}

/// Dosya türlerini kaydeder. Hepsi yazılabildiyse true.
pub fn register(dir: &Path) -> bool {
    let exe = dir.join(EXE).display().to_string();
    let icon = format!("{exe},0");
    let open = format!("\"{exe}\" \"%1\"");
    let mut ok = true;
    for (ext, prog, tr, en) in TYPES {
        let name = carlui_core::t!(tr, en);
        ok &= shell::reg_set(&classes(prog), &[("", Val::Str(name)), ("FriendlyTypeName", Val::Str(name))]);
        ok &= shell::reg_set(&classes(&format!(r"{prog}\DefaultIcon")), &[("", Val::Str(&icon))]);
        ok &= shell::reg_set(&classes(&format!(r"{prog}\shell\open\command")), &[("", Val::Str(&open))]);
        ok &= shell::reg_set(&classes(&format!(r"{ext}\OpenWithProgids")), &[(prog, Val::Str(""))]);
        // Uzantının hiç açanı yoksa Carl Office varsayılan olur; varsa dokunulmaz.
        if shell::reg_get(HKEY_CLASSES_ROOT, ext, "").is_none_or(|v| v.is_empty()) {
            ok &= shell::reg_set(&classes(ext), &[("", Val::Str(prog))]);
        }
        ok &= shell::reg_set(&format!(r"{CAPS}\FileAssociations"), &[(ext, Val::Str(prog))]);
        ok &= shell::reg_set(&classes(&format!(r"Applications\{EXE}\SupportedTypes")), &[(ext, Val::Str(""))]);
    }
    let desc = carlui_core::t!("Hafif ofis: Excel, Word, PowerPoint ve PDF dosyalarını açar ve düzenler", "Lightweight office: opens and edits Excel, Word, PowerPoint and PDF files");
    ok &= shell::reg_set(CAPS, &[("ApplicationName", Val::Str(APP_NAME)), ("ApplicationDescription", Val::Str(desc)), ("ApplicationIcon", Val::Str(&icon))]);
    ok &= shell::reg_set(REGISTERED, &[(APP_NAME, Val::Str(CAPS))]);
    ok &= shell::reg_set(&classes(&format!(r"Applications\{EXE}")), &[("FriendlyAppName", Val::Str(APP_NAME))]);
    ok &= shell::reg_set(&classes(&format!(r"Applications\{EXE}\shell\open\command")), &[("", Val::Str(&open))]);
    notify();
    ok
}

/// Kurucunun yazdığı dosya türü kayıtlarını siler.
pub fn unregister() {
    for (ext, prog, _, _) in TYPES {
        shell::reg_delete_value(&classes(&format!(r"{ext}\OpenWithProgids")), prog);
        if shell::reg_get(HKEY_CURRENT_USER, &classes(ext), "").is_some_and(|v| v == prog) {
            shell::reg_delete_value(&classes(ext), "");
        }
        shell::reg_delete_key(&classes(prog));
    }
    shell::reg_delete_key(&classes(&format!(r"Applications\{EXE}")));
    shell::reg_delete_value(REGISTERED, APP_NAME);
    shell::reg_delete_key(r"Software\CarlUI\Office");
    if shell::key_exists(HKEY_CURRENT_USER, r"Software\CarlUI") {
        // Yalnızca boşsa silinir (RegDeleteKey alt anahtarı olanı silmez).
        unsafe {
            let _ = windows::Win32::System::Registry::RegDeleteKeyW(HKEY_CURRENT_USER, windows::core::w!(r"Software\CarlUI"));
        }
    }
    notify();
}

/// Gezgin simgeleri ve "Birlikte aç" listesini yenilesin.
fn notify() {
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
}

/// Office seçimi kaldırıldıysa exe'yi, kısayolu ve kayıtları geri alır.
pub fn remove(dir: &Path) {
    unregister();
    if let Some(m) = shell::menu_dir() {
        let _ = std::fs::remove_file(m.join(LINK));
    }
    let exe = dir.join(EXE);
    if exe.exists() && !shell::recycle(std::slice::from_ref(&exe)) {
        let _ = std::fs::remove_file(exe);
    }
}
