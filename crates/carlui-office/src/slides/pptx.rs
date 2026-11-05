//! Bu modül "pptx dosyasını okuyup slaytları, şekilleri ve devralınan
//! biçimleri bellekteki sunu modeline çevirelim" sorununu çözüyor. Sıra:
//! presentation.xml → asıllar (tema, renk eşlemesi, metin biçemleri) →
//! slaytlar; her slaytın düzeni gerektiğinde bir kez okunur. Yer tutucular
//! ebeveynlerinden (düzen → asıl) konum, liste düzeyleri ve gövde ayarlarını
//! devralır; slaytın kendi yazdıkları bunların üstüne biner.

use super::*;
use crate::{
    rich::{Run, PAlign},
    sheet::xlsx::rels,
    xml,
};

type Rels = HashMap<String, (String, String)>;

pub fn algn(v: &str) -> Option<PAlign> {
    Some(match v {
        "l" => PAlign::Left,
        "ctr" => PAlign::Center,
        "r" => PAlign::Right,
        "just" | "dist" | "justLow" | "thaiDist" => PAlign::Justify,
        _ => return None,
    })
}

/// `a:rPr` / `a:defRPr` → biçim.
pub fn rpr_fmt(n: &Node, th: &Theme) -> Fmt {
    let flag = |k: &str| n.attr(k).map(|v| v == "1" || v == "true");
    Fmt {
        b: flag("b"),
        i: flag("i"),
        u: n.attr("u").map(|v| v != "none"),
        strike: n.attr("strike").map(|v| v != "noStrike"),
        size: n.attr("sz").and_then(|v| v.parse::<f32>().ok()).map(|v| v / 100.0),
        color: n.child("solidFill").and_then(|f| fill_color(f, th)),
    }
}

/// lstStyle / titleStyle … içindeki `lvlNpPr` öğeleri.
pub fn lst_levels(n: &Node, th: &Theme) -> Levels {
    let mut lv = Levels::default();
    for c in &n.children {
        let name = c.local();
        let Some(i) = name.strip_prefix("lvl").and_then(|r| r.strip_suffix("pPr")).and_then(|d| d.parse::<usize>().ok()) else { continue };
        if !(1..=9).contains(&i) {
            continue;
        }
        let i = i - 1;
        lv.algn[i] = c.attr("algn").and_then(algn);
        lv.mar[i] = emu(c.attr("marL"));
        lv.bullet[i] = bullet_of(c);
        if let Some(d) = c.child("defRPr") {
            lv.fmt[i] = rpr_fmt(d, th);
        }
    }
    lv
}

fn norm_ph(ty: Option<&str>) -> String {
    match ty.unwrap_or("body") {
        "ctrTitle" | "title" => "title",
        "subTitle" | "obj" | "body" => "body",
        t => t,
    }
    .to_string()
}

fn read_ph(sp: &Node, src: &str) -> Option<Ph> {
    let nv = sp.children.iter().find(|c| c.local().starts_with("nv"))?;
    let ph = nv.child("nvPr")?.child("ph")?;
    Some(Ph { ty: norm_ph(ph.attr("type")), idx: ph.attr("idx").and_then(|v| v.parse().ok()), raw: ph.raw(src).to_string() })
}

/// Grup dönüşümü: X = x·a + b, Y = y·c + d.
#[derive(Clone, Copy)]
struct Xf {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
}

const ID: Xf = Xf { a: 1.0, b: 0.0, c: 1.0, d: 0.0 };

fn xfrm_box(x: &Node) -> Option<(f32, f32, f32, f32)> {
    let off = x.child("off")?;
    let ext = x.child("ext");
    Some((emu(off.attr("x"))?, emu(off.attr("y"))?, ext.and_then(|e| emu(e.attr("cx"))).unwrap_or(0.0), ext.and_then(|e| emu(e.attr("cy"))).unwrap_or(0.0)))
}

