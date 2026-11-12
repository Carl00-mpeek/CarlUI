//! Bu bölüm, imleç görünümünün Windows'taki gibi serbestçe seçilememesi
//! sorununu şu yöntemle çözüyor: Görünüm sayfasında stil (Neon/Mürekkep),
//! hazır renk örnekleri + "Özel" renk seçici (ChooseColor), dış kenar rengi ve
//! ışıma anahtarı. Her değişiklik kaydedilir; ana süreç imleci hemen yeniler.

use crate::{
    ui::{edit, Flow, P},
    Act,
};
use carlui_core::config::{Config, CursorConfig, CursorStyle};
use carlui_ui::paint::{R, TEXT};
use windows::Win32::Foundation::COLORREF;
use windows::Win32::UI::Controls::Dialogs::*;
use windows::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;

const NEON: [u32; 8] = [0xFF4FD8, 0x9B5CFF, 0x3AA0FF, 0x22D3EE, 0x36E27A, 0xFFB020, 0xFF5A4F, 0xF2F2F5];
const INK: [u32; 7] = [0x1C1C22, 0x3A3A44, 0x2B4C9A, 0x5B2B8A, 0x1F5E3A, 0x7A1F2B, 0xF2F2F5];
const SOFT: [u32; 6] = [0xF7F7F8, 0xE9E4DA, 0xDCE6F5, 0xF6DDE6, 0xDDEFE3, 0x2A2A30];
const ZEN: [u32; 6] = [0xBCD6FF, 0xFFFFFF, 0xFFC4D6, 0xC9B8FF, 0xA8F0E0, 0xFFD9A0];
const EDGE: [u32; 2] = [0xFFFFFF, 0x17171C];

fn parse(s: &str) -> Option<u32> {
    let h = s.trim().trim_start_matches('#');
    (h.len() == 6).then(|| u32::from_str_radix(h, 16).ok()).flatten()
}

fn fmt(c: u32) -> String {
    format!("#{c:06x}")
}

/// Windows renk seçicisi; iptal edilirse None. Vurgu rengi de bunu kullanır.
pub(super) fn pick(initial: u32) -> Option<u32> {
    static mut CUSTOM: [COLORREF; 16] = [COLORREF(0x00FFFFFF); 16];
    let bgr = |c: u32| (c & 0xFF) << 16 | (c & 0xFF00) | (c >> 16) & 0xFF;
    unsafe {
        let mut cc = CHOOSECOLORW {
            lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32,
            hwndOwner: GetActiveWindow(),
            rgbResult: COLORREF(bgr(initial)),
            lpCustColors: std::ptr::addr_of_mut!(CUSTOM) as *mut COLORREF,
            Flags: CC_FULLOPEN | CC_RGBINIT | CC_ANYCOLOR,
            ..Default::default()
        };
        ChooseColorW(&mut cc).as_bool().then(|| bgr(cc.rgbResult.0))
    }
}

/// Renk örnekleri satırı; `auto` verilirse başta bir "Otomatik" örneği olur.
/// Listede olmayan seçili renk, "+" (özel) düğmesinin önünde gösterilir.
#[allow(clippy::too_many_arguments)]
fn swatches(p: &mut P, f: &mut Flow, label: &str, desc: &str, colors: &[u32], value: &str, auto: bool, set: fn(&mut CursorConfig, String)) {
    let sw = 26.0;
    let gap = 9.0;
    let cur = parse(value);
    let extra = cur.filter(|c| !colors.contains(c));
    let count = colors.len() + auto as usize + extra.is_some() as usize + 1;
    let total = count as f32 * (sw + gap);
    let r = f.row(p, label, desc, total);
    let mut x = r.right() - 16.0 - total + gap;
    let y = r.y + (r.h - sw) / 2.0;
    let mut item = |p: &mut P, selected: bool, draw: &dyn Fn(&mut P, R), act: Act| {
        let s = R::new(x, y, sw, sw);
        if selected {
            p.round(s.inset(-4.0), sw / 2.0 + 4.0, 0xFFFFFF, 0.9);
            p.round(s.inset(-2.0), sw / 2.0 + 2.0, 0x000000, 0.9);
        } else if p.hover(s) {
            p.round(s.inset(-3.0), sw / 2.0 + 3.0, 0xFFFFFF, 0.25);
        }
        draw(p, s);
        p.hit(s.inset(-3.0), act);
        x += sw + gap;
    };
    if auto {
        item(
            p,
            value == "auto",
            &|p, s| {
                p.round(s, sw / 2.0, 0xFFFFFF, 1.0);
                p.round(s.inset(sw * 0.27), sw * 0.23, 0x17171C, 1.0);
            },
            edit(move |c| set(&mut c.cursor, "auto".into())),
        );
    }
    for &c in colors.iter().chain(extra.iter()) {
        item(
            p,
            cur == Some(c),
            &|p, s| {
                carlui_ui::tone::literal(|| p.round(s, sw / 2.0, c, 1.0));
                // Beyaz örnek beyaz camda kaybolmasın.
                if carlui_ui::tone::light() {
                    p.outline(s, sw / 2.0, 0xFFFFFF, 0.3);
                }
            },
            edit(move |cfg| set(&mut cfg.cursor, fmt(c))),
        );
    }
    let initial = cur.unwrap_or(0xFFFFFF);
    item(
        p,
        false,
        &|p, s| {
            p.round(s, sw / 2.0, 0xFFFFFF, 0.12);
            p.glyph('\u{E710}', s, 12.0, TEXT, 0.85);
        },
        edit(move |cfg: &mut Config| {
            if let Some(c) = pick(initial) {
                set(&mut cfg.cursor, fmt(c));
            }
        }),
    );
}

