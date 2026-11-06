// Exe simgesini gömer: assets\icons\<ad>.ico → .rc → .res (Windows SDK'daki
// rc.exe) → bağlayıcı. Simge kaynak kimliği 1'dir (LoadIconW(inst, 1)).
// rc.exe bulunamazsa derleme durmaz, yalnızca uyarı verir. Her exe'nin
// build.rs'i bu dosyayı `include!` ile alır. Simgeler tools\make-icons.ps1
// ile üretilir.

fn find_rc() -> Option<std::path::PathBuf> {
    if let Ok(p) = std::env::var("RC") {
        return Some(p.into());
    }
    let kits = std::path::Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let mut found: Vec<std::path::PathBuf> = std::fs::read_dir(kits).ok()?.flatten().map(|e| e.path().join(r"x64\rc.exe")).filter(|p| p.exists()).collect();
    found.sort();
    found.pop()
}

/// Exe'nin "Ayrıntılar" sekmesindeki sürüm/yayımcı bilgisi (VERSIONINFO).
/// İmzasız exe'lerde bu bilginin olmaması antivirüslerin şüphesini artırır.
fn version_rc() -> String {
    let ver = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let mut n: Vec<u16> = ver.split(['.', '-']).filter_map(|p| p.parse().ok()).take(3).collect();
    n.resize(4, 0);
    let nums = n.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",");
    let desc = match std::env::var("CARGO_PKG_NAME").as_deref() {
        Ok("carlui") => "CarlUI shell",
        Ok("carlui-dock") => "CarlUI Dock",
        Ok("carlui-tray") => "CarlUI Tray",
        Ok("carlui-island") => "CarlUI Island",
        Ok("carlui-start") => "CarlUI Start menu",
        Ok("carlui-settings") => "CarlUI Settings",
        Ok("carlui-taskmgr") => "CarlUI Task manager",
        Ok("carlui-convert") => "CarlUI File converter",
        Ok("carlui-sensors") => "CarlUI Sensors",
        Ok("carlui-vault") => "CarlUI Vault",
        Ok("carlui-uninstall") => "CarlUI Uninstaller",
        Ok("carlui-tools") => "CarlUI Tools",
        Ok("carlui-expand") => "CarlUI Text expander",
        Ok("carlui-replay") => "CarlUI Instant replay",
        Ok("carlui-setup") => "CarlUI Setup",
        _ => "CarlUI",
    };
    let file = match std::env::var("CARGO_PKG_NAME").unwrap_or_default().as_str() {
        "carlui-setup" => "CarlUI-Setup.exe".to_string(),
        p => format!("{p}.exe"),
    };
    format!(
        "1 VERSIONINFO\nFILEVERSION {nums}\nPRODUCTVERSION {nums}\nFILEOS 0x40004\nFILETYPE 0x1\n{{\n BLOCK \"StringFileInfo\"\n {{\n  BLOCK \"040904B0\"\n  {{\n   VALUE \"CompanyName\", \"Demhat Dayan\"\n   VALUE \"FileDescription\", \"{desc}\"\n   VALUE \"FileVersion\", \"{ver}\"\n   VALUE \"InternalName\", \"{file}\"\n   VALUE \"LegalCopyright\", \"Copyright (c) 2026 Demhat Dayan, MIT License\"\n   VALUE \"OriginalFilename\", \"{file}\"\n   VALUE \"ProductName\", \"CarlUI\"\n   VALUE \"ProductVersion\", \"{ver}\"\n  }}\n }}\n BLOCK \"VarFileInfo\"\n {{\n  VALUE \"Translation\", 0x409, 1200\n }}\n}}\n"
    )
}

#[allow(dead_code)]
fn embed_icon(name: &str) {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let ico = manifest.join(r"..\..\assets\icons").join(format!("{name}.ico"));
    println!("cargo:rerun-if-changed={}", ico.display());
    println!("cargo:rerun-if-changed=../../tools/winres.rs");
    let Ok(ico) = ico.canonicalize() else {
        println!("cargo:warning=simge yok: {}", ico.display());
        return;
    };
    let Some(rc) = find_rc() else {
        println!("cargo:warning=rc.exe bulunamadı; {name} simgesiz derleniyor");
        return;
    };
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let script = out.join("icon.rc");
    let path = ico.display().to_string().trim_start_matches(r"\\?\").replace('\\', "\\\\");
    std::fs::write(&script, format!("1 ICON \"{path}\"\n{}", version_rc())).unwrap();
    let res = out.join("icon.res");
    let ok = std::process::Command::new(rc).arg("/nologo").arg("/fo").arg(&res).arg(&script).status().map(|s| s.success()).unwrap_or(false);
    if ok {
        println!("cargo:rustc-link-arg-bins={}", res.display());
    } else {
        println!("cargo:warning=rc.exe simgeyi derleyemedi: {name}");
    }
}
