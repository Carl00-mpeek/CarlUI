//! Duraklatma sinyalleri: oturum kilidi, ekranın kapanması, pil tasarrufu ve
//! pilde çalışma. Hepsi olay tabanlı (Windows mesajı); hiçbir şey yoklanmaz.
//! Kayıtlar yalnızca canlı dock açıkken yapılır ve kapanınca geri alınır.

use windows::{
    core::GUID,
    Win32::{
        Foundation::*,
        System::{Power::*, RemoteDesktop::*},
        UI::WindowsAndMessaging::*,
    },
};

/// Ekran açık/kapalı/kısık (Data: 0 kapalı, 1 açık, 2 kısık).
const GUID_CONSOLE_DISPLAY_STATE: GUID = GUID::from_u128(0x6fe69556_704a_47a0_8f24_c28d936fda47);
/// Pil tasarrufu (Data: 1 açık).
const GUID_POWER_SAVING_STATUS: GUID = GUID::from_u128(0xe00958c0_c213_4ace_ac77_fecced2eeea5);
const WTS_SESSION_LOCK: usize = 7;
const WTS_SESSION_UNLOCK: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Power {
    pub locked: bool,
    pub display_off: bool,
    pub saver: bool,
    pub on_battery: bool,
}

pub struct Watch {
    hwnd: HWND,
    notes: Vec<HPOWERNOTIFY>,
    session: bool,
    pub now: Power,
}

impl Watch {
    pub fn new(hwnd: HWND) -> Self {
        let mut notes = Vec::new();
        unsafe {
            for g in [GUID_CONSOLE_DISPLAY_STATE, GUID_POWER_SAVING_STATUS] {
                // Kayıt anında Windows şimdiki değeri de bir kez gönderir.
                if let Ok(h) = RegisterPowerSettingNotification(HANDLE(hwnd.0), &g, DEVICE_NOTIFY_WINDOW_HANDLE) {
                    notes.push(h);
                }
            }
        }
        let session = unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION).is_ok() };
        let mut w = Self { hwnd, notes, session, now: Power::default() };
        w.read_status();
        w
    }

    /// AC/pil ve pil tasarrufu bayrağı.
    fn read_status(&mut self) {
        let mut st = SYSTEM_POWER_STATUS::default();
        if unsafe { GetSystemPowerStatus(&mut st) }.is_ok() {
            self.now.on_battery = st.ACLineStatus == 0;
            self.now.saver = st.SystemStatusFlag == 1;
        }
    }

    /// İlgili mesajsa durumu günceller; değiştiyse `true`.
    pub fn handle(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> bool {
        let before = self.now;
        match msg {
            WM_WTSSESSION_CHANGE => match wp.0 {
                WTS_SESSION_LOCK => self.now.locked = true,
                WTS_SESSION_UNLOCK => self.now.locked = false,
                _ => {}
            },
            WM_POWERBROADCAST => match wp.0 as u32 {
                PBT_APMPOWERSTATUSCHANGE => self.read_status(),
                PBT_POWERSETTINGCHANGE if lp.0 != 0 => {
                    let s = unsafe { &*(lp.0 as *const POWERBROADCAST_SETTING) };
                    if s.DataLength >= 1 {
                        let v = s.Data[0];
                        if s.PowerSetting == GUID_CONSOLE_DISPLAY_STATE {
                            self.now.display_off = v == 0;
                        } else if s.PowerSetting == GUID_POWER_SAVING_STATUS {
                            self.now.saver = v == 1;
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
        self.now != before
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        unsafe {
            for h in self.notes.drain(..) {
                let _ = UnregisterPowerSettingNotification(h);
            }
            if self.session {
                let _ = WTSUnRegisterSessionNotification(self.hwnd);
            }
        }
    }
}
