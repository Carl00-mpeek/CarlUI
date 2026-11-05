//! Bu modül "Steam'i açıp sabitleyince dock'a 'Steam WebHelper' yazılıyor ve
//! sabitlenen ikondan Steam açılmıyor" sorununu çözüyor. Steam'in penceresi
//! asıl uygulamanın değil, yardımcı süreç `steamwebhelper.exe`'nin; dock da
//! sabitlerken o exe'yi kaydediyordu. Windows görev çubuğu bunu kendi "uygulama
//! çözücüsü" ile yapar (Explorer'ın kullandığı, belgelenmemiş
//! `IApplicationResolver`): pencerenin uygulama kimliğini (AppUserModelID —
//! Steam bunu süreç düzeyinde `Valve.Steam.Client` olarak koyar, pencerede
//! görünmez) bulur, sonra o kimliğe en uygun Başlat menüsü kısayolunu seçer.
//! Burada aynı çözücü çağrılır; böylece sabitlenen öğe başlatıcının kısayolu
//! olur (Steam.lnk → steam.exe), adı ve simgesi de doğru gelir.

use std::ffi::c_void;
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::Com::*,
        UI::Shell::IShellItem,
    },
};

const CLSID_RESOLVER: GUID = GUID::from_u128(0x660b90c8_73a9_4b58_8cae_355b7f55341b);
/// Windows 8 ve sonrası arayüz kimliği.
const IID_RESOLVER: GUID = GUID::from_u128(0xde25675a_72de_44b4_9373_05170450c140);

type AppIdFn = unsafe extern "system" fn(*mut c_void, HWND, *mut PWSTR, *mut BOOL, *mut BOOL, *mut BOOL) -> HRESULT;
type ShortcutFn = unsafe extern "system" fn(*mut c_void, PCWSTR, *mut *mut c_void) -> HRESULT;

/// Arayüzün işlev tablosunun kullandığımız başı (sıra Windows'takiyle aynı olmalı).
#[repr(C)]
struct Vtbl {
    /// QueryInterface, AddRef, Release.
    unknown: [usize; 3],
    get_app_id_for_shortcut: usize,
    get_app_id_for_shortcut_object: usize,
    get_app_id_for_window: AppIdFn,
    get_app_id_for_process: usize,
    get_shortcut_for_process: usize,
    get_best_shortcut_for_app_id: ShortcutFn,
}

thread_local! {
    static RESOLVER: Option<IUnknown> = unsafe {
        let unk: Option<IUnknown> = CoCreateInstance(&CLSID_RESOLVER, None, CLSCTX_INPROC_SERVER | CLSCTX_LOCAL_SERVER).ok();
        unk.and_then(|u| {
            let mut p = std::ptr::null_mut();
            u.query(&IID_RESOLVER, &mut p).ok().ok()?;
            (!p.is_null()).then(|| IUnknown::from_raw(p))
        })
    };
}

fn with<T>(f: impl FnOnce(*mut c_void, &Vtbl) -> Option<T>) -> Option<T> {
    RESOLVER.with(|r| {
        let r = r.as_ref()?;
        let this = r.as_raw();
        let vt = unsafe { &**(this as *const *const Vtbl) };
        f(this, vt)
    })
}

fn take(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe {
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as _));
        s
    }
}

/// Pencerenin Windows'a göre uygulama kimliği; ikinci değer: uygulama bunu
/// kendisi mi koydu (yoksa Windows exe yolundan mı türetti).
pub fn app_id(hwnd: HWND) -> Option<(String, bool)> {
    with(|this, vt| unsafe {
        let mut id = PWSTR::null();
        let (mut prevented, mut explicit, mut embedded) = (BOOL(0), BOOL(0), BOOL(0));
        (vt.get_app_id_for_window)(this, hwnd, &mut id, &mut prevented, &mut explicit, &mut embedded).ok().ok()?;
        let id = take(id);
        (!id.is_empty()).then_some((id, explicit.as_bool()))
    })
}

/// Uygulamanın kendi koyduğu kimlik (Steam gibi süreç düzeyinde verilenler dahil).
pub fn explicit_app_id(hwnd: HWND) -> Option<String> {
    app_id(hwnd).and_then(|(id, explicit)| explicit.then_some(id))
}

/// Kimliğe en uygun kısayol (Başlat menüsü, görev çubuğu ya da Uygulamalar klasörü).
pub fn best_shortcut(app_id: &str) -> Option<IShellItem> {
    with(|this, vt| unsafe {
        let mut p = std::ptr::null_mut();
        let id = HSTRING::from(app_id);
        (vt.get_best_shortcut_for_app_id)(this, PCWSTR(id.as_ptr()), &mut p).ok().ok()?;
        (!p.is_null()).then(|| IShellItem::from_raw(p))
    })
}
