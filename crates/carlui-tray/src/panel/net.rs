//! Ağ paneli (görev çubuğu görünümünde ağ simgesine tıklanınca): Ethernet
//! durumu, görünen kablosuz ağlar ve ağ ayarları. Açılınca önce bilinen liste
//! gösterilir, bağdaştırıcıya tarama yaptırılır ve sonuç gelince liste yenilenir.

use super::*;
use crate::wifi::{self, Network};

/// Ağ panelindeki tıklanabilir satır.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum NetRow {
    Ethernet,
    Wifi(usize),
    /// Kayıtlı profili olmayan ağlar için Windows'un ağ listesi.
    More,
    Settings,
    /// Bluetooth panelinde radyoyu açıp kapatan satır.
    BtToggle,
    /// Bluetooth cihazı (`bt_devs` sırası).
    BtDevice(usize),
    BtSettings,
}

/// Bluetooth taraması sürüyor mu (üst üste başlatılmasın).
static BT_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

const MAX_NETS: usize = 8;

/// Wi-Fi sinyal çubukları (Segoe Fluent Icons: Wifi1..3, Wifi).
fn bars(signal: u32) -> char {
    match signal {
        0..=25 => '\u{E872}',
        26..=50 => '\u{E873}',
        51..=75 => '\u{E874}',
        _ => '\u{E701}',
    }
}

impl Panel {
    /// Taramayı başlatır; birkaç saniye sonra panele yenile mesajı gelir.
    pub(super) fn scan_later(&self, delay_ms: u64, rescan: bool) {
        let hwnd = self.hwnd.0 as isize;
        std::thread::spawn(move || {
            if rescan {
                wifi::scan();
            }
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            unsafe {
                let _ = PostMessageW(HWND(hwnd as _), WM_APP_NETS, WPARAM(0), LPARAM(0));
            }
        });
    }

    fn net_row(&mut self, gfx: &Gfx, kids: &VisualCollection, y: f32, glyph: char, name: &str, note: &str, lit: bool, what: NetRow) -> Result<f32> {
        let s = self.s;
        let pad = (16.0 * s).round();
        let rs = self.theme.radius_scale;
        let row_w = self.width - pad * 2.0;
        let row_h = (40.0 * s).round();
        let row = self.comp.CreateContainerVisual()?;
        row.SetOffset(v3(pad, y))?;
        row.SetSize(v2(row_w, row_h))?;
        let rk = row.Children()?;
        let off: CompositionBrush = self.comp.CreateColorBrushWithColor(color(carlui_core::theme::Rgba(255, 255, 255, 0x10)))?.cast()?;
        let (bg, _) = self.rrect(row_w, row_h, 10.0 * s * rs, &off)?;
        rk.InsertAtTop(&bg)?;
        if lit {
            let (on, _) = self.rrect(row_w, row_h, 10.0 * s * rs, &self.accent_brush(true)?)?;
            on.SetOpacity(if carlui_ui::tone::light() { 0.2 } else { 0.45 })?;
            rk.InsertAtTop(&on)?;
        }
        let g = self.text.glyph(gfx, glyph, 15.0 * s, if lit { 0.95 } else { 0.8 })?;
        rk.InsertAtTop(&self.sprite(&g.surface, g.width, g.height, 12.0 * s, (row_h - g.height) / 2.0)?)?;
        let mut right = row_w - 12.0 * s;
        if !note.is_empty() {
            let n = self.text.render(gfx, note, 11.5 * s, false, row_w / 2.0, 0.65)?;
            right -= n.width;
            rk.InsertAtTop(&self.sprite(&n.surface, n.width, n.height, right, (row_h - n.height) / 2.0)?)?;
            right -= 8.0 * s;
        }
        let t = self.text.render(gfx, name, 12.5 * s, lit, (right - 40.0 * s).max(20.0), if lit { 0.95 } else { 0.85 })?;
        rk.InsertAtTop(&self.sprite(&t.surface, t.width, t.height, 40.0 * s, (row_h - t.height) / 2.0)?)?;
        kids.InsertAtTop(&row)?;
        self.net_rows.push((rect(pad, y, row_w, row_h), what));
        Ok(row_h + (4.0 * s).round())
    }

    fn net_label(&self, gfx: &Gfx, kids: &VisualCollection, text: &str, y: f32) -> Result<f32> {
        let s = self.s;
        let pad = (16.0 * s).round();
        let t = self.text.render(gfx, text, 11.5 * s, true, self.width - pad * 2.0, 0.55)?;
        kids.InsertAtTop(&self.sprite(&t.surface, t.width, t.height, pad + 4.0 * s, y)?)?;
        Ok(t.height + (6.0 * s).round())
    }

