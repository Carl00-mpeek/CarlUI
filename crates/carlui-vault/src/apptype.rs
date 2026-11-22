//! Masaüstü uygulamasına (Steam, Discord…) giriş bilgisi yazma. Kasa önce
//! uygulama penceresini öne getirir, sonra:
//! - UI Automation ile parola kutusu (`IsPassword`) ve ondan önceki son
//!   düzenlenebilir kutu (kullanıcı adı) bulunur; her kutuya odak verilip
//!   içeriği seçilir ve değer yazılır. Chromium/CEF/Electron (Discord, Steam…)
//!   erişilebilirlik ağacını ilk istekte kurduğundan bunlara bir kez daha sorulur.
//! - Kutular bulunamazsa ("kör"): odaktaki kutu (kullanıcı adı) seçilip yazılır,
//!   Tab ile parolaya geçilir.
//!
//! Her yazmadan önce pencerenin hâlâ önde olduğuna bakılır; değilse durulur
//! (değer başka bir pencereye gitmesin).

use crate::inject;
use carlui_core::{info, t, warn};
use std::time::Duration;
use windows::Win32::{
    Foundation::HWND,
    System::Com::*,
    UI::{Accessibility::*, Input::KeyboardAndMouse::*},
};
use windows::core::VARIANT;
use zeroize::Zeroizing;

const FRONT_WAIT: Duration = Duration::from_millis(1500);
const STEP: Duration = Duration::from_millis(60);

/// Yerel penceredeki (kullanıcı adı, parola) kutuları.
unsafe fn native_fields(hwnd: HWND) -> windows::core::Result<Option<(Option<IUIAutomationElement>, IUIAutomationElement)>> {
    let ua: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
    let edits = ua.CreatePropertyCondition(UIA_ControlTypePropertyId, &VARIANT::from(UIA_EditControlTypeId.0))?;
    let list = ua.ElementFromHandle(hwnd)?.FindAll(TreeScope_Descendants, &edits)?;
    let mut user = None;
    for i in 0..list.Length()?.min(64) {
        let el = list.GetElement(i)?;
        if el.CurrentIsOffscreen()?.as_bool() || !el.CurrentIsEnabled()?.as_bool() {
            continue;
        }
        if el.CurrentIsPassword()?.as_bool() {
            return Ok(Some((user, el)));
        }
        user = Some(el);
    }
    Ok(None)
}

/// Odaktaki kutuya yazar: içerik seçilir, değer yazılır. Pencere önde değilse durur.
fn put(hwnd: HWND, value: &str) -> Result<(), String> {
    if !inject::wait_front(hwnd, FRONT_WAIT) {
        return Err(t!("Uygulama penceresi önde değil; yazma durduruldu.", "The app window isn't in front; typing stopped.").into());
    }
    if !inject::select_all() || !inject::type_text(value) {
        return Err(t!("Değer yazılamadı.", "Couldn't type the value.").into());
    }
    std::thread::sleep(STEP);
    Ok(())
}

fn key(hwnd: HWND, vk: VIRTUAL_KEY) -> Result<(), String> {
    if !inject::wait_front(hwnd, FRONT_WAIT) {
        return Err(t!("Uygulama penceresi önde değil; yazma durduruldu.", "The app window isn't in front; typing stopped.").into());
    }
    inject::chord(&[], vk);
    std::thread::sleep(STEP);
    Ok(())
}

fn run(hwnd: HWND, user: &str, pass: &str) -> Result<&'static str, String> {
    if !inject::wait_front(hwnd, Duration::from_secs(2)) {
        return Err(t!("Uygulama penceresi öne gelmedi; yazılmadı.", "The app window didn't come to the front; nothing was typed.").into());
    }
    // Pencere öne geldikten sonra odağın yerine oturması için az bekle.
    std::thread::sleep(Duration::from_millis(150));
    let mut fields = unsafe { native_fields(hwnd) }.ok().flatten();
    if fields.is_none() && carlui_core::win::is_chromium_window(hwnd) {
        // Chromium erişilebilirlik ağacını ilk UI Automation isteğinde kurar: kısa bekleyip yeniden sor.
        std::thread::sleep(Duration::from_millis(300));
        fields = unsafe { native_fields(hwnd) }.ok().flatten();
    }
    match fields {
        Some((user_el, pass_el)) => {
            if let (Some(el), false) = (user_el, user.is_empty()) {
                unsafe { el.SetFocus() }.map_err(|e| e.message().to_string())?;
                std::thread::sleep(STEP);
                put(hwnd, user)?;
            }
            unsafe { pass_el.SetFocus() }.map_err(|e| e.message().to_string())?;
            std::thread::sleep(STEP);
            put(hwnd, pass)?;
            key(hwnd, VK_RETURN)?;
            Ok("yerel")
        }
        None => {
            if !user.is_empty() {
                put(hwnd, user)?;
                key(hwnd, VK_TAB)?;
            }
            put(hwnd, pass)?;
            key(hwnd, VK_RETURN)?;
            Ok("kör")
        }
    }
}

/// Ayrı iş parçacığında yazar (kasa penceresi donmasın). `app` günlük için.
pub fn fill(hwnd: HWND, app: String, user: Zeroizing<String>, pass: Zeroizing<String>) {
    let h = hwnd.0 as isize;
    let _ = std::thread::Builder::new().name("vault-appfill".into()).spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        match run(HWND(h as _), &user, &pass) {
            Ok(how) => info!("kasa: {app} uygulamasına giriş bilgisi yazıldı ({how})"),
            Err(e) => warn!("kasa: {app} uygulamasına yazılamadı: {e}"),
        }
    });
}
