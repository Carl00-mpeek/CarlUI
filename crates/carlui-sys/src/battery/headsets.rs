//! Kablosuz kulaklık alıcıları: SteelSeries Arctis / Nova ve HyperX modelleri.
//!
//! Her model tek bir soru-cevap (ya da kısa bir dizi) ile pilini söyler. Bayt
//! düzenleri HeadsetControl projesinin cihaz notlarından alınmıştır; kod
//! bağımsız yazılmıştır. Cevap dizinleri hidapi görünümündedir: rapor kimliği
//! 0 olan cihazlarda baştaki sıfır atılır (`view`).

use super::*;

/// Hangi HID koleksiyonuna yazılacağı.
#[derive(Clone, Copy)]
enum Sel {
    /// Kullanım sayfası / kullanım.
    Usage(u16, u16),
    /// USB arayüz numarası (`mi_XX`); üreticiye özel sayfa tercih edilir.
    Mi(u8),
}

/// Bir cevabın yorumu: `None` başka rapor, `Some(None)` kulaklık kapalı.
type Parse = fn(&[u8]) -> Option<Option<(u8, bool)>>;

struct Model {
    vid: u16,
    pids: &'static [u16],
    name: &'static str,
    sel: Sel,
    /// Sırayla sorulan istekler; her biri kendi yorumlayıcısıyla. Son isteğin
    /// sonucu yüzdedir; öncekiler yalnızca "kapalı mı / şarjda mı" söyler.
    steps: &'static [(&'static [u8], Parse)],
}

/// Rapor kimliği 0 ise hidapi gibi baştaki sıfırı at.
fn view(raw: &[u8]) -> &[u8] {
    match raw.first() {
        Some(0) => &raw[1..],
        _ => raw,
    }
}

fn map(v: u8, lo: u8, hi: u8) -> u8 {
    if hi <= lo {
        return v.min(100);
    }
    ((v.clamp(lo, hi) - lo) as u32 * 100 / (hi - lo) as u32) as u8
}

// ---- SteelSeries ---------------------------------------------------------------

const SS: u16 = 0x1038;

fn arctis1(r: &[u8]) -> Option<Option<(u8, bool)>> {
    let r = view(r);
    (r.len() > 3).then(|| (r[2] != 0x01).then(|| (r[3].min(100), false)))
}

fn arctis7(r: &[u8]) -> Option<Option<(u8, bool)>> {
    let r = view(r);
    (r.len() > 2).then(|| Some((r[2].min(100), false)))
}

fn arctis9(r: &[u8]) -> Option<Option<(u8, bool)>> {
    let r = view(r);
    (r.len() > 4).then(|| Some((map(r[3], 0x64, 0x9A), r[4] == 0x01)))
}

/// Yeni nesil (0xB0 durum sorusu) cevabı `b0 ...` ile başlar.
fn b0(r: &[u8]) -> Option<&[u8]> {
    let r = view(r);
    (r.len() > 4 && r[0] == 0xB0).then_some(r)
}

fn arctis7plus(r: &[u8]) -> Option<Option<(u8, bool)>> {
    let r = b0(r)?;
    Some((r[1] != 0x01).then(|| (map(r[2], 0, 4), r[3] == 0x01)))
}

fn nova7_discrete(r: &[u8]) -> Option<Option<(u8, bool)>> {
    let r = b0(r)?;
    Some((r[3] != 0x00).then(|| (map(r[2], 0, 4), matches!(r[3], 1 | 2))))
}

fn nova7(r: &[u8]) -> Option<Option<(u8, bool)>> {
    let r = b0(r)?;
    Some((r[3] != 0x00).then(|| (r[2].min(100), matches!(r[3], 1 | 2))))
}

fn nova5(r: &[u8]) -> Option<Option<(u8, bool)>> {
    let r = b0(r)?;
    Some((r[1] != 0x02).then(|| (r[3].min(100), r[4] == 0x01)))
}

fn nova_pro(r: &[u8]) -> Option<Option<(u8, bool)>> {
    let r = view(r);
    if r.len() < 16 || r.get(1) != Some(&0xB0) {
        return None;
    }
    Some((r[15] != 0x01).then(|| (map(r[6], 0, 8), r[15] == 0x02)))
}

