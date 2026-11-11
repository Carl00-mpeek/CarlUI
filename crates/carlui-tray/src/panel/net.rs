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
}

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

    /// Ağ satırına tıklandı. Dönüş: panel kapanmalı mı.
    pub(super) fn net_click(&mut self, i: usize) -> bool {
        let Some(&(_, what)) = self.net_rows.get(i) else { return false };
        match what {
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

pub(super) fn open_uri(uri: &str) {
    unsafe {
        windows::Win32::UI::Shell::ShellExecuteW(HWND::default(), w!("open"), &HSTRING::from(uri), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}
