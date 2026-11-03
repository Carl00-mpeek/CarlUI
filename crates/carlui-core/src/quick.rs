//! Bu modül "hızlı ayarları hem kontrol merkezinden hem aramadan değiştir"
//! sorununu şu yöntemle çözüyor: ses (WASAPI), parlaklık (DDC/CI), Wi-Fi /
//! Bluetooth (Windows radyo API'si) ve karanlık mod (kayıt defteri + yayın)
//! burada tek yerde durur; tepsi paneli ve arama aynı fonksiyonları çağırır.
//! Radyo ve DDC/CI çağrıları yavaştır (50–300 ms): çağıran iş parçacığında yapar.

use windows::{
    core::*,
    Devices::Radios::{Radio, RadioKind, RadioState},
    Win32::{
        Devices::Display::*,
        Foundation::*,
        Graphics::Gdi::*,
        Media::Audio::{Endpoints::IAudioEndpointVolume, *},
        System::{Com::*, Registry::*},
        UI::WindowsAndMessaging::*,
    },
};

pub struct Volume {
    ep: IAudioEndpointVolume,
}

impl Volume {
    /// COM bu iş parçacığında başlatılmış olmalı.
    pub fn new() -> Option<Self> {
        unsafe {
            let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let dev = en.GetDefaultAudioEndpoint(eRender, eMultimedia).ok()?;
            let ep: IAudioEndpointVolume = dev.Activate(CLSCTX_ALL, None).ok()?;
            Some(Self { ep })
        }
    }

    pub fn get(&self) -> (f32, bool) {
        unsafe {
            let v = self.ep.GetMasterVolumeLevelScalar().unwrap_or(0.0);
            let m = self.ep.GetMute().map(|b| b.as_bool()).unwrap_or(false);
            (v, m)
        }
    }

    pub fn set(&self, v: f32) {
        unsafe {
            let _ = self.ep.SetMasterVolumeLevelScalar(v.clamp(0.0, 1.0), std::ptr::null());
            if v > 0.0 {
                let _ = self.ep.SetMute(false, std::ptr::null());
            }
        }
    }

    pub fn set_mute(&self, on: bool) {
        unsafe {
            let _ = self.ep.SetMute(on, std::ptr::null());
        }
    }

    pub fn toggle_mute(&self) {
        let (_, m) = self.get();
        self.set_mute(!m);
    }
}

fn with_primary_physical<R>(f: impl Fn(HANDLE) -> Option<R>) -> Option<R> {
    unsafe {
        let mon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut n = 0u32;
        GetNumberOfPhysicalMonitorsFromHMONITOR(mon, &mut n).ok()?;
        if n == 0 {
            return None;
        }
        let mut list = vec![PHYSICAL_MONITOR::default(); n as usize];
        GetPhysicalMonitorsFromHMONITOR(mon, &mut list).ok()?;
        let r = f(list[0].hPhysicalMonitor);
        let _ = DestroyPhysicalMonitors(&list);
        r
    }
}

/// Ana monitörün parlaklığı 0..1 (DDC/CI desteklenmiyorsa None).
pub fn brightness_get() -> Option<f32> {
    with_primary_physical(|h| unsafe {
        let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
        (GetMonitorBrightness(h, &mut min, &mut cur, &mut max) != 0 && max > min).then(|| (cur - min) as f32 / (max - min) as f32)
    })
}

/// Parlaklığı ayarlar; monitör DDC/CI desteklemiyorsa false.
pub fn brightness_set(v: f32) -> bool {
    with_primary_physical(|h| unsafe {
        let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
        if GetMonitorBrightness(h, &mut min, &mut cur, &mut max) == 0 {
            return None;
        }
        let value = min + ((max - min) as f32 * v.clamp(0.0, 1.0)).round() as u32;
        (SetMonitorBrightness(h, value) != 0).then_some(())
    })
    .is_some()
}

fn radios() -> Result<Vec<Radio>> {
    let _ = Radio::RequestAccessAsync()?.get();
    Ok(Radio::GetRadiosAsync()?.get()?.into_iter().collect())
}

/// (Wi-Fi, Bluetooth) durumu; aygıt yoksa None. WinRT başlatılmış olmalı.
pub fn radio_states() -> (Option<bool>, Option<bool>) {
    let (mut wifi, mut bt) = (None, None);
    if let Ok(list) = radios() {
        for radio in list {
            let on = radio.State().map(|s| s == RadioState::On).unwrap_or(false);
            match radio.Kind() {
                Ok(RadioKind::WiFi) => wifi = Some(wifi.unwrap_or(false) || on),
                Ok(RadioKind::Bluetooth) => bt = Some(bt.unwrap_or(false) || on),
                _ => {}
            }
        }
    }
    (wifi, bt)
}

/// Wi-Fi ya da Bluetooth radyosunu açar/kapatır (bekler). Aygıt yoksa false.
pub fn set_radio(bluetooth: bool, on: bool) -> bool {
    let want = if bluetooth { RadioKind::Bluetooth } else { RadioKind::WiFi };
    let mut found = false;
    if let Ok(list) = radios() {
        for radio in list {
            if radio.Kind().ok() == Some(want) {
                found = true;
                let state = if on { RadioState::On } else { RadioState::Off };
                match radio.SetStateAsync(state).and_then(|op| op.get()) {
                    Ok(res) => crate::info!("radyo {:?} -> {on}: {:?}", want, res),
                    Err(e) => crate::warn!("radyo değiştirilemedi: {e}"),
                }
            }
        }
    }
    found
}

const PERSONALIZE: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");

/// Windows karanlık modda mı (uygulamalar)?
pub fn dark_mode() -> bool {
    let mut v = 1u32;
    let mut size = 4u32;
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, PERSONALIZE, w!("AppsUseLightTheme"), RRF_RT_REG_DWORD, None, Some(&mut v as *mut u32 as _), Some(&mut size)) };
    r.is_ok() && v == 0
}

/// Uygulama ve sistem temasını karanlık/açık yapar ve açık pencerelere duyurur
/// (Ayarlar > Kişiselleştirme > Renkler'deki seçimle aynı).
pub fn set_dark_mode(on: bool) {
    let light: u32 = if on { 0 } else { 1 };
    unsafe {
        let mut key = HKEY::default();
        if RegCreateKeyExW(HKEY_CURRENT_USER, PERSONALIZE, 0, None, REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None, &mut key, None).is_err() {
            crate::warn!("karanlık mod ayarlanamadı");
            return;
        }
        let bytes = light.to_le_bytes();
        let _ = RegSetValueExW(key, w!("AppsUseLightTheme"), 0, REG_DWORD, Some(&bytes));
        let _ = RegSetValueExW(key, w!("SystemUsesLightTheme"), 0, REG_DWORD, Some(&bytes));
        let _ = RegCloseKey(key);
        let topic = w!("ImmersiveColorSet");
        let _ = SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, WPARAM(0), LPARAM(topic.as_ptr() as isize), SMTO_ABORTIFHUNG, 1000, None);
    }
    crate::info!("karanlık mod: {on}");
}
