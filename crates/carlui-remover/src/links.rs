//! Kısayol (`.lnk`) dosyaları: klasörlerde bulunur, hedefleri çözülür.

use std::{
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
};
use windows::{
    core::{Interface, HSTRING},
    Win32::{
        System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ},
        UI::Shell::{IShellLinkW, ShellLink},
    },
};

const REPARSE: u32 = 0x400;

pub fn is_lnk(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
}

/// `dir` ve en çok `depth` kat alt klasöründeki kısayollar. Bağlantı klasörleri
/// izlenmez.
pub fn under(dir: &Path, depth: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), 0)];
    while let Some((d, n)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(m) = std::fs::symlink_metadata(&p) else { continue };
            if m.file_attributes() & REPARSE != 0 {
                continue;
            }
            if m.is_dir() {
                if n < depth {
                    stack.push((p, n + 1));
                }
            } else if is_lnk(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// Kısayolun gösterdiği dosya. MSI'ın "ilan edilmiş" kısayolları ve web
/// bağlantıları dosya göstermez: `None`. COM açık bir iş parçacığında çağrılır.
pub fn target(lnk: &Path) -> Option<String> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        link.cast::<IPersistFile>().ok()?.Load(&HSTRING::from(lnk.as_os_str()), STGM_READ).ok()?;
        let mut buf = [0u16; 1024];
        link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
        let n = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        let s = String::from_utf16_lossy(&buf[..n]).trim().to_string();
        (!s.is_empty()).then_some(s)
    }
}
