//! Bu modül "seçilen rengi hangi biçimde yazayım?" sorununu çözüyor: piksel
//! rengi ayardaki biçime (HEX, RGB, HSL) çevrilir. Saf hesap; test edilir.

use carlui_core::config::ColorFormat;

pub fn hsl(r: u8, g: u8, b: u8) -> (u32, u32, u32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return (0, 0, (l * 100.0).round() as u32);
    }
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (((h * 60.0).round() as u32) % 360, (s * 100.0).round() as u32, (l * 100.0).round() as u32)
}

pub fn format(rgb: (u8, u8, u8), f: ColorFormat) -> String {
    let (r, g, b) = rgb;
    match f {
        ColorFormat::Hex => format!("#{r:02X}{g:02X}{b:02X}"),
        ColorFormat::Rgb => format!("rgb({r}, {g}, {b})"),
        ColorFormat::Hsl => {
            let (h, s, l) = hsl(r, g, b);
            format!("hsl({h}, {s}%, {l}%)")
        }
    }
}

pub fn next(f: ColorFormat) -> ColorFormat {
    match f {
        ColorFormat::Hex => ColorFormat::Rgb,
        ColorFormat::Rgb => ColorFormat::Hsl,
        ColorFormat::Hsl => ColorFormat::Hex,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        let c = (30, 144, 255);
        assert_eq!(format(c, ColorFormat::Hex), "#1E90FF");
        assert_eq!(format(c, ColorFormat::Rgb), "rgb(30, 144, 255)");
        assert_eq!(format(c, ColorFormat::Hsl), "hsl(210, 100%, 56%)");
        assert_eq!(format((255, 255, 255), ColorFormat::Hsl), "hsl(0, 0%, 100%)");
        assert_eq!(format((255, 0, 0), ColorFormat::Hsl), "hsl(0, 100%, 50%)");
        assert_eq!(format((0, 128, 0), ColorFormat::Hsl), "hsl(120, 100%, 25%)");
    }
}
