// Kurucunun derlemesi: (1) target\release'teki CarlUI exe'lerini ve tarayıcı
// eklentisini tek bir zstd akışı olarak sıkıştırıp kurucuya gömen payload.rs'i
// üretir (`uninstaller` özelliğinde yalnızca dosya adları); (2) simgeyi ve
// "yönetici isteme" bildirimini (manifest) exe'ye ekler. Önce diğer exe'ler
// derlenmelidir: tools\build-setup.ps1 bu sırayı izler.

include!("../../tools/winres.rs");

use std::path::{Path, PathBuf};

const EXES: [&str; 15] = ["carlui", "carlui-dock", "carlui-tray", "carlui-island", "carlui-start", "carlui-settings", "carlui-taskmgr", "carlui-convert", "carlui-sensors", "carlui-vault", "carlui-uninstall", "carlui-office", "carlui-tools", "carlui-expand", "carlui-replay"];
const EXTENSION: [&str; 11] = ["manifest.json", "background.js", "content.js", "passkey-page.js", "passkey-relay.js", "_locales/en/messages.json", "_locales/tr/messages.json", "popup.html", "popup.js", "popup.css", "KURULUM.md"];

fn plain(p: &Path) -> String {
    p.display().to_string().trim_start_matches(r"\\?\").to_string()
}

/// Gömülecek tek bir dosya: kurulacak adı ve kaynağı.
struct Item {
    group: &'static str,
    name: String,
    path: PathBuf,
}

fn payload(root: &Path, out: &Path) {
    // tools\build-setup.ps1 exe'leri ayrı bir hedef klasörde derler (çalışan
    // CarlUI'nin kilitlediği target\release'e dokunmamak için) ve yolu buradan verir.
    println!("cargo:rerun-if-env-changed=CARLUI_PAYLOAD_DIR");
    println!("cargo:rerun-if-env-changed=CARLUI_UNINSTALLER");
    let rel = std::env::var_os("CARLUI_PAYLOAD_DIR").map(PathBuf::from).unwrap_or_else(|| root.join(r"target\release"));
    // Kaldırıcı derlemesi yalnızca dosya adlarını bilir; içerik gömülmez.
    let names_only = std::env::var_os("CARGO_FEATURE_UNINSTALLER").is_some();
    let mut items = Vec::new();
    let mut add = |group: &'static str, name: String, path: PathBuf| {
        println!("cargo:rerun-if-changed={}", plain(&path));
        if path.exists() {
            items.push(Item { group, name, path });
            true
        } else {
            false
        }
    };
    for e in EXES {
        if !add("FILES", format!("{e}.exe"), rel.join(format!("{e}.exe"))) {
            println!("cargo:warning=kurucuya eklenmedi (önce derleyin): {e}.exe");
        }
    }
    // Lisans belgeleri exe'lerin yanına kurulur (MIT ve PawnIO modülünün LGPL'i).
    for (name, src) in [("LICENSE.txt", r"LICENSE"), ("THIRD-PARTY-NOTICES.md", r"THIRD-PARTY-NOTICES.md"), ("IntelMSR.COPYING.txt", r"crates\carlui-sensors\IntelMSR.COPYING.txt")] {
        add("FILES", name.into(), root.join(src));
    }
    let ext = root.join(r"extension\carlui-downloads");
    for f in EXTENSION {
        add("EXTENSION", f.into(), ext.join(f));
    }
    // Mozilla'nın imzaladığı paket varsa Firefox'a otomatik tanıtılır.
    add("XPI", "carlui-downloads.xpi".into(), root.join(r"extension\carlui-downloads.xpi"));
    // İsteğe bağlı CarlUI duvar kâğıdı (tools\make-wallpaper.ps1 üretir).
    add("WALLPAPER", "carlui.png".into(), root.join(r"assets\wallpaper\carlui-3840x2160.png"));
    // Küçük kaldırıcı (aynı crate'in `uninstaller` özellikli derlemesi).
    if let Some(u) = std::env::var_os("CARLUI_UNINSTALLER") {
        add("UNINSTALLER", "CarlUI-Uninstall.exe".into(), PathBuf::from(u));
    }

    // Hepsi tek akışta sıkıştırılır: exe'ler aynı kitaplık kodunu taşıdığından
    // ortak sıkıştırma ayrı ayrı sıkıştırmaktan çok daha küçük çıkar.
    let mut raw = Vec::new();
    let mut groups: Vec<(&str, Vec<String>)> = ["FILES", "EXTENSION", "XPI", "WALLPAPER", "UNINSTALLER"].iter().map(|g| (*g, Vec::new())).collect();
    for it in &items {
        let len = if names_only { 0 } else { std::fs::metadata(&it.path).map(|m| m.len() as usize).unwrap_or(0) };
        let off = raw.len();
        if !names_only {
            raw.extend(std::fs::read(&it.path).unwrap());
        }
        let line = format!("Entry {{ name: {:?}, off: {off}, len: {len} }}", it.name);
        groups.iter_mut().find(|(g, _)| *g == it.group).unwrap().1.push(line);
    }
    let blob = out.join("payload.zst");
    let packed = if raw.is_empty() {
        Vec::new()
    } else {
        let mut e = zstd::stream::Encoder::new(Vec::new(), 19).unwrap();
        let _ = e.multithread(std::thread::available_parallelism().map_or(1, |n| n.get() as u32));
        e.include_contentsize(true).unwrap();
        e.set_pledged_src_size(Some(raw.len() as u64)).unwrap();
        std::io::Write::write_all(&mut e, &raw).unwrap();
        e.finish().unwrap()
    };
    std::fs::write(&blob, &packed).unwrap();

    let mut code = String::new();
    for (g, lines) in &groups {
        code += &match *g {
            "FILES" | "EXTENSION" => format!("pub static {g}: &[Entry] = &[{}];\n", lines.join(", ")),
            _ => format!("pub static {g}: Option<Entry> = {};\n", lines.first().map_or("None".into(), |l| format!("Some({l})"))),
        };
    }
    code += &format!("pub const RAW_LEN: usize = {};\n", raw.len());
    code += &format!("static PACKED: &[u8] = include_bytes!(r\"{}\");\n", plain(&blob));
    let logo = root.join(r"assets\icons\carlui.png");
    code += &format!("pub static LOGO: &[u8] = include_bytes!(r\"{}\");\n", plain(&logo));
    code += &format!("pub static NOTES: &str = {:?};\n", if names_only { String::new() } else { notes(root) });
    std::fs::write(out.join("payload.rs"), code).unwrap();
}

