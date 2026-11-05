//! Bu modül "PDF'yi Acrobat'a gerek kalmadan değiştirelim" sorununu çözüyor:
//! sayfa döndürme, silme, taşıma, başka PDF'yi sona ekleme (birleştirme),
//! bir sayfayı ayrı dosyaya çıkarma, sayfaya metin yazma ve sayfa metnini
//! okuma. Hepsi bayt alıp bayt döndürür; görünüm geri alma için eski baytları
//! saklar. Şifreli dosyalar düzenlenmez (şifre kaydedilirken kaybolurdu).

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

type Res<T> = Result<T, String>;

/// Sayfa ağacında üst düğümden miras alınabilen alanlar.
const INHERIT: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub fn load(bytes: &[u8]) -> Res<Document> {
    let d = Document::load_mem(bytes).map_err(err)?;
    if d.was_encrypted() || d.is_encrypted() {
        return Err(carlui_core::t!("Şifreli PDF düzenlenemez.", "Encrypted PDFs can't be edited.").into());
    }
    Ok(d)
}

pub fn save(d: &mut Document) -> Res<Vec<u8>> {
    d.prune_objects();
    let mut out = Vec::new();
    d.save_to(&mut out).map_err(err)?;
    Ok(out)
}

fn pages(d: &Document) -> Vec<ObjectId> {
    d.get_pages().into_values().collect()
}

fn page_id(d: &Document, i: usize) -> Res<ObjectId> {
    pages(d).get(i).copied().ok_or_else(|| "sayfa yok".to_string())
}

/// Sayfada ya da üst düğümlerinde aranan alan (miras kuralı).
fn inherited(d: &Document, id: ObjectId, key: &[u8]) -> Option<Object> {
    let mut cur = Some(id);
    for _ in 0..64 {
        let dict = d.get_dictionary(cur?).ok()?;
        if let Ok(v) = dict.get(key) {
            return Some(v.clone());
        }
        cur = dict.get(b"Parent").and_then(Object::as_reference).ok();
    }
    None
}

fn number(d: &Document, o: &Object) -> f32 {
    // Başvuru zinciri sınırlı: kendini gösteren nesne (5 0 R → 5 0 R) sonsuz döngü olmasın.
    let mut o = o;
    for _ in 0..32 {
        match o {
            Object::Integer(i) => return *i as f32,
            Object::Real(r) => return *r,
            Object::Reference(id) => match d.get_object(*id) {
                Ok(n) => o = n,
                Err(_) => return 0.0,
            },
            _ => return 0.0,
        }
    }
    0.0
}

/// Sayfanın görünen kutusu (CropBox, yoksa MediaBox): x0, y0, x1, y1.
fn page_box(d: &Document, id: ObjectId) -> [f32; 4] {
    let b = inherited(d, id, b"CropBox").or_else(|| inherited(d, id, b"MediaBox"));
    let arr = match b {
        Some(Object::Array(a)) => a,
        Some(Object::Reference(r)) => d.get_object(r).ok().and_then(|o| o.as_array().ok().cloned()).unwrap_or_default(),
        _ => Vec::new(),
    };
    if arr.len() < 4 {
        return [0.0, 0.0, 612.0, 792.0];
    }
    let v: Vec<f32> = arr.iter().map(|o| number(d, o)).collect();
    [v[0].min(v[2]), v[1].min(v[3]), v[0].max(v[2]), v[1].max(v[3])]
}

fn rotation(d: &Document, id: ObjectId) -> i64 {
    let r = inherited(d, id, b"Rotate").map(|o| number(d, &o) as i64).unwrap_or(0);
    r.rem_euclid(360) / 90 * 90
}

#[cfg(test)]
pub fn page_count(bytes: &[u8]) -> Res<usize> {
    Ok(load(bytes)?.get_pages().len())
}

/// Sayfayı saat yönünde 90° döndürür.
pub fn rotate(bytes: &[u8], i: usize) -> Res<Vec<u8>> {
    let mut d = load(bytes)?;
    let id = page_id(&d, i)?;
    let r = (rotation(&d, id) + 90) % 360;
    d.get_dictionary_mut(id).map_err(err)?.set("Rotate", r);
    save(&mut d)
}