// ---- HyperX --------------------------------------------------------------------

const HP: u16 = 0x03F0;

/// Cloud II Wireless: `06 ff bb <komut>` cevabı aynı başlıkla gelir.
fn cloud2_charging(r: &[u8]) -> Option<Option<(u8, bool)>> {
    (r.len() > 4 && r[..4] == [0x06, 0xFF, 0xBB, 0x03]).then(|| Some((0, r[4] == 1)))
}

fn cloud2_level(r: &[u8]) -> Option<Option<(u8, bool)>> {
    (r.len() > 7 && r[..4] == [0x06, 0xFF, 0xBB, 0x02]).then(|| Some((r[7].min(100), false)))
}

/// Cloud Alpha Wireless: `21 bb <komut>`; 0x03 bağlantı, 0x0c şarj, 0x0b yüzde.
fn alpha_conn(r: &[u8]) -> Option<Option<(u8, bool)>> {
    (r.len() > 3 && r[..3] == [0x21, 0xBB, 0x03]).then(|| (r[3] != 0x01).then_some((0, false)))
}

fn alpha_charging(r: &[u8]) -> Option<Option<(u8, bool)>> {
    (r.len() > 3 && r[..3] == [0x21, 0xBB, 0x0C]).then(|| Some((0, r[3] == 0x01)))
}

fn alpha_level(r: &[u8]) -> Option<Option<(u8, bool)>> {
    (r.len() > 3 && r[..3] == [0x21, 0xBB, 0x0B]).then(|| Some((r[3].min(100), false)))
}

static MODELS: &[Model] = &[
    Model { vid: SS, pids: &[0x12B3, 0x12B6, 0x12D7, 0x12D5], name: "SteelSeries Arctis 1", sel: Sel::Usage(0xFF43, 0x0202), steps: &[(&[0x06, 0x12], arctis1)] },
    Model { vid: SS, pids: &[0x1260, 0x12AD, 0x1252, 0x1280], name: "SteelSeries Arctis 7", sel: Sel::Usage(0xFF43, 0x0202), steps: &[(&[0x06, 0x18], arctis7)] },
    Model { vid: SS, pids: &[0x12C2, 0x12C4], name: "SteelSeries Arctis 9", sel: Sel::Mi(0), steps: &[(&[0x00, 0x20], arctis9)] },
    Model { vid: SS, pids: &[0x220E, 0x2212, 0x2216, 0x2236], name: "SteelSeries Arctis 7+", sel: Sel::Usage(0xFFC0, 1), steps: &[(&[0x00, 0xB0], arctis7plus)] },
    Model {
        vid: SS,
        pids: &[0x2202, 0x2206, 0x220A, 0x223A, 0x227A, 0x22A4],
        name: "SteelSeries Arctis Nova 7",
        sel: Sel::Usage(0xFFC0, 1),
        steps: &[(&[0x00, 0xB0], nova7_discrete)],
    },
    Model {
        vid: SS,
        pids: &[0x22A1, 0x227E, 0x2258, 0x229E, 0x22AD, 0x22A9, 0x22A5, 0x22A7, 0x2298],
        name: "SteelSeries Arctis Nova 7",
        sel: Sel::Usage(0xFFC0, 1),
        steps: &[(&[0x00, 0xB0], nova7)],
    },
    Model { vid: SS, pids: &[0x2232, 0x2253], name: "SteelSeries Arctis Nova 5", sel: Sel::Usage(0xFFC0, 1), steps: &[(&[0x00, 0xB0], nova5)] },
    Model { vid: SS, pids: &[0x2269, 0x226D], name: "SteelSeries Arctis Nova 3", sel: Sel::Usage(0xFFC0, 1), steps: &[(&[0x00, 0xB0], nova5)] },
    Model { vid: SS, pids: &[0x12E0, 0x12E5], name: "SteelSeries Arctis Nova Pro", sel: Sel::Mi(4), steps: &[(&[0x06, 0xB0], nova_pro)] },
    Model {
        vid: HP,
        pids: &[0x0696, 0x018B],
        name: "HyperX Cloud II Wireless",
        sel: Sel::Usage(0xFF90, 0x0303),
        steps: &[(&[0x06, 0xFF, 0xBB, 0x03, 0x00], cloud2_charging), (&[0x06, 0xFF, 0xBB, 0x02, 0x00], cloud2_level)],
    },
    Model {
        vid: HP,
        pids: &[0x098D],
        name: "HyperX Cloud Alpha Wireless",
        sel: Sel::Mi(0),
        steps: &[(&[0x21, 0xBB, 0x03], alpha_conn), (&[0x21, 0xBB, 0x0C], alpha_charging), (&[0x21, 0xBB, 0x0B], alpha_level)],
    },
];