pub fn paint(cfg: &Config, p: &mut P, f: &mut Flow) {
    let c = &cfg.cursor;
    f.section(p, t!("İmleç", "Cursor"));
    f.toggle(p, t!("CarlUI imleci", "CarlUI cursor"), t!("Ok ve bağlantı eli. Kapatınca Windows'un imleci geri gelir.", "Arrow and link hand. Turn off to bring back the Windows cursor."), c.enabled, |c| c.cursor.enabled = !c.cursor.enabled);
    if !c.enabled {
        return;
    }
    let styles = [CursorStyle::Neon, CursorStyle::Ink, CursorStyle::Soft, CursorStyle::Zen];
    f.choice(
        p,
        t!("Stil", "Style"),
        t!("Neon: renkli degrade. Mürekkep: koyu, kenarlı. Yumuşak: sade, yuvarlak. Zen: gümüş, sakuralı.", "Neon: colorful gradient. Ink: dark with an outline. Soft: plain and round. Zen: silver with sakura."),
        &[
            ("Neon", edit(|c| c.cursor.style = CursorStyle::Neon)),
            (t!("Mürekkep", "Ink"), edit(|c| c.cursor.style = CursorStyle::Ink)),
            (t!("Yumuşak", "Soft"), edit(|c| c.cursor.style = CursorStyle::Soft)),
            ("Zen", edit(|c| c.cursor.style = CursorStyle::Zen)),
        ],
        styles.iter().position(|s| *s == c.style).unwrap_or(0),
        92.0,
    );
    match c.style {
        CursorStyle::Ink => swatches(p, f, t!("Gövde rengi", "Body color"), t!("+ ile istediğin rengi seç.", "Pick any color with +."), &INK, &c.ink_color, false, |c, v| c.ink_color = v),
        CursorStyle::Soft => swatches(p, f, t!("Gövde rengi", "Body color"), t!("+ ile istediğin rengi seç.", "Pick any color with +."), &SOFT, &c.soft_color, false, |c, v| c.soft_color = v),
        CursorStyle::Zen => swatches(p, f, t!("Işıma rengi", "Glow color"), t!("Gümüş gövdeye hafifçe de çalar.", "Also tints the silver body slightly."), &ZEN, &c.zen_color, false, |c, v| c.zen_color = v),
        CursorStyle::Neon => swatches(p, f, t!("Renk", "Color"), t!("+ ile istediğin rengi seç.", "Pick any color with +."), &NEON, &c.color, false, |c, v| c.color = v),
    }
    f.slider(p, t!("Boyut", "Size"), t!("Windows'un imleç boyutuna göre; erişilebilirlik ayarındaki büyütme de hesaba katılır.", "Relative to the Windows cursor size, including accessibility scaling."), c.scale, 0.5, 1.6, 0.05, |v| t!(format!("%{:.0}", v * 100.0), format!("{:.0}%", v * 100.0)), |c, v| c.cursor.scale = v);
    swatches(p, f, t!("Dış kenar", "Outline"), t!("Otomatik: gövdeye göre beyaz ya da koyu.", "Auto: white or dark depending on the body."), &EDGE, &c.edge, true, |c, v| c.edge = v);
    if matches!(c.style, CursorStyle::Neon | CursorStyle::Zen) {
        f.toggle(p, t!("Işıma", "Glow"), t!("İmlecin çevresinde renkli, yumuşak parıltı.", "A soft colored glow around the cursor."), c.glow, |c| c.cursor.glow = !c.cursor.glow);
    }
}
