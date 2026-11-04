// Kurucunun derlemesi: (1) target\release'teki CarlUI exe'lerini ve tarayıcı
// eklentisini kurucunun içine gömen payload.rs'i üretir; (2) simgeyi ve
// "yönetici isteme" bildirimini (manifest) exe'ye ekler. Önce diğer exe'ler
// derlenmelidir: tools\build-setup.ps1 bu sırayı izler.

include!("../../tools/winres.rs");

use std::path::{Path, PathBuf};

const EXES: [&str; 10] = ["carlui", "carlui-dock", "carlui-tray", "carlui-island", "carlui-start", "carlui-settings", "carlui-taskmgr", "carlui-convert", "carlui-sensors", "carlui-vault"];
const EXTENSION: [&str; 6] = ["manifest.json", "background.js", "popup.html", "popup.js", "popup.css", "KURULUM.md"];

fn plain(p: &Path) -> String {
    p.display().to_string().trim_start_matches(r"\\?\").to_string()
}

fn payload(root: &Path, out: &Path) {
    // tools\build-setup.ps1 exe'leri ayrı bir hedef klasörde derler (çalışan
    // CarlUI'nin kilitlediği target\release'e dokunmamak için) ve yolu buradan verir.
    println!("cargo:rerun-if-env-changed=CARLUI_PAYLOAD_DIR");
    let rel = std::env::var_os("CARLUI_PAYLOAD_DIR").map(PathBuf::from).unwrap_or_else(|| root.join(r"target\release"));
    let mut code = String::from("pub static FILES: &[(&str, &[u8])] = &[\n");
    for e in EXES {
        let p = rel.join(format!("{e}.exe"));
        println!("cargo:rerun-if-changed={}", plain(&p));
        if p.exists() {
            code += &format!("    (\"{e}.exe\", include_bytes!(r\"{}\")),\n", plain(&p));
        } else {
            println!("cargo:warning=kurucuya eklenmedi (önce derleyin): {e}.exe");
        }
    }
    // Lisans belgeleri exe'lerin yanına kurulur (MIT ve PawnIO modülünün LGPL'i).
    for (name, src) in [("LICENSE.txt", r"LICENSE"), ("THIRD-PARTY-NOTICES.md", r"THIRD-PARTY-NOTICES.md"), ("IntelMSR.COPYING.txt", r"crates\carlui-sensors\IntelMSR.COPYING.txt")] {
        let p = root.join(src);
        println!("cargo:rerun-if-changed={}", plain(&p));
        if p.exists() {
            code += &format!("    (\"{name}\", include_bytes!(r\"{}\")),\n", plain(&p));
        }
    }
    code += "];\n\npub static EXTENSION: &[(&str, &[u8])] = &[\n";
    let ext = root.join(r"extension\carlui-downloads");
    for f in EXTENSION {
        let p = ext.join(f);
        println!("cargo:rerun-if-changed={}", plain(&p));
        if p.exists() {
            code += &format!("    (\"{f}\", include_bytes!(r\"{}\")),\n", plain(&p));
        }
    }
    code += "];\n\n";
    // Mozilla'nın imzaladığı paket varsa Firefox'a otomatik tanıtılır.
    let xpi = root.join(r"extension\carlui-downloads.xpi");
    println!("cargo:rerun-if-changed={}", plain(&xpi));
    if xpi.exists() {
        code += &format!("pub static XPI: Option<&[u8]> = Some(include_bytes!(r\"{}\"));\n", plain(&xpi));
    } else {
        code += "pub static XPI: Option<&[u8]> = None;\n";
    }
    let logo = root.join(r"assets\icons\carlui.png");
    code += &format!("pub static LOGO: &[u8] = include_bytes!(r\"{}\");\n", plain(&logo));
    // İsteğe bağlı CarlUI duvar kâğıdı (tools\make-wallpaper.ps1 üretir).
    let wall = root.join(r"assets\wallpaper\carlui-3840x2160.png");
    println!("cargo:rerun-if-changed={}", plain(&wall));
    code += &format!("pub static WALLPAPER: &[u8] = include_bytes!(r\"{}\");\n", plain(&wall));
    std::fs::write(out.join("payload.rs"), code).unwrap();
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
