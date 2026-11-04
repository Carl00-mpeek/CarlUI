//! Ekran görüntüsü seçimindeki QR kodları (Carl Vault Aşama 4). Seçim bitince
//! seçili alan gri tonlamaya çevrilip `rqrr` ile taranır; bulunan her kodun
//! içeriği türüne ayrılır ve düzenleyicide türe uygun düğmeler çıkar:
//! - 2FA (`otpauth://totp/…`): "Carl Vault" → anahtar kasanın doğrulanmış
//!   borusuna gider, kasa doldurulmuş yeni kayıt açar. Panoya hiç girmez.
//!   Google Authenticator'ın aktarım QR'ı (`otpauth-migration://`) da aynı
//!   yoldan gider; kasa içindeki hesapları içe aktarma ekranında gösterir.
//! - Bağlantı (http/https): Aç / Kopyala. Wi-Fi: parolayı kopyala.
//! - E-posta, telefon, kişi kartı, düz metin: Kopyala.
//!
//! Okunan metin düşerken bellekten silinir (2FA anahtarı taşıyabilir).

use zeroize::{Zeroize, Zeroizing};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Totp,
    Url,
    Wifi,
    Email,
    Phone,
    Contact,
    Text,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    Vault,
    Open,
    Copy,
}

pub struct Qr {
    pub kind: Kind,
    pub text: Zeroizing<String>,
}

fn starts(s: &str, p: &str) -> bool {
    s.get(..p.len()).is_some_and(|h| h.eq_ignore_ascii_case(p))
}

pub fn classify(s: &str) -> Kind {
    let s = s.trim();
    // Google Authenticator'ın "Hesapları aktar" QR'ı da 2FA'dır (birden çok hesap).
    if starts(s, "otpauth://totp/") || starts(s, "otpauth-migration://offline?") {
        Kind::Totp
    } else if (starts(s, "https://") || starts(s, "http://")) && !s.contains(char::is_whitespace) {
        Kind::Url
    } else if starts(s, "WIFI:") {
        Kind::Wifi
    } else if starts(s, "mailto:") || starts(s, "MATMSG:") {
        Kind::Email
    } else if starts(s, "tel:") || starts(s, "SMSTO:") || starts(s, "sms:") {
        Kind::Phone
    } else if starts(s, "BEGIN:VCARD") || starts(s, "MECARD:") {
        Kind::Contact
    } else {
        Kind::Text
    }
}