pub struct Ctx<'a> {
    pub th: &'a Theme,
    pub master: Option<&'a Master>,
    /// Yer tutucu ebeveynleri (düzenin ya da asılın şekilleri).
    pub parents: &'a [Shape],
    /// Yer tutucu olmayan metnin biçemi.
    pub dflt: &'a Levels,
    pub rels: &'a Rels,
    pub src: &'a str,
}

impl Ctx<'_> {
    fn parent(&self, ph: &Ph) -> Option<&Shape> {
        let phs = || self.parents.iter().filter_map(|s| s.ph.as_ref().map(|p| (s, p)));
        if let Some(i) = ph.idx {
            if let Some((s, _)) = phs().find(|(_, p)| p.idx == Some(i) && (p.ty == ph.ty || ph.ty == "body" || p.ty == "body")) {
                return Some(s);
            }
        }
        phs().find(|(_, p)| p.ty == ph.ty).map(|(s, _)| s)
    }

    fn base_levels(&self, ph: Option<&Ph>) -> Levels {
        let m = self.master;
        match ph.map(|p| p.ty.as_str()) {
            Some("title") => m.map(|m| m.title.clone()).unwrap_or_default(),
            Some("body") => m.map(|m| m.body.clone()).unwrap_or_default(),
            Some(_) => m.map(|m| m.other.clone()).unwrap_or_default(),
            None => {
                let mut l = m.map(|m| m.other.clone()).unwrap_or_default();
                l.merge(self.dflt);
                l
            }
        }
    }
}

/// `p:spTree` (ya da grup) içindeki şekiller.
pub fn shapes(tree: &Node, cx: &Ctx, out: &mut Vec<Shape>) {
    walk(tree, cx, ID, false, out);
}

fn walk(tree: &Node, cx: &Ctx, xf: Xf, grouped: bool, out: &mut Vec<Shape>) {
    for n in &tree.children {
        match n.local() {
            "sp" | "cxnSp" | "pic" | "graphicFrame" => {
                if let Some(s) = shape(n, cx, xf, grouped) {
                    out.push(s);
                }
            }
            "grpSp" => {
                let mut inner = xf;
                if let Some(x) = n.path(&["grpSpPr", "xfrm"]) {
                    if let (Some((ox, oy, w, h)), Some(ch)) = (xfrm_box(x), x.child("chOff")) {
                        let ce = x.child("chExt");
                        let (chx, chy) = (emu(ch.attr("x")).unwrap_or(0.0), emu(ch.attr("y")).unwrap_or(0.0));
                        let (cw, chh) = (ce.and_then(|e| emu(e.attr("cx"))).unwrap_or(w), ce.and_then(|e| emu(e.attr("cy"))).unwrap_or(h));
                        let ga = if cw > 0.0 { w / cw } else { 1.0 };
                        let gc = if chh > 0.0 { h / chh } else { 1.0 };
                        let (gb, gd) = (ox - chx * ga, oy - chy * gc);
                        inner = Xf { a: xf.a * ga, b: xf.a * gb + xf.b, c: xf.c * gc, d: xf.c * gd + xf.d };
                    }
                }
                walk(n, cx, inner, true, out);
            }
            "AlternateContent" => {
                if let Some(f) = n.child("Fallback") {
                    walk(f, cx, xf, grouped, out);
                }
            }
            _ => {}
        }
    }
}

