//! Görünüm sayfasının vurgu rengi bölümü: 7 hazır palet, "duvar kâğıdından"
//! seçeneği, özel renk (Windows renk seçicisi ya da renk kodu). Özel ve duvar
//! kâğıdı renkleri çekirdekte (`carlui_core::accent`) okunaklı aralığa çekilir;
//! burada yalnızca seçim yapılır ve sonuç örnek olarak gösterilir.

use crate::{
    input::Form,
    ui::{edit, Flow, P},
    Act, App,
};
use carlui_core::{
    accent::{self, parse_hex, to_hex},
    config::{AccentSource, Config, ThemeConfig},
    theme::{Palette, Rgba},
};
use carlui_ui::paint::{R, TEXT};

const SW: f32 = 30.0;
const GAP: f32 = 10.0;

fn hex(c: Rgba) -> u32 {
    accent::from_rgba(c)
}

/// Renk kodu formunun ilk değeri: özel renk, yoksa şu anki vurgu.
pub fn current_hex(t: &ThemeConfig) -> String {
    match parse_hex(&t.custom_accent) {
        Some(c) => to_hex(c),
        None => to_hex(hex(t.accent_pair().0)),
    }
}

/// Seçim halkası ya da üstüne gelince soluk halka.
fn ring(p: &mut P, s: R, selected: bool) {
    if selected {
        p.round(s.inset(-4.0), SW / 2.0 + 4.0, 0xFFFFFF, 0.9);
        p.round(s.inset(-2.0), SW / 2.0 + 2.0, 0x000000, 0.9);
    } else if p.hover(s) {
        p.round(s.inset(-3.0), SW / 2.0 + 3.0, 0xFFFFFF, 0.25);
    }
}

/// Degradenin iki ucu: dış daire ve içteki küçük daire (paletlerle aynı).
fn pair(p: &mut P, s: R, (a, b): (Rgba, Rgba)) {
    carlui_ui::tone::literal(|| {
        p.round(s, SW / 2.0, hex(a), 1.0);
        p.round(s.inset(SW * 0.28), SW * 0.22, hex(b), 1.0);
    });
}

/// Boş örnek: soluk daire ve simge.
fn empty(p: &mut P, s: R, glyph: char) {
    p.round(s, SW / 2.0, 0xFFFFFF, 0.12);
    p.glyph(glyph, s, 13.0, TEXT, 0.85);
}

/// Kartta görünen değer: hangi kaynak, hangi renk.
fn status(t: &ThemeConfig) -> String {
    match t.accent_source {
        AccentSource::Palette => t.palette.name().to_string(),
        AccentSource::Custom => match parse_hex(&t.custom_accent) {
            Some(c) => t!(format!("Özel · {}", to_hex(c)), format!("Custom · {}", to_hex(c))),
            None => t!(format!("Özel renk geçersiz · {}", t.palette.name()), format!("Invalid custom color · {}", t.palette.name())),
        },
        AccentSource::Wallpaper => match parse_hex(&t.wallpaper_accent) {
            Some(c) => t!(format!("Duvar kâğıdından · {}", to_hex(c)), format!("From wallpaper · {}", to_hex(c))),
            None => t!(format!("Duvar kâğıdında renk yok · {}", t.palette.name()), format!("No color in wallpaper · {}", t.palette.name())),
        },
    }
}

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let c = &app.cfg.theme;
    let custom = parse_hex(&c.custom_accent);
    let wall = parse_hex(&c.wallpaper_accent);
    // 7 palet + duvar kâğıdı + özel + renk seçici.
    let n = Palette::ALL.len() as f32 + 3.0;
    let r = f.row_sub(
        p,
        t!("Vurgu rengi", "Accent color"),
        t!(
            "Hazır paletlerden biri, duvar kâğıdından otomatik renk (resim simgesi) ya da istediğin renk (+). Çok koyu ya da çok açık renkler okunaklı kalsın diye yumuşatılır. Duvar kâğıdında belirgin bir renk yoksa son seçilen palet kullanılır.",
            "One of the palettes, an automatic color from your wallpaper (picture icon) or any color you like (+). Very dark or very light colors are softened so text stays readable. If the wallpaper has no clear color, the last chosen palette is used."
        ),
        &status(c),
        None,
        n * (SW + GAP),
    );
    let mut x = r.right() - 16.0 - n * (SW + GAP) + GAP;
    let y = r.y + (r.h - SW) / 2.0;
    let mut slot = || {
        let s = R::new(x, y, SW, SW);
        x += SW + GAP;
        s
    };
    for pal in Palette::ALL {
        let s = slot();
        ring(p, s, c.accent_source == AccentSource::Palette && pal == c.palette);
        pair(p, s, pal.accent());
        p.hit(
            s.inset(-3.0),
            edit(move |c| {
                c.theme.palette = pal;
                c.theme.accent_source = AccentSource::Palette;
            }),
        );
    }
    // Duvar kâğıdı: renk hesaplandıysa onun degradesi, üstünde resim simgesi.
    let s = slot();
    ring(p, s, c.accent_source == AccentSource::Wallpaper);
    match wall {
        Some(w) => {
            pair(p, s, accent::derive(w));
            carlui_ui::tone::literal(|| p.glyph('\u{E8B9}', s, 12.0, accent::on_accent(w), 0.9));
        }
        None => empty(p, s, '\u{E8B9}'),
    }
    p.hit(s.inset(-3.0), edit(|c| c.theme.accent_source = AccentSource::Wallpaper));
    // Özel renk: seçilmişse kendisi; yoksa tıklayınca renk seçici açılır.
    let s = slot();
    ring(p, s, c.accent_source == AccentSource::Custom);
    let initial = custom.unwrap_or_else(|| hex(c.accent_pair().0));
    match custom {
        Some(v) => {
            pair(p, s, accent::derive(v));
            p.hit(s.inset(-3.0), edit(|c| c.theme.accent_source = AccentSource::Custom));
        }
        None => {
            empty(p, s, '\u{E790}');
            p.hit(s.inset(-3.0), edit(move |c| choose(c, initial)));
        }
    }
    // "+": Windows renk seçicisi.
    let s = slot();
    ring(p, s, false);
    empty(p, s, '\u{E710}');
    p.hit(s.inset(-3.0), edit(move |c| choose(c, initial)));

    let code = current_hex(c);
    let r = f.row_sub(
        p,
        t!("Renk kodu", "Color code"),
        t!("Özel rengi #RRGGBB biçiminde yaz (ör. #4F8BFF). Kaydedince vurgu bu renge geçer.", "Type a custom color as #RRGGBB (e.g. #4F8BFF). Saving switches the accent to it."),
        &code,
        None,
        120.0,
    );
    p.button(R::new(r.right() - 16.0 - 104.0, r.y + (r.h - 32.0) / 2.0, 104.0, 32.0), Some('\u{E70F}'), t!("Değiştir", "Change"), Act::Input(10), app.input.is_none(), false);
    super::form::draw(app, p, f, Form::AccentHex, &[(t!("Renk kodu", "Color code"), "#4F8BFF")]);
}

/// Renk seçiciyi açar; seçilirse özel renk olur.
fn choose(c: &mut Config, initial: u32) {
    if let Some(v) = super::cursor::pick(initial) {
        c.theme.custom_accent = to_hex(v);
        c.theme.accent_source = AccentSource::Custom;
    }
}
