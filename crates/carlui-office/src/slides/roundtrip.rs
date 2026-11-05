//! Bu modül "PowerPoint dosyasını açıp düzenleyip kaydedince bozulmadan geri
//! okunuyor mu" sorununu çözüyor: elle kurulmuş küçük bir sunu üstünde metin
//! düzenleme, metin kutusu ekleme, slayt çoğaltma/ekleme/silme ve gizleme
//! sonrası kaydedip yeniden yükleyerek sonucu denetler.

use super::{pptx, save};
use crate::rich::{type_text, Cursor, Pos};
use crate::zip::Zip;

const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn rels(items: &[(&str, &str, &str)]) -> Vec<u8> {
    let body: String = items.iter().map(|(id, ty, t)| format!(r#"<Relationship Id="{id}" Type="{REL}/{ty}" Target="{t}"/>"#)).collect();
    format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{body}</Relationships>"#).into_bytes()
}

fn sp(id: u32, ph: &str, text: &str) -> String {
    format!(r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="S{id}"/><p:cNvSpPr/><p:nvPr>{ph}</p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="457200" y="274638"/><a:ext cx="8229600" cy="1143000"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="tr-TR" b="1"/><a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp>"#)
}

fn tree(inner: &str) -> String {
    format!(r#"<p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{inner}</p:spTree></p:cSld>"#)
}

/// En küçük geçerli sunu: bir asıl, bir düzen, bir tema, iki slayt.
fn sample() -> Vec<u8> {
    let mut z = Zip::new();
    z.set("[Content_Types].xml", br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slides/slide1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/><Override PartName="/ppt/slides/slide2.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/></Types>"#.to_vec());
    z.set("_rels/.rels", rels(&[("rId1", "officeDocument", "ppt/presentation.xml")]));
    z.set("ppt/presentation.xml", format!(r#"<p:presentation {NS}><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:sldIdLst><p:sldId id="256" r:id="rId2"/><p:sldId id="257" r:id="rId3"/></p:sldIdLst><p:sldSz cx="9144000" cy="5143500"/></p:presentation>"#).into_bytes());
    z.set("ppt/_rels/presentation.xml.rels", rels(&[("rId1", "slideMaster", "slideMasters/slideMaster1.xml"), ("rId2", "slide", "slides/slide1.xml"), ("rId3", "slide", "slides/slide2.xml"), ("rId4", "theme", "theme/theme1.xml")]));
    z.set("ppt/theme/theme1.xml", br#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="T"><a:themeElements><a:clrScheme name="C"><a:dk1><a:srgbClr val="000000"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1><a:accent1><a:srgbClr val="4472C4"/></a:accent1></a:clrScheme></a:themeElements></a:theme>"#.to_vec());
    z.set("ppt/slideMasters/slideMaster1.xml", format!(r#"<p:sldMaster {NS}>{}<p:clrMap bg1="lt1" tx1="dk1"/><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst></p:sldMaster>"#, tree("")).into_bytes());
    z.set("ppt/slideMasters/_rels/slideMaster1.xml.rels", rels(&[("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml"), ("rId2", "theme", "../theme/theme1.xml")]));
    z.set("ppt/slideLayouts/slideLayout1.xml", format!(r#"<p:sldLayout {NS} type="obj">{}</p:sldLayout>"#, tree(&sp(2, r#"<p:ph type="title"/>"#, "Başlık ekle"))).into_bytes());
    z.set("ppt/slideLayouts/_rels/slideLayout1.xml.rels", rels(&[("rId1", "slideMaster", "../slideMasters/slideMaster1.xml")]));
    for (n, t) in [(1, "Birinci &amp; slayt"), (2, "İkinci slayt")] {
        z.set(&format!("ppt/slides/slide{n}.xml"), format!(r#"<p:sld {NS}>{}<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#, tree(&sp(2, r#"<p:ph type="title"/>"#, t))).into_bytes());
        z.set(&format!("ppt/slides/_rels/slide{n}.xml.rels"), rels(&[("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml")]));
    }
    z.to_bytes()
}

fn title(d: &super::Deck, i: usize) -> String {
    d.slides[i].shapes.iter().find(|s| s.has_text()).map(|s| s.paras.iter().map(|p| p.text()).collect::<Vec<_>>().join("\n")).unwrap_or_default()
}

#[test]
fn load_edit_save_reload() {
    let mut d = pptx::load(&sample()).unwrap();
    assert_eq!(d.slides.len(), 2);
    assert_eq!(title(&d, 0), "Birinci & slayt");
    assert!((d.w - 720.0).abs() < 0.5 && (d.h - 405.0).abs() < 0.5);

    // Metin düzenle: sona ek yaz.
    {
        let sh = d.slides[0].shapes.iter_mut().find(|s| s.has_text()).unwrap();
        let mut c = Cursor::default();
        let end = sh.paras[0].text().chars().count();
        c.set(Pos { p: 0, c: end }, false);
        type_text(&mut sh.paras, &mut c, " <düzenlendi>\nYeni satır");
        sh.dirty = true;
    }
    // Metin kutusu ekle, ikinci slaytı çoğalt, sonuna boş slayt ekle, ilkini gizle.
    let tb = save::insert_textbox(&mut d, 1, 100.0, 100.0, 200.0, 40.0).unwrap();
    {
        let sh = &mut d.slides[1].shapes[tb];
        let mut c = Cursor::default();
        type_text(&mut sh.paras, &mut c, "Kutu");
        sh.dirty = true;
    }
    save::duplicate(&mut d, 1).unwrap();
    let n = save::new_slide(&mut d, 2).unwrap();
    assert_eq!(n, 3);
    d.slides[0].hidden = true;
    let bytes = save::save(&mut d).unwrap();

    let d2 = pptx::load(&bytes).unwrap();
    assert_eq!(d2.slides.len(), 4);
    assert_eq!(title(&d2, 0), "Birinci & slayt <düzenlendi>\nYeni satır");
    assert!(d2.slides[0].hidden);
    assert!(!d2.slides[1].hidden);
    for i in [1, 2] {
        assert_eq!(title(&d2, i), "İkinci slayt");
        assert!(d2.slides[i].shapes.iter().any(|s| s.paras.iter().any(|p| p.text() == "Kutu")));
    }
    // Biçim korunur: ilk koşunun kalınlığı.
    assert!(d2.zip.read_str(&d2.slides[0].part).unwrap().contains(r#"b="1""#));
    // Her slayt içerik türlerine kayıtlı.
    let ct = d2.zip.read_str("[Content_Types].xml").unwrap();
    for s in &d2.slides {
        assert!(ct.contains(&format!("/{}", s.part)), "{} içerik türünde yok", s.part);
    }

    // Slayt silmek: biri silinince kayıt üç slayta düşer, artık parça kalmaz.
    let mut d3 = pptx::load(&bytes).unwrap();
    d3.slides.remove(1);
    let b3 = save::save(&mut d3).unwrap();
    let d4 = pptx::load(&b3).unwrap();
    assert_eq!(d4.slides.len(), 3);
    assert_eq!(d4.zip.names().filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml")).count(), 3);
}

/// Gerçek bir dosyayla deneme: `CARL_PPTX` değişkeni verilirse yükler,
/// değiştirmeden kaydeder ve yeniden yükler.
#[test]
fn real_file_if_given() {
    let Ok(p) = std::env::var("CARL_PPTX") else { return };
    let bytes = std::fs::read(p).unwrap();
    let mut d = pptx::load(&bytes).unwrap();
    let n = d.slides.len();
    let texts: Vec<String> = (0..n).map(|i| title(&d, i)).collect();
    let out = save::save(&mut d).unwrap();
    let d2 = pptx::load(&out).unwrap();
    assert_eq!(d2.slides.len(), n);
    assert_eq!((0..n).map(|i| title(&d2, i)).collect::<Vec<_>>(), texts);
}