fn shape(n: &Node, cx: &Ctx, xf: Xf, grouped: bool) -> Option<Shape> {
    let src = cx.src;
    let th = cx.th;
    let ph = read_ph(n, src);
    let parent = ph.as_ref().and_then(|p| cx.parent(p));
    let mut s = match parent {
        Some(p) => Shape {
            x: p.x,
            y: p.y,
            w: p.w,
            h: p.h,
            fill: p.fill,
            line: p.line,
            geom: p.geom.clone(),
            lv: p.lv.clone(),
            anchor: p.anchor,
            ins: p.ins,
            scale: p.scale,
            grow: p.grow,
            wrap: p.wrap,
            font_color: p.font_color,
            ..Default::default()
        },
        None => Shape { lv: cx.base_levels(ph.as_ref()), ..Default::default() },
    };
    s.title = ph.as_ref().is_some_and(|p| p.ty == "title");
    s.ph = ph;
    s.span = (n.start, n.end);
    s.grouped = grouped;
    // p:style: dolgu / çizgi / yazı rengi varsayılanları.
    if let Some(st) = n.child("style") {
        let r = |k: &str| st.child(k).filter(|r| r.attr("idx").is_some_and(|i| i != "0")).and_then(|r| fill_color(r, th));
        if let Some(c) = r("fillRef") {
            s.fill = Some(c);
        }
        if let Some(c) = r("lnRef") {
            s.line = Some((c, 0.75));
        }
        if let Some(c) = st.child("fontRef").and_then(|r| fill_color(r, th)) {
            s.font_color = Some(c);
            for f in s.lv.fmt.iter_mut() {
                f.color = Some(c);
            }
        }
    }
    let local = n.local();
    // Konum.
    let xfrm = if local == "graphicFrame" { n.child("xfrm") } else { n.child("spPr").and_then(|p| p.child("xfrm")) };
    if let Some(x) = xfrm {
        s.xf_span = Some((x.start, x.end));
        if let Some((x0, y0, w, h)) = xfrm_box(x) {
            s.x = x0 * xf.a + xf.b;
            s.y = y0 * xf.c + xf.d;
            s.w = w * xf.a;
            s.h = h * xf.c;
        }
        if local == "cxnSp" && (x.attr("flipH") == Some("1")) != (x.attr("flipV") == Some("1")) {
            s.geom = "line/".into();
        }
    }
    if let Some(sp) = n.child("spPr") {
        s.sppr_span = Some((sp.start, sp.end));
        for c in &sp.children {
            match c.local() {
                "prstGeom" => {
                    if !s.geom.starts_with("line") {
                        s.geom = c.attr("prst").unwrap_or("rect").to_string();
                    }
                }
                "custGeom" => s.geom = "cust".into(),
                "solidFill" => s.fill = fill_color(c, th),
                "noFill" => s.fill = None,
                "gradFill" => s.fill = c.find("gs").and_then(|g| fill_color(g, th)).or(s.fill),
                "ln" => {
                    if c.child("noFill").is_some() {
                        s.line = None;
                    } else {
                        let w = emu(c.attr("w")).or(s.line.map(|l| l.1)).unwrap_or(0.75);
                        if let Some(col) = c.child("solidFill").and_then(|f| fill_color(f, th)).or(s.line.map(|l| l.0)) {
                            s.line = Some((col, w));
                        } else if let Some(l) = s.line.as_mut() {
                            l.1 = w;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    match local {
        "cxnSp" => {
            if !s.geom.starts_with("line") {
                s.geom = "line".into();
            }
            s.fill = None;
            if s.line.is_none() {
                s.line = Some((0x404040, 0.75));
            }
        }
        "pic" => {
            let id = n.find("blip").and_then(|b| b.attr("r:embed"))?;
            let path = cx.rels.get(id).map(|r| r.1.clone())?;
            s.kind = Kind::Pic(path);
        }
        "graphicFrame" => {
            let gd = n.find("graphicData")?;
            let uri = gd.attr("uri").unwrap_or("");
            s.kind = if let Some(t) = gd.child("tbl") {
                Kind::Table(Box::new(super::table::parse(t, src, cx.th)))
            } else if uri.ends_with("/chart") {
                Kind::Frame("chart".into())
            } else if uri.contains("diagram") {
                Kind::Frame("smartart".into())
            } else {
                Kind::Frame("object".into())
            };
        }
        _ => {}
    }
    // Metin gövdesi.
    if let Some(tb) = n.child("txBody") {
        if let Some(bp) = tb.child("bodyPr") {
            match bp.attr("anchor") {
                Some("t") => s.anchor = 0,
                Some("ctr") => s.anchor = 1,
                Some("b") => s.anchor = 2,
                _ => {}
            }
            for (i, k) in ["lIns", "tIns", "rIns", "bIns"].iter().enumerate() {
                if let Some(v) = emu(bp.attr(k)) {
                    s.ins[i] = v;
                }
            }
            if let Some(w) = bp.attr("wrap") {
                s.wrap = w != "none";
            }
            if let Some(a) = bp.child("normAutofit") {
                s.scale = a.attr("fontScale").and_then(|v| v.parse::<f32>().ok()).map(|v| v / 100_000.0).unwrap_or(1.0);
            } else if bp.child("spAutoFit").is_some() {
                s.grow = true;
                s.scale = 1.0;
            } else if bp.child("noAutofit").is_some() {
                s.scale = 1.0;
                s.grow = false;
            }
        }
        if let Some(ls) = tb.child("lstStyle") {
            let own = lst_levels(ls, th);
            s.lv.merge(&own);
        }
        let ps: Vec<&Node> = tb.children_named("p").collect();
        if let (Some(f), Some(l)) = (ps.first(), ps.last()) {
            s.body = Some((f.start, l.end));
            s.paras = ps.iter().map(|p| para(p, src, th)).collect();
        }
    }
    s.renumber();
    Some(s)
}

/// `a:p` → paragraf.
pub fn para(p: &Node, src: &str, th: &Theme) -> Para {
    let mut out = Para { open: p.open_tag(src), span: Some((p.start, p.end)), ..Default::default() };
    for c in &p.children {
        match c.local() {
            "pPr" => {
                out.ppr = c.raw(src).to_string();
                out.level = c.attr("lvl").and_then(|v| v.parse::<u8>().ok()).unwrap_or(0).min(8);
                out.align = c.attr("algn").and_then(algn);
                out.orig_align = out.align;
            }
            "r" => {
                let rp = c.child("rPr");
                let fmt = rp.map(|r| rpr_fmt(r, th)).unwrap_or_default();
                let text = c.child("t").map(|t| t.text(src)).unwrap_or_default();
                out.runs.push(Run { text, fmt, orig: fmt, rpr: rp.map(|r| r.raw(src).to_string()).unwrap_or_default(), ..Default::default() });
            }
            "br" => out.runs.push(Run { text: "\n".into(), atom: Some(c.raw(src).to_string()), ..Default::default() }),
            "fld" => {
                let fmt = c.child("rPr").map(|r| rpr_fmt(r, th)).unwrap_or_default();
                let text = c.child("t").map(|t| t.text(src)).unwrap_or_default();
                out.runs.push(Run { text, fmt, orig: fmt, atom: Some(c.raw(src).to_string()), ..Default::default() });
            }
            "endParaRPr" => out.end = c.raw(src).to_string(),
            _ => {}
        }
    }
    out
}

fn bg_of(csld: &Node, th: &Theme, r: &Rels) -> Bg {
    let Some(bg) = csld.child("bg") else { return Bg::None };
    if let Some(pr) = bg.child("bgPr") {
        if let Some(f) = pr.child("solidFill") {
            return fill_color(f, th).map(Bg::Solid).unwrap_or_default();
        }
        if let Some(g) = pr.child("gradFill") {
            return g.find("gs").and_then(|g| fill_color(g, th)).map(Bg::Solid).unwrap_or_default();
        }
        if let Some(id) = pr.find("blip").and_then(|b| b.attr("r:embed")) {
            if let Some((_, p)) = r.get(id) {
                return Bg::Image(p.clone());
            }
        }
    }
    if let Some(c) = bg.child("bgRef").and_then(|b| fill_color(b, th)) {
        return Bg::Solid(c);
    }
    Bg::None
}

fn load_theme(z: &Zip, path: &str) -> Theme {
    let mut th = Theme::default();
    let Some(src) = z.read_str(path) else { return th };
    let Ok(root) = xml::parse(&src) else { return th };
    if let Some(cs) = root.find("clrScheme") {
        for c in &cs.children {
            if let Some(col) = c.children.first().and_then(|x| color_el(x, &th)) {
                th.colors.insert(c.local().to_string(), col);
            }
        }
    }
    if let Some(fs) = root.find("fontScheme") {
        let f = |k: &str| fs.child(k).and_then(|m| m.child("latin")).and_then(|l| l.attr("typeface")).unwrap_or("Calibri").to_string();
        th.major = f("majorFont");
        th.minor = f("minorFont");
    } else {
        th.major = "Calibri".into();
        th.minor = "Calibri".into();
    }
    th
}

fn load_master(z: &Zip, path: &str, dflt: &Levels) -> Option<(Master, Layer)> {
    let src = z.read_str(path)?;
    let root = xml::parse(&src).ok()?;
    let r = rels(z, path);
    let mut m = Master::default();
    if let Some((_, tp)) = r.values().find(|(t, _)| t.ends_with("/theme")) {
        m.theme = load_theme(z, tp);
    }
    if let Some(map) = root.child("clrMap") {
        for (k, v) in &map.attrs {
            if let Some(c) = m.theme.colors.get(v.as_str()).copied() {
                m.theme.colors.insert(k.clone(), c);
            }
        }
    }
    if let Some(ts) = root.child("txStyles") {
        if let Some(t) = ts.child("titleStyle") {
            m.title = lst_levels(t, &m.theme);
        }
        if let Some(t) = ts.child("bodyStyle") {
            m.body = lst_levels(t, &m.theme);
        }
        if let Some(t) = ts.child("otherStyle") {
            m.other = lst_levels(t, &m.theme);
        }
    }
    let csld = root.child("cSld")?;
    let mut layer = Layer { bg: bg_of(csld, &m.theme, &r), show_master: true, ..Default::default() };
    if let Some(tree) = csld.child("spTree") {
        let cx = Ctx { th: &m.theme, master: Some(&m), parents: &[], dflt, rels: &r, src: &src };
        shapes(tree, &cx, &mut layer.shapes);
    }
    Some((m, layer))
}

/// Düzeni (ve asılı yoksa onu) okuyup `d.layers`'a ekler.
pub fn ensure_layout(d: &mut Deck, path: &str) {
    if d.layers.contains_key(path) {
        return;
    }
    let Some(src) = d.zip.read_str(path) else { return };
    let Ok(root) = xml::parse(&src) else { return };
    let r = rels(&d.zip, path);
    let master = r.values().find(|(t, _)| t.ends_with("/slideMaster")).map(|x| x.1.clone()).unwrap_or_default();
    if !d.masters.contains_key(&master) {
        if let Some((m, l)) = load_master(&d.zip, &master, &d.default_text) {
            d.masters.insert(master.clone(), m);
            d.layers.insert(master.clone(), l);
        }
    }
    let mut layer = Layer {
        show_master: root.attr("showMasterSp") != Some("0"),
        master: master.clone(),
        ty: root.attr("type").unwrap_or("obj").to_string(),
        ..Default::default()
    };
    let th = d.masters.get(&master).map(|m| m.theme.clone()).unwrap_or_default();
    if let Some(csld) = root.child("cSld") {
        layer.name = csld.attr("name").unwrap_or("").to_string();
        layer.bg = bg_of(csld, &th, &r);
        if let Some(tree) = csld.child("spTree") {
            let empty = Vec::new();
            let parents = d.layers.get(&master).map(|l| &l.shapes).unwrap_or(&empty);
            let cx = Ctx { th: &th, master: d.masters.get(&master), parents, dflt: &d.default_text, rels: &r, src: &src };
            shapes(tree, &cx, &mut layer.shapes);
        }
    }
    d.layers.insert(path.to_string(), layer);
}

/// Bir slayt parçasını okur. Düzeni `ensure_layout` ile önceden yüklenmiş olmalı.
pub fn parse_slide(d: &Deck, part: &str, src: String, r: Rels) -> Result<Slide, String> {
    let root = xml::parse(&src)?;
    let layout = r.values().find(|(t, _)| t.ends_with("/slideLayout")).map(|x| x.1.clone()).unwrap_or_default();
    let th = d.theme_for(&layout);
    let csld = root.child("cSld").ok_or("cSld yok")?;
    let tree = csld.child("spTree").ok_or("spTree yok")?;
    let mut shapes_v = Vec::new();
    {
        let empty = Vec::new();
        let parents = d.layers.get(&layout).map(|l| &l.shapes).unwrap_or(&empty);
        let cx = Ctx { th: &th, master: d.master_of(&layout), parents, dflt: &d.default_text, rels: &r, src: &src };
        shapes(tree, &cx, &mut shapes_v);
    }
    Ok(Slide {
        part: part.to_string(),
        bg: bg_of(csld, &th, &r),
        shapes: shapes_v,
        layout,
        show_master: root.attr("showMasterSp") != Some("0"),
        hidden: root.attr("show") == Some("0"),
        tree_end: tree.inner.1,
        rels: r,
        new_rels: None,
        id: 0,
        rid: String::new(),
        src,
    })
}

pub fn load(bytes: &[u8]) -> Result<Deck, String> {
    let z = Zip::parse(bytes).map_err(|e| e.to_string())?;
    let main = rels(&z, "").into_values().find(|(t, _)| t.ends_with("/officeDocument")).map(|x| x.1).unwrap_or_else(|| "ppt/presentation.xml".into());
    let main = main.trim_start_matches('/').to_string();
    let psrc = z.read_str(&main).ok_or("presentation.xml yok")?;
    let pres = xml::parse(&psrc)?;
    let prels = rels(&z, &main);
    let sz = pres.child("sldSz");
    let w = sz.and_then(|s| emu(s.attr("cx"))).unwrap_or(960.0);
    let h = sz.and_then(|s| emu(s.attr("cy"))).unwrap_or(540.0);
    let mut d = Deck { zip: z, w, h, slides: Vec::new(), layers: HashMap::new(), masters: HashMap::new(), default_text: Levels::default(), orig: Vec::new() };
    // Asılları oku (ilk asılın teması varsayılan metin biçemi için gerekir).
    let master_paths: Vec<String> = pres.child("sldMasterIdLst").map(|l| l.children.iter().filter_map(|m| m.attr("r:id")).filter_map(|id| prels.get(id).map(|x| x.1.clone())).collect()).unwrap_or_default();
    for mp in &master_paths {
        if let Some((m, l)) = load_master(&d.zip, mp, &Levels::default()) {
            d.masters.insert(mp.clone(), m);
            d.layers.insert(mp.clone(), l);
        }
    }
    if let Some(dt) = pres.child("defaultTextStyle") {
        let th = master_paths.first().and_then(|m| d.masters.get(m)).map(|m| m.theme.clone()).unwrap_or_default();
        d.default_text = lst_levels(dt, &th);
        // Asılların yer tutucu olmayan şekillerini doğru biçemle yeniden oku.
        for mp in &master_paths {
            if let Some((_, l)) = load_master(&d.zip, mp, &d.default_text) {
                d.layers.insert(mp.clone(), l);
            }
        }
    }
    let ids: Vec<(u32, String)> = pres.child("sldIdLst").map(|l| l.children.iter().filter_map(|s| Some((s.attr("id")?.parse().ok()?, s.attr("r:id")?.to_string()))).collect()).unwrap_or_default();
    for (id, rid) in ids {
        let Some((_, part)) = prels.get(&rid).cloned() else { continue };
        let Some(src) = d.zip.read_str(&part) else { continue };
        let r = rels(&d.zip, &part);
        if let Some((_, lp)) = r.values().find(|(t, _)| t.ends_with("/slideLayout")) {
            let lp = lp.clone();
            ensure_layout(&mut d, &lp);
        }
        let mut s = parse_slide(&d, &part, src, r)?;
        s.id = id;
        s.rid = rid.clone();
        d.orig.push((part, id, rid));
        d.slides.push(s);
    }
    // İlişkisi olup slayta bağlı olmayan düzenler de yeni slayt için lazım olabilir.
    let lays: Vec<String> = d.layers.values().map(|l| l.master.clone()).filter(|m| !m.is_empty()).collect();
    for m in lays {
        let mr = rels(&d.zip, &m);
        let paths: Vec<String> = mr.values().filter(|(t, _)| t.ends_with("/slideLayout")).map(|x| x.1.clone()).collect();
        for p in paths {
            ensure_layout(&mut d, &p);
        }
    }
    Ok(d)
}
