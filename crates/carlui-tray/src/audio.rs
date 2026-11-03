//! Bu modül tray'deki ses bölümünün iki sorununu çözüyor:
//! - **Hangi hoparlörden çalsın?** Etkin çıkış aygıtları WASAPI'den listelenir;
//!   seçilen aygıt Windows'un ses ayarlarındaki gibi varsayılan yapılır. Bunun
//!   resmi bir API'si yok: Windows'un kendi ayarlarının kullandığı
//!   `IPolicyConfig` arabirimi (EarTrumpet ve SoundSwitch de bunu kullanır)
//!   vtable üzerinden doğrudan çağrılır.
//! - **Uygulama başına ses:** Varsayılan aygıttaki ses oturumları (Ses
//!   Karıştırıcısı'ndaki kaydırıcıların aynısı) uygulama exe'sine göre
//!   gruplanır; bir kaydırıcı o uygulamanın bütün oturumlarını birlikte ayarlar.

use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Media::Audio::*,
        System::{
            Com::{StructuredStorage::PropVariantToStringAlloc, *},
            Threading::*,
        },
        UI::Shell::PropertiesSystem::PROPERTYKEY,
    },
};

pub struct Device {
    pub id: String,
    pub name: String,
    pub default: bool,
}

pub struct App {
    pub exe: String,
    pub name: String,
    pub volume: f32,
    pub muted: bool,
    ctls: Vec<ISimpleAudioVolume>,
}

impl App {
    pub fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.0);
        for c in &self.ctls {
            unsafe {
                let _ = c.SetMasterVolume(self.volume, std::ptr::null());
                if self.volume > 0.0 && self.muted {
                    let _ = c.SetMute(false, std::ptr::null());
                }
            }
        }
        if self.volume > 0.0 {
            self.muted = false;
        }
    }

    pub fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        for c in &self.ctls {
            unsafe {
                let _ = c.SetMute(self.muted, std::ptr::null());
            }
        }
    }
}

// Aygıtın görünen adı (PKEY_Device_FriendlyName; FunctionDiscovery özelliği açmamak için elle).
const FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY { fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0), pid: 14 };

fn enumerator() -> Option<IMMDeviceEnumerator> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok() }
}

fn take_pwstr(p: PWSTR) -> String {
    unsafe {
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as _));
        s
    }
}

/// Etkin çıkış aygıtları; varsayılan olan işaretli.
pub fn devices() -> Vec<Device> {
    let Some(en) = enumerator() else { return Vec::new() };
    unsafe {
        let def = en.GetDefaultAudioEndpoint(eRender, eConsole).ok().and_then(|d| d.GetId().ok()).map(take_pwstr).unwrap_or_default();
        let Ok(list) = en.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return Vec::new() };
        let n = list.GetCount().unwrap_or(0);
        let mut out = Vec::new();
        for i in 0..n {
            let Ok(dev) = list.Item(i) else { continue };
            let Ok(id) = dev.GetId().map(take_pwstr) else { continue };
            let name = dev
                .OpenPropertyStore(STGM_READ)
                .ok()
                .and_then(|ps| ps.GetValue(&FRIENDLY_NAME).ok())
                .and_then(|v| PropVariantToStringAlloc(&v).ok())
                .map(take_pwstr)
                .unwrap_or_else(|| t!("Ses aygıtı", "Audio device").into());
            out.push(Device { default: id == def, id, name });
        }
        out
    }
}

/// Aygıtı bütün roller (konsol, multimedya, iletişim) için varsayılan yapar.
pub fn set_default(id: &str) -> bool {
    const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
    const IID_POLICY_CONFIG: GUID = GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8);
    // IUnknown (3) + GetMixFormat … SetPropertyValue (10) → SetDefaultEndpoint 13. sırada.
    const SET_DEFAULT_ENDPOINT: usize = 13;
    type SetDefault = unsafe extern "system" fn(*mut core::ffi::c_void, PCWSTR, ERole) -> HRESULT;
    unsafe {
        let Ok(unk) = CoCreateInstance::<_, IUnknown>(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL) else { return false };
        let mut p: *mut core::ffi::c_void = std::ptr::null_mut();
        if unk.query(&IID_POLICY_CONFIG, &mut p).is_err() || p.is_null() {
            return false;
        }
        let vtbl = *(p as *const *const usize);
        let f: SetDefault = std::mem::transmute(*vtbl.add(SET_DEFAULT_ENDPOINT));
        let wid = HSTRING::from(id);
        let mut ok = true;
        for role in [eConsole, eMultimedia, eCommunications] {
            ok &= f(p, PCWSTR(wid.as_ptr()), role).is_ok();
        }
        // QueryInterface'in verdiği başvuruyu bırak (IUnknown::Release, 2. sıra).
        let release: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32 = std::mem::transmute(*vtbl.add(2));
        release(p);
        ok
    }
}

fn process_exe(pid: u32) -> String {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        if ok.is_ok() { String::from_utf16_lossy(&buf[..len as usize]) } else { String::new() }
    }
}

/// Varsayılan aygıttaki uygulamalar (exe başına bir satır), en fazla `max` tane.
/// Sistem sesleri en sonda.
pub fn apps(max: usize) -> Vec<App> {
    let Some(en) = enumerator() else { return Vec::new() };
    let mut out: Vec<App> = Vec::new();
    let mut system: Option<App> = None;
    unsafe {
        let Ok(dev) = en.GetDefaultAudioEndpoint(eRender, eMultimedia) else { return out };
        let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { return out };
        let Ok(list) = mgr.GetSessionEnumerator() else { return out };
        for i in 0..list.GetCount().unwrap_or(0) {
            let Ok(ctl) = list.GetSession(i) else { continue };
            if ctl.GetState().is_ok_and(|s| s == AudioSessionStateExpired) {
                continue;
            }
            let Ok(ctl2) = ctl.cast::<IAudioSessionControl2>() else { continue };
            let Ok(vol) = ctl.cast::<ISimpleAudioVolume>() else { continue };
            let v = vol.GetMasterVolume().unwrap_or(1.0);
            let m = vol.GetMute().map(|b| b.as_bool()).unwrap_or(false);
            if ctl2.IsSystemSoundsSession() == S_OK {
                let s = system.get_or_insert_with(|| App { exe: String::new(), name: t!("Sistem sesleri", "System sounds").into(), volume: v, muted: m, ctls: Vec::new() });
                s.ctls.push(vol);
                continue;
            }
            let exe = process_exe(ctl2.GetProcessId().unwrap_or(0));
            if exe.is_empty() {
                continue;
            }
            match out.iter_mut().find(|a| a.exe.eq_ignore_ascii_case(&exe)) {
                Some(a) => a.ctls.push(vol),
                None => {
                    let name = crate::host::file_description(&exe).unwrap_or_default();
                    out.push(App { exe, name, volume: v, muted: m, ctls: vec![vol] });
                }
            }
        }
    }
    out.truncate(max);
    out.extend(system);
    out
}