/// Sürüm notu (Yenilikler ekranı için): `CARLUI_NOTES` ya da dist\release-notes.md.
/// GitHub sürüm notuyla aynı biçim: "## Türkçe" … "---" … "## English" ….
fn notes(root: &Path) -> String {
    println!("cargo:rerun-if-env-changed=CARLUI_NOTES");
    let path = std::env::var_os("CARLUI_NOTES").map(PathBuf::from).unwrap_or_else(|| root.join(r"dist\release-notes.md"));
    println!("cargo:rerun-if-changed={}", plain(&path));
    std::fs::read_to_string(&path).unwrap_or_default()
}

fn resources(root: &Path, manifest_dir: &Path, out: &Path) {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }
    let ico = root.join(r"assets\icons\carlui.ico");
    let man = manifest_dir.join("setup.manifest");
    println!("cargo:rerun-if-changed={}", plain(&man));
    let Some(rc) = find_rc() else {
        println!("cargo:warning=rc.exe bulunamadı; kurucu simgesiz ve bildirimsiz derleniyor");
        return;
    };
    let esc = |p: &Path| plain(p).replace('\\', "\\\\");
    let script = out.join("setup.rc");
    std::fs::write(&script, format!("1 ICON \"{}\"\n1 24 \"{}\"\n{}", esc(&ico), esc(&man), version_rc())).unwrap();
    let res = out.join("setup.res");
    let ok = std::process::Command::new(rc).arg("/nologo").arg("/fo").arg(&res).arg(&script).status().map(|s| s.success()).unwrap_or(false);
    if ok {
        println!("cargo:rustc-link-arg-bins={}", res.display());
    } else {
        println!("cargo:warning=rc.exe kurucu kaynaklarını derleyemedi");
    }
}

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest_dir.join(r"..\..").canonicalize().unwrap();
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    payload(&root, &out);
    resources(&root, &manifest_dir, &out);
}