    /// Ağ panelinin içeriği; bittiği yeri döndürür.
    pub(super) fn build_net(&mut self, gfx: &Gfx, content: &ContainerVisual) -> Result<f32> {
        let s = self.s;
        let pad = (16.0 * s).round();
        let kids = content.Children()?;
        self.net_rows.clear();
        let mut y = pad;

        if carlui_core::wired::wired_up() {
            y += self.net_label(gfx, &kids, "Ethernet", y)?;
            y += self.net_row(gfx, &kids, y, '\u{E839}', "Ethernet", t!("Bağlı", "Connected"), true, NetRow::Ethernet)?;
            y += (8.0 * s).round();
        }

        y += self.net_label(gfx, &kids, "Wi-Fi", y)?;
        match wifi::networks() {
            None => y += self.net_label(gfx, &kids, t!("Kablosuz bağdaştırıcı yok", "No wireless adapter"), y)?,
            Some(list) if list.is_empty() => {
                let msg = if self.radios.wifi == Some(false) { t!("Wi-Fi kapalı", "Wi-Fi is off") } else { t!("Ağ aranıyor…", "Looking for networks…") };
                y += self.net_label(gfx, &kids, msg, y)?;
            }
            Some(list) => {
                self.nets = list.into_iter().take(MAX_NETS).collect::<Vec<Network>>();
                for i in 0..self.nets.len() {
                    let n = self.nets[i].clone();
                    let note = if n.connected {
                        t!("Bağlı", "Connected")
                    } else if n.profile.is_empty() && n.secure {
                        "\u{1F512}"
                    } else {
                        ""
                    };
                    y += self.net_row(gfx, &kids, y, bars(n.signal), &n.ssid, note, n.connected, NetRow::Wifi(i))?;
                }
                if self.nets.iter().any(|n| !n.connected && n.profile.is_empty()) {
                    y += self.net_row(gfx, &kids, y, '\u{E712}', t!("Yeni ağa bağlan…", "Connect to a new network…"), "", false, NetRow::More)?;
                }
            }
        }
        y += (8.0 * s).round();
        y += self.net_row(gfx, &kids, y, '\u{E713}', t!("Ağ ve internet ayarları", "Network & internet settings"), "", false, NetRow::Settings)?;
        Ok(y + pad - (4.0 * s).round())
    }