pub fn delete(bytes: &[u8], i: usize) -> Res<Vec<u8>> {
    let mut d = load(bytes)?;
    if d.get_pages().len() <= 1 {
        return Err(carlui_core::t!("Son sayfa silinemez.", "The last page can't be deleted.").into());
    }
    page_id(&d, i)?;
    d.delete_pages(&[i as u32 + 1]);
    save(&mut d)
}

/// Sayfa ağacını tek düzeye indirir: miras alanlar sayfalara kopyalanır,
/// bütün sayfalar kök düğümün çocuğu olur. Döndürülen kök kimliğidir.
fn flatten(d: &mut Document, order: &[ObjectId]) -> Res<ObjectId> {
    let root = d.catalog().map_err(err)?.get(b"Pages").and_then(Object::as_reference).map_err(err)?;
    for &id in order {
        let fill: Vec<(&[u8], Object)> = INHERIT.iter().filter_map(|k| inherited(d, id, k).map(|v| (*k, v))).collect();
        let page = d.get_dictionary_mut(id).map_err(err)?;
        for (k, v) in fill {
            if !page.has(k) {
                page.set(k.to_vec(), v);
            }
        }
        page.set("Parent", root);
    }
    let kids: Vec<Object> = order.iter().map(|&id| Object::Reference(id)).collect();
    let n = kids.len() as i64;
    let node = d.get_dictionary_mut(root).map_err(err)?;
    node.set("Kids", kids);
    node.set("Count", n);
    for k in INHERIT {
        node.remove(k);
    }
    Ok(root)
}

/// `i`. sayfayı `to` konumuna taşır.
pub fn move_page(bytes: &[u8], i: usize, to: usize) -> Res<Vec<u8>> {
    let mut d = load(bytes)?;
    let mut order = pages(&d);
    if i >= order.len() || to >= order.len() {
        return Err("sayfa yok".into());
    }
    let p = order.remove(i);
    order.insert(to, p);
    flatten(&mut d, &order)?;
    save(&mut d)
}

/// `other`ın bütün sayfalarını sona ekler.
pub fn merge(bytes: &[u8], other: &[u8]) -> Res<Vec<u8>> {
    let mut d = load(bytes)?;
    let mut o = load(other)?;
    o.renumber_objects_with(d.max_id + 1);
    let mut extra = pages(&o);
    // Ötekinin sayfalarındaki miras alanları sayfanın kendisine taşı.
    for &id in &extra {
        let fill: Vec<(&[u8], Object)> = INHERIT.iter().filter_map(|k| inherited(&o, id, k).map(|v| (*k, v))).collect();
        if let Ok(page) = o.get_dictionary_mut(id) {
            for (k, v) in fill {
                if !page.has(k) {
                    page.set(k.to_vec(), v);
                }
            }
        }
    }
    d.max_id = d.max_id.max(o.max_id);
    d.objects.extend(o.objects);
    let mut order = pages(&d);
    order.append(&mut extra);
    flatten(&mut d, &order)?;
    save(&mut d)
}

/// Yalnızca `i`. sayfadan oluşan yeni PDF.
pub fn extract(bytes: &[u8], i: usize) -> Res<Vec<u8>> {
    let mut d = load(bytes)?;
    let keep = page_id(&d, i)?;
    flatten(&mut d, &[keep])?;
    save(&mut d)
}

pub fn text(bytes: &[u8], i: usize) -> Res<String> {
    let d = load(bytes)?;
    d.extract_text(&[i as u32 + 1]).map_err(err)
}