/// Takılı ve tabloda bulunan her kulaklık için bir okuma. Taranan VID'ler
/// genel HID taramasının dışında tutulmak üzere `seen`e eklenir.
pub(super) fn read(seen: &mut Vec<String>) -> Vec<Reading> {
    let mut out = Vec::new();
    let mut vids: Vec<u16> = MODELS.iter().map(|m| m.vid).collect();
    vids.dedup();
    for vid in vids {
        let all = interfaces(&format!("vid_{vid:04x}"));
        for m in MODELS.iter().filter(|m| m.vid == vid) {
            for &pid in m.pids {
                let tag = format!("pid_{pid:04x}");
                let mine: Vec<&Iface> = all.iter().filter(|i| i.path.to_lowercase().contains(&tag)).collect();
                let Some(i) = pick(&mine, m.sel) else { continue };
                seen.push(format!("vid_{vid:04x}&{tag}"));
                let (percent, charging) = ask_model(i, m).map_or((None, false), |(p, c)| (Some(p), c));
                out.push(Reading { kind: Kind::Headset, name: m.name.into(), percent, charging, charge_known: false });
            }
        }
    }
    out
}

fn pick<'a>(cands: &[&'a Iface], sel: Sel) -> Option<&'a Iface> {
    match sel {
        Sel::Usage(p, u) => cands.iter().find(|i| i.page == p && i.usage == u).copied(),
        Sel::Mi(n) => {
            let tag = format!("mi_{n:02x}");
            let on: Vec<&&Iface> = cands.iter().filter(|i| {
                let p = i.path.to_lowercase();
                p.contains(&tag) || (n == 0 && !p.contains("mi_"))
            }).filter(|i| i.out_len > 0 && i.in_len > 0).collect();
            on.iter().find(|i| i.page >= 0xFF00).or(on.first()).map(|i| **i)
        }
    }
}

/// Modelin adımlarını yürütür: (yüzde, şarjda) ya da `None` (kapalı / cevap yok).
fn ask_model(i: &Iface, m: &Model) -> Option<(u8, bool)> {
    let d = Dev::open(&i.path)?;
    d.drain(i.in_len);
    let mut charging = false;
    let mut pct = 0;
    for (req, parse) in m.steps {
        let (p, c) = d.ask(&padded(req, i.out_len), i.in_len, 1000, parse)??; // ikinci `?`: kulaklık kapalı
        charging |= c;
        pct = p; // son adımın değeri yüzdedir
    }
    Some((pct, charging))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn olcekler() {
        assert_eq!(map(2, 0, 4), 50);
        assert_eq!(map(0x9A, 0x64, 0x9A), 100);
        assert_eq!(map(0x10, 0x64, 0x9A), 0);
    }

    #[test]
    fn nova_cevabi() {
        // Rapor kimliği 0: Windows'ta baştaki sıfır gelir.
        assert_eq!(nova5(&[0, 0xB0, 0x00, 0x00, 72, 1]), Some(Some((72, true))));
        assert_eq!(nova5(&[0, 0xB0, 0x02, 0x00, 72, 1]), Some(None));
        assert_eq!(nova5(&[0, 0x37, 0x00]), None);
        assert_eq!(nova7_discrete(&[0, 0xB0, 0, 3, 3, 0]), Some(Some((75, false))));
    }

    #[test]
    fn hyperx_basliklari() {
        assert_eq!(cloud2_level(&[0x06, 0xFF, 0xBB, 0x02, 0, 0, 0, 64]), Some(Some((64, false))));
        assert_eq!(cloud2_level(&[0x06, 0xFF, 0xBB, 0x03, 0, 0, 0, 64]), None);
        assert_eq!(alpha_conn(&[0x21, 0xBB, 0x03, 0x01]), Some(None));
    }
}