    /// Bluetooth cihazlarını arka planda tarar (önce `pair` verilmişse o cihazla
    /// eşleştirir); bitince panele yenile mesajı gelir.
    pub(super) fn bt_later(&self, pair: Option<String>) {
        use std::sync::atomic::Ordering;
        if pair.is_none() && BT_BUSY.swap(true, Ordering::AcqRel) {
            return;
        }
        let hwnd = self.hwnd.0 as isize;
        std::thread::spawn(move || {
            let _ = unsafe { windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED) };
            let pairing = pair.is_some();
            if let Some(id) = pair {
                crate::bt::pair(&id);
            }
            crate::bt::refresh();
            if !pairing {
                BT_BUSY.store(false, Ordering::Release);
            }
            unsafe {
                let _ = PostMessageW(HWND(hwnd as _), WM_APP_NETS, WPARAM(0), LPARAM(0));
            }
        });
    }

    /// Bluetooth panelinin içeriği; bittiği yeri döndürür.
    pub(super) fn build_bt(&mut self, gfx: &Gfx, content: &ContainerVisual) -> Result<f32> {
        let s = self.s;
        let pad = (16.0 * s).round();
        let kids = content.Children()?;
        self.net_rows.clear();
        let mut y = pad;

        y += self.net_label(gfx, &kids, "Bluetooth", y)?;
        match self.radios.bluetooth {
            None => y += self.net_label(gfx, &kids, t!("Bluetooth bağdaştırıcısı yok", "No Bluetooth adapter"), y)?,
            Some(on) => {
                let note = if on { t!("Açık", "On") } else { t!("Kapalı", "Off") };
                y += self.net_row(gfx, &kids, y, '\u{E702}', "Bluetooth", note, on, NetRow::BtToggle)?;
                y += (8.0 * s).round();
                if !on {
                    y += self.net_label(gfx, &kids, t!("Cihazları görmek için Bluetooth'u açın", "Turn on Bluetooth to see devices"), y)?;
                } else {
                    let list = crate::bt::LIST.lock().ok().and_then(|l| l.clone());
                    match list {
                        None => y += self.net_label(gfx, &kids, t!("Cihazlar aranıyor…", "Looking for devices…"), y)?,
                        Some(list) => {
                            self.bt_devs = list;
                            let mut section = None;
                            for k in 0..self.bt_devs.len() {
                                let d = self.bt_devs[k].clone();
                                let here = if d.connected { 0 } else if d.paired { 1 } else { 2 };
                                if section != Some(here) {
                                    if section.is_some() {
                                        y += (4.0 * s).round();
                                    }
                                    section = Some(here);
                                    let title = match here {
                                        0 => t!("Bağlı", "Connected"),
                                        1 => t!("Eşleştirilmiş", "Paired"),
                                        _ => t!("Yakındaki cihazlar", "Nearby devices"),
                                    };
                                    y += self.net_label(gfx, &kids, title, y)?;
                                }
                                let note = match here {
                                    0 => t!("Bağlı", "Connected"),
                                    1 => "",
                                    _ => t!("Eşleştir", "Pair"),
                                };
                                y += self.net_row(gfx, &kids, y, d.glyph, &d.name, note, d.connected, NetRow::BtDevice(k))?;
                            }
                            if self.bt_devs.is_empty() {
                                y += self.net_label(gfx, &kids, t!("Cihaz bulunamadı", "No devices found"), y)?;
                            }
                        }
                    }
                }
            }
        }
        y += (8.0 * s).round();
        y += self.net_row(gfx, &kids, y, '\u{E713}', t!("Bluetooth ve cihaz ayarları", "Bluetooth & devices settings"), "", false, NetRow::BtSettings)?;
        Ok(y + pad - (4.0 * s).round())
    }

    /// Ağ satırına tıklandı. Dönüş: panel kapanmalı mı.
    pub(super) fn net_click(&mut self, i: usize) -> bool {
        let Some(&(_, what)) = self.net_rows.get(i) else { return false };
        match what {
            NetRow::BtToggle => {
                let on = self.radios.bluetooth != Some(true);
                quick::set_radio(true, on, self.hwnd.0 as isize, WM_APP_RADIOS);
                false
            }
            NetRow::BtDevice(k) => {
                let Some(d) = self.bt_devs.get(k) else { return false };
                if d.paired {
                    // Bağlanma/ayırma Windows'ta herkese açık API değil: ayarlar açılır.
                    open_uri("ms-settings:bluetooth");
                    return true;
                }
                carlui_core::info!("Bluetooth eşleştiriliyor: {}", d.name);
                self.bt_later(Some(d.id.clone()));
                false
            }
            NetRow::BtSettings => {
                open_uri("ms-settings:bluetooth");
                true
            }
            NetRow::Ethernet => {
                open_uri("ms-settings:network-ethernet");
                true
            }
            NetRow::Settings => {
                open_uri("ms-settings:network");
                true
            }
            NetRow::More => {
                open_uri("ms-availablenetworks:");
                true
            }
            NetRow::Wifi(k) => {
                let Some(n) = self.nets.get(k) else { return false };
                if n.connected {
                    open_uri("ms-settings:network-wifi");
                    return true;
                }
                if n.profile.is_empty() {
                    // Parolayı Windows'un ağ listesi sorar.
                    open_uri("ms-availablenetworks:");
                    return true;
                }
                if wifi::connect(&n.profile) {
                    carlui_core::info!("Wi-Fi bağlanıyor: {}", n.ssid);
                } else {
                    carlui_core::warn!("Wi-Fi bağlanamadı: {}", n.ssid);
                }
                self.scan_later(3500, false);
                false
            }
        }
    }
}

impl Panel {
    /// Ağ satırına sağ tık: Ethernet ile Wi-Fi arasında geçiş. Dönüş: panel kapanmalı mı.
    ///
    /// - Ethernet satırı → Wi-Fi: radyo açılır ve Windows'un ağ listesi gelir
    ///   (bağdaştırıcıyı kapatmak yönetici ister; kablo takılıyken Windows
    ///   Ethernet'i yine öncelikli tutabilir).
    /// - Wi-Fi satırı → Ethernet: kablo bağlıysa Wi-Fi radyosu kapanır ve
    ///   trafik Ethernet'e geçer; kablo yoksa Ethernet ayarları açılır.
    /// - Diğer satırlar → Ağ ve internet ayarları.
    pub(super) fn net_right_click(&mut self, i: usize) -> bool {
        let Some(&(_, what)) = self.net_rows.get(i) else { return false };
        match what {
            NetRow::Ethernet => {
                quick::to_wireless(self.radios, self.hwnd.0 as isize, WM_APP_RADIOS);
                true
            }
            NetRow::Wifi(_) if carlui_core::wired::wired_up() => {
                carlui_core::info!("Ethernet'e geçiliyor: Wi-Fi kapatılıyor");
                quick::set_radio(false, false, self.hwnd.0 as isize, WM_APP_RADIOS);
                // Radyo kapanınca liste "Wi-Fi kapalı"ya döner.
                self.scan_later(1200, false);
                false
            }
            NetRow::Wifi(_) => {
                open_uri("ms-settings:network-ethernet");
                true
            }
            NetRow::More | NetRow::Settings => {
                open_uri("ms-settings:network");
                true
            }
            NetRow::BtToggle | NetRow::BtDevice(_) | NetRow::BtSettings => {
                open_uri("ms-settings:bluetooth");
                true
            }
        }
    }
}

pub(super) fn open_uri(uri: &str) {
    unsafe {
        windows::Win32::UI::Shell::ShellExecuteW(HWND::default(), w!("open"), &HSTRING::from(uri), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}