/// PDF dizgesi: parantez ve ters bölü kaçışlı, Türkçe Windows-1254 kodlu.
fn pdf_string(s: &str) -> Vec<u8> {
    let (enc, _, _) = encoding_rs::WINDOWS_1254.encode(s);
    let mut out = vec![b'('];
    for &b in enc.iter() {
        if matches!(b, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(b);
    }
    out.push(b')');
    out
}

/// Görünen sayfada (döndürülmüş, sol üst köşe başlangıç, 0..1) noktayı sayfa
/// koordinatına çevirir.
fn to_page(bx: [f32; 4], rot: i64, u: f32, v: f32) -> (f32, f32) {
    // s, t: döndürülmemiş sayfada soldan / üstten oran.
    let (s, t) = match rot {
        90 => (v, 1.0 - u),
        180 => (1.0 - u, 1.0 - v),
        270 => (1.0 - v, u),
        _ => (u, v),
    };
    (bx[0] + s * (bx[2] - bx[0]), bx[3] - t * (bx[3] - bx[1]))
}

/// Görünen sayfada (`u`, `v`) noktasına yazı ekler (metnin sol üstü).
/// `size` punto, `rgb` 0xRRGGBB. Satırlar `\n` ile ayrılır.
pub fn add_text(bytes: &[u8], i: usize, u: f32, v: f32, text: &str, size: f32, rgb: u32) -> Res<Vec<u8>> {
    let mut d = load(bytes)?;
    let id = page_id(&d, i)?;
    let bx = page_box(&d, id);
    let rot = rotation(&d, id);
    // Görünen sayfanın yüksekliği (punto): ilk satırın taban çizgisine in.
    let disp_h = if rot % 180 == 0 { bx[3] - bx[1] } else { bx[2] - bx[0] };
    let (x, y) = to_page(bx, rot, u, v + size * 0.82 / disp_h.max(1.0));
    let a = (rot as f32).to_radians();
    let (c, s) = (a.cos(), a.sin());
    let (r, g, b) = (((rgb >> 16) & 255) as f32 / 255.0, ((rgb >> 8) & 255) as f32 / 255.0, (rgb & 255) as f32 / 255.0);

    // Yazı tipi: Helvetica, Türkçe harfler Differences ile.
    let enc = d.add_object(dictionary! {
        "Type" => "Encoding",
        "BaseEncoding" => "WinAnsiEncoding",
        "Differences" => vec![
            208.into(), Object::Name(b"Gbreve".to_vec()),
            221.into(), Object::Name(b"Idotaccent".to_vec()), Object::Name(b"Scedilla".to_vec()),
            240.into(), Object::Name(b"gbreve".to_vec()),
            253.into(), Object::Name(b"dotlessi".to_vec()), Object::Name(b"scedilla".to_vec()),
        ],
    });
    let font = d.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => enc,
    });
    // Kaynakları sayfanın kendisine kopyala (miras ya da paylaşılan sözlük bozulmasın).
    let mut res = match inherited(&d, id, b"Resources") {
        Some(Object::Dictionary(r)) => r,
        Some(Object::Reference(rid)) => d.get_dictionary(rid).cloned().unwrap_or_default(),
        _ => Dictionary::new(),
    };
    let mut fonts = match res.get(b"Font") {
        Ok(Object::Dictionary(f)) => f.clone(),
        Ok(Object::Reference(fid)) => d.get_dictionary(*fid).cloned().unwrap_or_default(),
        _ => Dictionary::new(),
    };
    let mut n = 1;
    while fonts.has(format!("CarlF{n}").as_bytes()) {
        n += 1;
    }
    let fname = format!("CarlF{n}");
    fonts.set(fname.clone(), font);
    res.set("Font", fonts);

    let mut ops = format!("Q q {r:.3} {g:.3} {b:.3} rg BT /{fname} {size:.2} Tf {:.2} TL {c:.4} {s:.4} {:.4} {c:.4} {x:.2} {y:.2} Tm\n", size * 1.2, -s).into_bytes();
    for (li, line) in text.split('\n').enumerate() {
        if li > 0 {
            ops.extend_from_slice(b"T* ");
        }
        ops.extend(pdf_string(line));
        ops.extend_from_slice(b" Tj\n");
    }
    ops.extend_from_slice(b"ET Q\n");

    // Var olan içeriği q…Q içine al: dönüşüm ya da renk bize taşmasın.
    let mut list: Vec<Object> = match d.get_dictionary(id).map_err(err)?.get(b"Contents") {
        Ok(Object::Reference(cid)) => match d.get_object(*cid) {
            Ok(Object::Array(a)) => a.clone(),
            _ => vec![Object::Reference(*cid)],
        },
        Ok(Object::Array(a)) => a.clone(),
        _ => Vec::new(),
    };
    let open = d.add_object(Stream::new(Dictionary::new(), b"q\n".to_vec()));
    let ours = d.add_object(Stream::new(Dictionary::new(), ops));
    list.insert(0, Object::Reference(open));
    list.push(Object::Reference(ours));
    let page = d.get_dictionary_mut(id).map_err(err)?;
    page.set("Contents", list);
    page.set("Resources", res);
    save(&mut d)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `n` sayfalı, her sayfasında "Sayfa i" yazan küçük PDF; sayfalar iki
    /// düzeyli ağaçta, kaynaklar ve MediaBox üst düğümden miras.
    pub fn sample(n: usize) -> Vec<u8> {
        let mut d = Document::with_version("1.5");
        let root = d.new_object_id();
        let mid = d.new_object_id();
        let font = d.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" });
        let mut kids = Vec::new();
        for i in 0..n {
            let c = d.add_object(Stream::new(Dictionary::new(), format!("BT /F1 24 Tf 72 700 Td (Sayfa {}) Tj ET", i + 1).into_bytes()));
            let p = d.add_object(dictionary! { "Type" => "Page", "Parent" => mid, "Contents" => c });
            kids.push(Object::Reference(p));
        }
        d.objects.insert(mid, Object::Dictionary(dictionary! { "Type" => "Pages", "Parent" => root, "Kids" => kids, "Count" => n as i64 }));
        d.objects.insert(
            root,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![Object::Reference(mid)], "Count" => n as i64,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
            }),
        );
        let cat = d.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
        d.trailer.set("Root", cat);
        let mut out = Vec::new();
        d.save_to(&mut out).unwrap();
        out
    }

    fn texts(b: &[u8]) -> Vec<String> {
        let n = page_count(b).unwrap();
        (0..n).map(|i| text(b, i).unwrap().trim().to_string()).collect()
    }

    #[test]
    fn rotate_delete_move() {
        let b = sample(3);
        assert_eq!(texts(&b), ["Sayfa 1", "Sayfa 2", "Sayfa 3"]);
        let r = rotate(&rotate(&b, 1).unwrap(), 1).unwrap();
        let d = load(&r).unwrap();
        assert_eq!(rotation(&d, pages(&d)[1]), 180);
        assert_eq!(rotation(&d, pages(&d)[0]), 0);

        let m = move_page(&r, 2, 0).unwrap();
        assert_eq!(texts(&m), ["Sayfa 3", "Sayfa 1", "Sayfa 2"]);
        let d = load(&m).unwrap();
        // Miras alanlar taşımadan sonra da yerinde: kaynak ve kutu.
        assert_eq!(page_box(&d, pages(&d)[0]), [0.0, 0.0, 612.0, 792.0]);
        assert_eq!(rotation(&d, pages(&d)[2]), 180);

        let x = delete(&m, 1).unwrap();
        assert_eq!(texts(&x), ["Sayfa 3", "Sayfa 2"]);
        let one = delete(&delete(&x, 0).unwrap(), 0);
        assert!(one.is_err(), "son sayfa silinmemeli");
    }

    #[test]
    fn merge_and_extract() {
        let a = sample(2);
        let b = sample(3);
        let m = merge(&a, &b).unwrap();
        assert_eq!(texts(&m), ["Sayfa 1", "Sayfa 2", "Sayfa 1", "Sayfa 2", "Sayfa 3"]);
        let e = extract(&m, 4).unwrap();
        assert_eq!(texts(&e), ["Sayfa 3"]);
        assert!(e.len() < m.len());
    }

    #[test]
    fn add_text_turkish() {
        let b = sample(1);
        let t = add_text(&b, 0, 0.1, 0.1, "Çağrı şöyle: İyi (gün)", 14.0, 0xCC0000).unwrap();
        let d = load(&t).unwrap();
        let id = pages(&d)[0];
        let content = d.get_page_content(id);
        let s = String::from_utf8_lossy(&content);
        assert!(s.contains("/CarlF1 14.00 Tf"), "{s}");
        assert!(content.windows(2).any(|w| w == b"\\("), "parantez kaçışlı olmalı");
        // ğ = 0xF0, ş = 0xFE, İ = 0xDD (Windows-1254).
        assert!(content.contains(&0xF0) && content.contains(&0xFE) && content.contains(&0xDD));
        // Önceki metin hâlâ okunur, sayfa kaynakları korunur.
        assert!(text(&t, 0).unwrap().contains("Sayfa 1"));
        // Döndürülmüş sayfada görünen sol üst, sayfanın sol altına düşer (90°).
        let (x, y) = to_page([0.0, 0.0, 612.0, 792.0], 90, 0.0, 0.0);
        assert!((x - 0.0).abs() < 0.01 && (y - 0.0).abs() < 0.01, "{x},{y}");
        let (x, y) = to_page([0.0, 0.0, 612.0, 792.0], 0, 0.0, 0.0);
        assert!((x - 0.0).abs() < 0.01 && (y - 792.0).abs() < 0.01);
    }

    /// Kötü niyetli PDF: kendini gösteren Rotate/MediaBox, döngülü sayfa ağacı,
    /// çok derin iç içe dizi. Hiçbiri takılmamalı ya da çökmemeli.
    #[test]
    fn hostile_pdf() {
        let mut d = lopdf::Document::load_mem(&sample(2)).unwrap();
        let id = pages(&d)[0];
        let me = d.new_object_id();
        d.objects.insert(me, Object::Reference(me));
        let bx = d.new_object_id();
        d.objects.insert(bx, Object::Reference(bx));
        let p = d.get_dictionary_mut(id).unwrap();
        p.set("Rotate", Object::Reference(me));
        p.set("MediaBox", Object::Reference(bx));
        // Sayfanın üst düğümü kendisi: miras ararken döngü.
        p.set("Parent", Object::Reference(id));
        let mut b = Vec::new();
        d.save_to(&mut b).unwrap();
        let _ = crate::guard::deep(|| (rotate(&b, 0), add_text(&b, 0, 0.5, 0.5, "x", 12.0, 0), move_page(&b, 0, 1), text(&b, 0)));

        // 100 bin düzey iç içe dizi.
        let mut raw = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R /X ".to_vec();
        raw.extend(std::iter::repeat_n(b'[', 100_000));
        raw.extend(std::iter::repeat_n(b']', 100_000));
        raw.extend_from_slice(b" >>\nendobj\n2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\ntrailer\n<< /Root 1 0 R >>\n%%EOF\n");
        let _ = crate::guard::deep(|| rotate(&raw, 0));
    }

    /// Gerçek dosyayla: `CARL_PDF` verilirse bütün işlemler hata vermeden çalışmalı.
    #[test]
    fn real_file_if_given() {
        let Ok(p) = std::env::var("CARL_PDF") else { return };
        let b = std::fs::read(p).unwrap();
        let n = page_count(&b).unwrap();
        let r = rotate(&b, 0).unwrap();
        assert_eq!(page_count(&r).unwrap(), n);
        let t = add_text(&r, 0, 0.2, 0.2, "Deneme ğüşiöç", 12.0, 0).unwrap();
        assert_eq!(page_count(&t).unwrap(), n);
        if n > 1 {
            assert_eq!(page_count(&delete(&t, n - 1).unwrap()).unwrap(), n - 1);
            assert_eq!(page_count(&move_page(&t, 0, n - 1).unwrap()).unwrap(), n);
        }
        assert_eq!(page_count(&extract(&t, 0).unwrap()).unwrap(), 1);
        assert_eq!(page_count(&merge(&t, &b).unwrap()).unwrap(), 2 * n);
    }
}
