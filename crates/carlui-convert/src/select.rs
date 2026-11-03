//! Gezgin'de seçili dosyalar. Kısayola basıldığında öndeki pencere bir Gezgin
//! penceresiyse (Windows 11'de etkin sekmesi) ya da masaüstüyse, Shell'in
//! kendi pencere listesinden (IShellWindows) o pencerenin görünümü bulunur ve
//! seçim okunur. Yalnızca gerçek dosya yolları alınır (klasörler atlanır).

use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::Com::*,
        UI::{Shell::*, WindowsAndMessaging::*},
    },
};

const SID_STOP_LEVEL_BROWSER: GUID = GUID::from_u128(0x4C96BE40_915C_11CF_99D3_00AA004AE837);

fn browser(disp: &IDispatch) -> Option<IShellBrowser> {
    unsafe { disp.cast::<IServiceProvider>().ok()?.QueryService::<IShellBrowser>(&SID_STOP_LEVEL_BROWSER).ok() }
}

fn selected(b: &IShellBrowser) -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let Ok(view) = b.QueryActiveShellView() else { return out };
        let Ok(fv) = view.cast::<IFolderView2>() else { return out };
        let Ok(items) = fv.GetSelection(false) else { return out };
        let n = items.GetCount().unwrap_or(0);
        for i in 0..n {
            let Ok(item) = items.GetItemAt(i) else { continue };
            if let Ok(p) = item.GetDisplayName(SIGDN_FILESYSPATH) {
                let s = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as _));
                if std::path::Path::new(&s).is_file() {
                    out.push(s);
                }
            }
        }
    }
    out
}

fn class(h: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// `fg`: kısayola basıldığı andaki ön pencere.
pub fn selection(fg: HWND) -> Vec<String> {
    unsafe {
        let Ok(sw) = CoCreateInstance::<_, IShellWindows>(&ShellWindows, None, CLSCTX_LOCAL_SERVER) else { return Vec::new() };
        let c = class(fg);
        if c == "Progman" || c == "WorkerW" {
            let loc = VARIANT::from(0i32); // CSIDL_DESKTOP
            let mut h = 0i32;
            return sw.FindWindowSW(&loc, &VARIANT::default(), SWC_DESKTOP, &mut h, SWFO_NEEDDISPATCH).ok().and_then(|d| browser(&d)).map(|b| selected(&b)).unwrap_or_default();
        }
        // Aynı pencerede birden çok sekme olabilir: görünür olan etkin sekmedir.
        let n = sw.Count().unwrap_or(0);
        let mut fallback = None;
        for i in 0..n {
            let Ok(d) = sw.Item(&VARIANT::from(i)) else { continue };
            let Some(b) = browser(&d) else { continue };
            let Ok(tab) = b.GetWindow() else { continue };
            if GetAncestor(tab, GA_ROOT) != fg {
                continue;
            }
            if IsWindowVisible(tab).as_bool() {
                return selected(&b);
            }
            fallback.get_or_insert(b);
        }
        fallback.map(|b| selected(&b)).unwrap_or_default()
    }
}
