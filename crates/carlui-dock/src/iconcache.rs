//! Bu modül "dock'ta sağ tık / İndirilenler menüsü bazen donarak açılıyor"
//! sorununu çözüyor. Menü her açılışta her satırın simgesini Windows'tan
//! yeniden çıkarıyordu; uygulamanın logosu da her görev satırı için ayrı ayrı
//! okunuyordu. Ağ paylaşımındaki, çıkarılmış USB'deki ya da uyuyan diskteki
//! bir dosyanın simgesi ise saniyelerce bekletebiliyordu (menü o sırada açılmaz).
//! Burada simgeler bir kez çıkarılıp saklanır; yavaş olabilecek yerlerdeki
//! dosyalara hiç dokunulmaz (satır uygulamanın logosuyla gösterilir).

use std::{cell::RefCell, collections::HashMap};
use windows::{
    core::*,
    Win32::{
        Graphics::Imaging::{CLSID_WICImagingFactory, IWICFormatConverter, IWICImagingFactory},
        Storage::FileSystem::GetDriveTypeW,
        System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
    },
};

/// Sınırsız büyümesin: dolunca baştan başlanır.
const MAX: usize = 400;

thread_local! {
    static WIC: Option<IWICImagingFactory> = unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok() };
    static CACHE: RefCell<HashMap<String, Option<IWICFormatConverter>>> = RefCell::new(HashMap::new());
}

/// Ağ yolu ya da yerel sabit disk/RAM diski dışındaki sürücü: simgesi
/// okunurken beklenebilir (uyanan disk, kopan bağlantı, çıkarılmış ortam).
pub fn slow_path(p: &str) -> bool {
    if p.starts_with("\\\\") || p.starts_with("//") {
        // Ağ paylaşımı (\\sunucu\…); \\?\C:\… biçimi yerel sayılır.
        return !(p.starts_with("\\\\?\\") && p.as_bytes().get(5) == Some(&b':'));
    }
    let b = p.as_bytes();
    if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        let root = HSTRING::from(format!("{}:\\", b[0] as char));
        // 3 = sabit disk, 6 = RAM diski.
        let kind = unsafe { GetDriveTypeW(&root) };
        return kind != 3 && kind != 6;
    }
    false
}

/// `key` için saklanan simge; yoksa `make` ile bir kez üretilir (başarısızlık
/// da saklanır, her açılışta yeniden denenmez).
pub fn get(key: &str, make: impl FnOnce(&IWICImagingFactory) -> Option<IWICFormatConverter>) -> Option<IWICFormatConverter> {
    if let Some(hit) = CACHE.with(|c| c.borrow().get(key).cloned()) {
        return hit;
    }
    let made = WIC.with(|w| w.as_ref().and_then(make));
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= MAX {
            c.clear();
        }
        c.insert(key.to_string(), made.clone());
    });
    made
}

/// Dosyanın / kabuk nesnesinin simgesi (yavaş yerlerdekiler atlanır).
pub fn shell(path: &str, px: i32) -> Option<IWICFormatConverter> {
    if path.is_empty() || slow_path(path) {
        return None;
    }
    get(&format!("{px}|{path}"), |w| carlui_ui::icons::shell_icon(w, path, px).ok())
}
