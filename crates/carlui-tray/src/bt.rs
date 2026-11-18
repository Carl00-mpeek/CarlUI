//! Bluetooth cihazları (WinRT): eşleştirilmiş cihazlar bağlantı durumuyla,
//! yakındaki eşleştirilmemiş cihazlar ve eşleştirme. Çağrılar bekler; yalnızca
//! arka plan iş parçacığından çağrılmalı (WinRT başlatılmış olmalı).

use std::sync::Mutex;
use windows::{
    core::HSTRING,
    Devices::{
        Bluetooth::{BluetoothConnectionStatus, BluetoothDevice, BluetoothLEDevice, BluetoothMajorClass},
        Enumeration::{DeviceInformation, DevicePairingResultStatus},
    },
};

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub paired: bool,
    pub connected: bool,
    /// Segoe Fluent simgesi (cihaz türüne göre).
    pub glyph: char,
}

/// Son tarama sonucu; panel kurulurken buradan okunur. None: henüz taranmadı.
pub static LIST: Mutex<Option<Vec<Device>>> = Mutex::new(None);

const MAX_NEARBY: usize = 6;

fn class_glyph(major: BluetoothMajorClass) -> char {
    match major {
        BluetoothMajorClass::AudioVideo => '\u{E7F6}',
        BluetoothMajorClass::Peripheral => '\u{E765}',
        BluetoothMajorClass::Phone => '\u{E8EA}',
        BluetoothMajorClass::Computer => '\u{E7F8}',
        _ => '\u{E702}',
    }
}

fn classic(paired: bool) -> windows::core::Result<Vec<Device>> {
    let sel = BluetoothDevice::GetDeviceSelectorFromPairingState(paired)?;
    let mut out = Vec::new();
    for info in DeviceInformation::FindAllAsyncAqsFilter(&sel)?.get()? {
        let id = info.Id()?;
        let name = info.Name()?.to_string();
        let (connected, glyph) = match BluetoothDevice::FromIdAsync(&id).and_then(|op| op.get()) {
            Ok(d) => (
                d.ConnectionStatus().ok() == Some(BluetoothConnectionStatus::Connected),
                d.ClassOfDevice().and_then(|c| c.MajorClass()).map(class_glyph).unwrap_or('\u{E702}'),
            ),
            Err(_) => (false, '\u{E702}'),
        };
        out.push(Device { id: id.to_string(), name, paired, connected, glyph });
    }
    Ok(out)
}

fn low_energy(paired: bool) -> windows::core::Result<Vec<Device>> {
    let sel = BluetoothLEDevice::GetDeviceSelectorFromPairingState(paired)?;
    let mut out = Vec::new();
    for info in DeviceInformation::FindAllAsyncAqsFilter(&sel)?.get()? {
        let id = info.Id()?;
        let name = info.Name()?.to_string();
        // Eşleştirilmemiş LE cihazları için bağlantı sorgusu cihaza bağlanmayı
        // tetikleyebilir; yalnızca eşleştirilmişlere sorulur.
        let connected = paired && BluetoothLEDevice::FromIdAsync(&id).and_then(|op| op.get()).and_then(|d| d.ConnectionStatus()).ok() == Some(BluetoothConnectionStatus::Connected);
        out.push(Device { id: id.to_string(), name, paired, connected, glyph: '\u{E702}' });
    }
    Ok(out)
}

/// Aynı cihaz klasik ve LE olarak iki kez görünebilir: ada göre birleştirir.
pub fn merge(list: Vec<Device>) -> Vec<Device> {
    let mut out: Vec<Device> = Vec::new();
    for d in list {
        if d.name.trim().is_empty() {
            continue;
        }
        match out.iter_mut().find(|o| o.name == d.name) {
            Some(o) => {
                o.connected |= d.connected;
                if d.paired && !o.paired {
                    o.paired = true;
                    o.id = d.id;
                }
                if o.glyph == '\u{E702}' {
                    o.glyph = d.glyph;
                }
            }
            None => out.push(d),
        }
    }
    // Bağlılar önce, sonra eşleştirilmişler, en son yakındakiler.
    out.sort_by_key(|d| (!d.connected, !d.paired));
    let mut nearby = 0;
    out.retain(|d| {
        if d.paired {
            return true;
        }
        nearby += 1;
        nearby <= MAX_NEARBY
    });
    out
}

/// Eşleştirilmiş ve yakındaki cihazları tarar ve `LIST`e yazar.
pub fn refresh() {
    let mut all = Vec::new();
    for paired in [true, false] {
        match classic(paired) {
            Ok(v) => all.extend(v),
            Err(e) => carlui_core::warn!("Bluetooth cihazları okunamadı: {e}"),
        }
        match low_energy(paired) {
            Ok(v) => all.extend(v),
            Err(e) => carlui_core::warn!("Bluetooth LE cihazları okunamadı: {e}"),
        }
    }
    let list = merge(all);
    if let Ok(mut l) = LIST.lock() {
        *l = Some(list);
    }
}

/// Cihazla eşleştirir (Windows gerekirse kendi onay penceresini gösterir).
pub fn pair(id: &str) -> bool {
    let res = DeviceInformation::CreateFromIdAsync(&HSTRING::from(id))
        .and_then(|op| op.get())
        .and_then(|info| info.Pairing())
        .and_then(|p| p.PairAsync())
        .and_then(|op| op.get())
        .and_then(|r| r.Status());
    match res {
        Ok(s) => {
            carlui_core::info!("Bluetooth eşleştirme: {:?}", s);
            s == DevicePairingResultStatus::Paired || s == DevicePairingResultStatus::AlreadyPaired
        }
        Err(e) => {
            carlui_core::warn!("Bluetooth eşleştirilemedi: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(name: &str, paired: bool, connected: bool) -> Device {
        Device { id: name.into(), name: name.into(), paired, connected, glyph: '\u{E702}' }
    }

    #[test]
    fn merge_orders_and_dedupes() {
        let list = merge(vec![dev("Klavye", true, false), dev("", false, false), dev("Kulaklık", true, false), dev("Kulaklık", true, true), dev("Telefon", false, false)]);
        let names: Vec<_> = list.iter().map(|d| (d.name.as_str(), d.connected)).collect();
        assert_eq!(names, [("Kulaklık", true), ("Klavye", false), ("Telefon", false)]);
    }

    #[test]
    fn merge_caps_nearby() {
        let list = merge((0..10).map(|i| dev(&format!("c{i}"), false, false)).collect());
        assert_eq!(list.len(), MAX_NEARBY);
    }
}