/// `WIFI:T:WPA;S:ağ;P:parola;;` alanı (`\;` gibi kaçışlar çözülür).
fn wifi_field(s: &str, key: char) -> String {
    let body = s.get(5..).unwrap_or("");
    let (mut out, mut cur, mut esc) = (Vec::new(), String::new(), false);
    for c in body.chars() {
        match (esc, c) {
            (true, _) => {
                cur.push(c);
                esc = false;
            }
            (false, '\\') => esc = true,
            (false, ';') => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out.into_iter().find_map(|f| f.strip_prefix(&format!("{key}:")).map(str::to_string)).unwrap_or_default()
}

fn short(s: &str, n: usize) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > n {
        format!("{}…", one.chars().take(n).collect::<String>())
    } else {
        one
    }
}

impl Qr {
    /// Düzenleyicide gösterilecek tür adı ve kısa önizleme. 2FA'da anahtar
    /// gösterilmez, yalnızca sağlayıcı ve hesap.
    pub fn title(&self) -> String {
        use carlui_core::t;
        let s = self.text.trim();
        match self.kind {
            Kind::Totp if starts(s, "otpauth-migration://") => t!("2FA · Google Authenticator aktarımı", "2FA · Google Authenticator export").into(),
            Kind::Totp => {
                let label = s.get(15..).unwrap_or("").split('?').next().unwrap_or("");
                let label = label.replace("%20", " ").replace("%40", "@").replace("%3A", ":").replace("%3a", ":");
                format!("{} · {}", t!("2FA kodu", "2FA code"), short(&label, 40))
            }
            Kind::Url => {
                let host = s.split("://").nth(1).and_then(|r| r.split(['/', '?', '#']).next()).unwrap_or("");
                format!("{} · {}", t!("Bağlantı", "Link"), short(host, 48))
            }
            Kind::Wifi => format!("Wi-Fi · {}", short(&wifi_field(s, 'S'), 40)),
            Kind::Email => format!("{} · {}", t!("E-posta", "Email"), short(s.trim_start_matches("mailto:").trim_start_matches("MATMSG:TO:"), 40)),
            Kind::Phone => format!("{} · {}", t!("Telefon", "Phone"), short(s.split(':').nth(1).unwrap_or(""), 30)),
            Kind::Contact => t!("Kişi kartı", "Contact card").into(),
            Kind::Text => format!("{} · {}", t!("Metin", "Text"), short(s, 48)),
        }
    }

    /// Türe göre düğmeler: (eylem, yazı).
    pub fn actions(&self) -> Vec<(Act, &'static str)> {
        use carlui_core::t;
        match self.kind {
            Kind::Totp => vec![(Act::Vault, "Carl Vault")],
            Kind::Url => vec![(Act::Open, t!("Aç", "Open")), (Act::Copy, t!("Kopyala", "Copy"))],
            Kind::Wifi => vec![(Act::Copy, t!("Parolayı kopyala", "Copy password"))],
            _ => vec![(Act::Copy, t!("Kopyala", "Copy"))],
        }
    }

    /// Kopyala'da panoya giden metin (Wi-Fi'da yalnızca parola).
    pub fn copy_text(&self) -> Zeroizing<String> {
        match self.kind {
            Kind::Wifi => Zeroizing::new(wifi_field(self.text.trim(), 'P')),
            Kind::Email => Zeroizing::new(self.text.trim().trim_start_matches("mailto:").to_string()),
            Kind::Phone => Zeroizing::new(self.text.trim().split_once(':').map(|(_, n)| n.split(':').next().unwrap_or(n)).unwrap_or("").to_string()),
            _ => Zeroizing::new(self.text.trim().to_string()),
        }
    }
}

/// En çok bu kadar piksel taranır; büyük seçim küçültülür (QR ekranda iridir).
const MAX_PIXELS: u64 = 2_500_000;

/// BGRA karedeki (x0,y0)-(x1,y1) alanında QR arar.
pub fn scan(px: &[u8], w: u32, h: u32, area: (u32, u32, u32, u32)) -> Vec<Qr> {
    let (x0, y0) = (area.0.min(w), area.1.min(h));
    let (x1, y1) = (area.2.min(w), area.3.min(h));
    if x1 <= x0 + 20 || y1 <= y0 + 20 {
        return Vec::new();
    }
    let (sw, sh) = (x1 - x0, y1 - y0);
    let step = (((sw as u64 * sh as u64) as f64 / MAX_PIXELS as f64).sqrt().ceil() as u32).max(1);
    let (gw, gh) = ((sw / step) as usize, (sh / step) as usize);
    let stride = w as usize * 4;
    let mut img = rqrr::PreparedImage::prepare_from_greyscale(gw, gh, |x, y| {
        let i = (y0 as usize + y * step as usize) * stride + (x0 as usize + x * step as usize) * 4;
        let (b, g, r) = (px[i] as u32, px[i + 1] as u32, px[i + 2] as u32);
        ((r * 77 + g * 150 + b * 29) >> 8) as u8
    });
    let mut out: Vec<Qr> = Vec::new();
    for grid in img.detect_grids() {
        let mut raw = Zeroizing::new(Vec::new());
        if grid.decode_to(&mut *raw).is_err() {
            continue;
        }
        // Baytlar kopyalanmadan metne taşınır; UTF-8 değilse silinip atlanır.
        let text = match String::from_utf8(std::mem::take(&mut *raw)) {
            Ok(s) => Zeroizing::new(s),
            Err(e) => {
                e.into_bytes().zeroize();
                continue;
            }
        };
        if !text.trim().is_empty() && !out.iter().any(|q| *q.text == *text) {
            out.push(Qr { kind: classify(&text), text });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qr(s: &str) -> Qr {
        Qr { kind: classify(s), text: Zeroizing::new(s.into()) }
    }

    #[test]
    fn kinds() {
        assert_eq!(classify("otpauth://totp/GitHub:carl?secret=JBSWY3DPEHPK3PXP"), Kind::Totp);
        assert_eq!(classify("OTPAUTH://TOTP/x?secret=A"), Kind::Totp);
        assert_eq!(classify("otpauth://hotp/x?secret=A"), Kind::Text, "HOTP desteklenmiyor, metin sayılır");
        assert_eq!(classify("otpauth-migration://offline?data=CjEKCkhlbGxvId6tvu8%3D"), Kind::Totp);
        assert_eq!(classify("https://github.com/login"), Kind::Url);
        assert_eq!(classify("https://a.com bu bir cümle"), Kind::Text);
        assert_eq!(classify("WIFI:T:WPA;S:Ev;P:gizli;;"), Kind::Wifi);
        assert_eq!(classify("mailto:carl@ornek.com"), Kind::Email);
        assert_eq!(classify("tel:+905551112233"), Kind::Phone);
        assert_eq!(classify("BEGIN:VCARD\nVERSION:3.0"), Kind::Contact);
        assert_eq!(classify("merhaba dünya"), Kind::Text);
    }

    #[test]
    fn titles_and_copy() {
        let t = qr("otpauth://totp/GitHub:carl%40ornek.com?secret=JBSWY3DPEHPK3PXP&issuer=GitHub");
        assert!(t.title().ends_with("GitHub:carl@ornek.com"), "{}", t.title());
        assert!(!t.title().contains("JBSW"), "anahtar görünmez");
        assert_eq!(t.actions()[0].0, Act::Vault);
        let w = qr(r"WIFI:T:WPA;S:Ev\;Ağı;P:pa\;rola;;");
        assert!(w.title().ends_with("Ev;Ağı"), "{}", w.title());
        assert_eq!(w.copy_text().as_str(), "pa;rola");
        assert!(qr("https://github.com/a?b=1").title().ends_with("github.com"));
        assert_eq!(qr("tel:+905551112233").copy_text().as_str(), "+905551112233");
        assert_eq!(qr("mailto:carl@ornek.com").copy_text().as_str(), "carl@ornek.com");
    }

    /// Koyu temalı bir ekran parçası gibi: gri zemin, ortada beyaz kenarlı QR.
    fn screen_with(text: &str, scale: usize, w: usize, h: usize, at: (usize, usize)) -> Vec<u8> {
        let code = qrcode::QrCode::new(text.as_bytes()).unwrap();
        let n = code.width();
        let colors = code.to_colors();
        let mut px = vec![40u8; w * h * 4];
        let quiet = 4 * scale;
        for y in 0..(n * scale + 2 * quiet) {
            for x in 0..(n * scale + 2 * quiet) {
                let (mx, my) = (x as isize - quiet as isize, y as isize - quiet as isize);
                let dark = mx >= 0 && my >= 0 && (mx as usize) < n * scale && (my as usize) < n * scale && colors[(my as usize / scale) * n + mx as usize / scale] == qrcode::Color::Dark;
                let v = if dark { 0 } else { 255 };
                let i = ((at.1 + y) * w + at.0 + x) * 4;
                px[i..i + 3].fill(v);
            }
        }
        px
    }

    #[test]
    fn decodes_screen_qr() {
        let uri = "otpauth://totp/GitHub:carl?secret=JBSWY3DPEHPK3PXP&issuer=GitHub";
        let (w, h) = (900, 600);
        let px = screen_with(uri, 4, w, h, (300, 150));
        let found = scan(&px, w as u32, h as u32, (0, 0, w as u32, h as u32));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, Kind::Totp);
        assert_eq!(found[0].text.as_str(), uri);
        // Seçim QR'ı kesmiyorsa bulunmaz.
        assert!(scan(&px, w as u32, h as u32, (0, 0, 250, 600)).is_empty());
    }

    #[test]
    fn large_selection_is_downscaled() {
        // 4K seçim küçültülse de ekran boyutundaki QR okunur.
        let (w, h) = (3840, 2160);
        let px = screen_with("https://github.com/login", 8, w, h, (1500, 800));
        let found = scan(&px, w as u32, h as u32, (0, 0, w as u32, h as u32));
        assert_eq!(found.first().map(|q| q.kind), Some(Kind::Url));
    }

    #[test]
    fn empty_or_tiny_area() {
        let px = vec![255u8; 100 * 100 * 4];
        assert!(scan(&px, 100, 100, (0, 0, 10, 10)).is_empty());
        assert!(scan(&px, 100, 100, (0, 0, 100, 100)).is_empty());
    }
}
