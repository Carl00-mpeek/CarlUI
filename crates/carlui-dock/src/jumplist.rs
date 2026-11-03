//! Bu modül "Windows görev çubuğunda sağ tıkta çıkan uygulama görevleri
//! (Firefox: Yeni pencere, Gizli pencere; Steam: Son oynananlar…) dock'ta
//! yok" sorununu çözüyor. Uygulamalar bu listeyi Windows'a verir, Windows da
//! `%APPDATA%\Microsoft\Windows\Recent\CustomDestinations\<kimlik>.customDestinations-ms`
//! dosyasına yazar. Dosya adı, uygulama kimliğinin (AUMID ya da exe yolu,
//! büyük harf, UTF-16) CRC64'üdür. Dosya; kategoriler ve her kategoride
//! "CLSID + IPersistStream verisi" biçiminde saklanmış kısayollardan oluşur.
//! Kısayollar Windows'un kendi nesnesine (ShellLink) yüklenir, tıklanınca
//! görev çubuğunun yaptığı gibi hedef + argümanlarla çalıştırılır.

use crate::dock::Entry;
use windows::{
    core::*,
    Win32::{
        Storage::EnhancedStorage::PKEY_Title,
        System::Com::{StructuredStorage::PropVariantToStringAlloc, *},
        UI::{
            Shell::{PropertiesSystem::*, *},
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    },
};

const FOOTER: u32 = 0xBABF_FBAB;
/// Bir özel kategoriden en fazla bu kadar öğe gösterilir.
const PER_CATEGORY: usize = 6;

pub struct Group {
    /// Boşsa uygulamanın "Görevler" bölümü.
    pub title: String,
    pub items: Vec<Item>,
}

pub struct Item {
    pub title: String,
    /// Kısayolda yazan simge (dosya, sıra); boş olabilir.
    pub icon: (String, i32),
    /// Hedef dosya (URL/uygulama hedeflerinde boş).
    pub target: String,
    link: IShellLinkW,
}

fn link_info(link: &IShellLinkW) -> ((String, i32), String) {
    let mut ib = [0u16; 260];
    let mut idx = 0;
    let mut pb = [0u16; 260];
    let z = |b: &[u16]| String::from_utf16_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(0)]);
    unsafe {
        let _ = link.GetIconLocation(&mut ib, &mut idx);
        let _ = link.GetPath(&mut pb, std::ptr::null_mut(), 0);
    }
    ((z(&ib), idx), z(&pb))
}

fn crc64(s: &str) -> u64 {
    let mut c = u64::MAX;
    for b in s.to_uppercase().encode_utf16().flat_map(|u| u.to_le_bytes()) {
        c ^= b as u64;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0x92C6_4265_D321_39A4 } else { c >> 1 };
        }
    }
    c
}

fn known(id: &GUID) -> Option<String> {
    unsafe { SHGetKnownFolderPath(id, KNOWN_FOLDER_FLAG(0), None).ok().map(|p| {
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as _));
        s
    }) }
}

/// Uygulamanın Windows'taki olası kimlikleri: AUMID, sonra exe yolu (bilinen
/// klasörler GUID'leriyle yazılmış hâli ve düz hâli).
fn ids(e: &Entry) -> Vec<String> {
    let mut v = Vec::new();
    if !e.app.aumid.is_empty() {
        v.push(e.app.aumid.clone());
    }
    let exe = if e.exe.is_empty() { e.app.exe.clone() } else { e.exe.clone() };
    if !exe.is_empty() {
        let low = exe.to_lowercase();
        for id in [FOLDERID_ProgramFilesX64, FOLDERID_ProgramFilesX86, FOLDERID_System, FOLDERID_SystemX86, FOLDERID_Windows, FOLDERID_UserProgramFiles, FOLDERID_LocalAppData] {
            if let Some(dir) = known(&id) {
                let d = dir.to_lowercase();
                if low.starts_with(&d) && low.as_bytes().get(d.len()) == Some(&b'\\') {
                    v.push(format!("{{{id:?}}}{}", &exe[dir.len()..]));
                }
            }
        }
        v.push(exe);
    }
    v
}

fn file_for(e: &Entry) -> Option<std::path::PathBuf> {
    let dir = std::path::PathBuf::from(std::env::var_os("APPDATA")?).join("Microsoft\\Windows\\Recent\\CustomDestinations");
    ids(e).iter().map(|id| dir.join(format!("{:016x}.customDestinations-ms", crc64(id)))).find(|p| p.metadata().map(|m| m.len() > 24).unwrap_or(false))
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn u16(&mut self) -> Option<u16> {
        let v = u16::from_le_bytes(self.b.get(self.at..self.at + 2)?.try_into().ok()?);
        self.at += 2;
        Some(v)
    }
    fn u32(&mut self) -> Option<u32> {
        let v = u32::from_le_bytes(self.b.get(self.at..self.at + 4)?.try_into().ok()?);
        self.at += 4;
        Some(v)
    }
    fn guid(&mut self) -> Option<GUID> {
        let s = self.b.get(self.at..self.at + 16)?;
        self.at += 16;
        Some(GUID::from_u128(u128::from_be_bytes([s[3], s[2], s[1], s[0], s[5], s[4], s[7], s[6], s[8], s[9], s[10], s[11], s[12], s[13], s[14], s[15]])))
    }

    /// Bir kısayolu Windows'un kendi nesnesine yükler; okunan bayt kadar ilerler.
    fn link(&mut self) -> Option<Option<IShellLinkW>> {
        let clsid = self.guid()?;
        unsafe {
            let stream = SHCreateMemStream(Some(&self.b[self.at..]))?;
            let obj: IPersistStream = CoCreateInstance(&clsid, None, CLSCTX_INPROC_SERVER).ok()?;
            obj.Load(&stream).ok()?;
            let mut pos = 0u64;
            stream.Seek(0, STREAM_SEEK_CUR, Some(&mut pos)).ok()?;
            self.at += pos as usize;
            Some(obj.cast::<IShellLinkW>().ok())
        }
    }
}

fn resolve(s: &str) -> String {
    if !s.starts_with('@') {
        return s.to_string();
    }
    let mut buf = [0u16; 260];
    unsafe {
        if SHLoadIndirectString(&HSTRING::from(s), &mut buf, None).is_ok() {
            let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
            return String::from_utf16_lossy(&buf[..n]);
        }
    }
    String::new()
}

fn title_of(link: &IShellLinkW) -> Option<String> {
    unsafe {
        let store: IPropertyStore = link.cast().ok()?;
        // Ayırıcılar (System.AppUserModel.IsDestListSeparator) atlanır.
        let sep = PROPERTYKEY { fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3), pid: 6 };
        if let Ok(v) = store.GetValue(&sep) {
            if PropVariantToStringAlloc(&v).map(|p| { let s = p.to_string().unwrap_or_default(); CoTaskMemFree(Some(p.0 as _)); s }).unwrap_or_default() == "-1" {
                return None;
            }
        }
        let v = store.GetValue(&PKEY_Title).ok()?;
        let p = PropVariantToStringAlloc(&v).ok()?;
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as _));
        let t = resolve(&s);
        (!t.is_empty()).then_some(t)
    }
}

/// Uygulamanın görev listesi (yoksa boş).
pub fn load(e: &Entry) -> Vec<Group> {
    let Some(path) = file_for(e) else { return Vec::new() };
    let Ok(bytes) = std::fs::read(&path) else { return Vec::new() };
    let mut r = Reader { b: &bytes, at: 0 };
    let mut out = Vec::new();
    (|| -> Option<()> {
        let _version = r.u32()?;
        let count = r.u32()?;
        let _ = r.u32()?;
        for _ in 0..count {
            let kind = r.u32()?;
            let mut group = Group { title: String::new(), items: Vec::new() };
            match kind {
                0 => {
                    let n = r.u16()? as usize;
                    let name: Vec<u16> = (0..n).map(|_| r.u16()).collect::<Option<_>>()?;
                    group.title = resolve(&String::from_utf16_lossy(&name));
                }
                1 => {
                    // Windows'un "Son kullanılanlar / Sık kullanılanlar" yeri: öğesi yok.
                    let _ = r.u32()?;
                    if r.u32()? != FOOTER {
                        return None;
                    }
                    continue;
                }
                2 => {}
                _ => return None,
            }
            let n = r.u32()?;
            for _ in 0..n {
                if let Some(link) = r.link()? {
                    if let Some(title) = title_of(&link) {
                        let (icon, target) = link_info(&link);
                        group.items.push(Item { title, icon, target, link });
                    }
                }
            }
            if r.u32()? != FOOTER {
                return None;
            }
            if !group.title.is_empty() {
                group.items.truncate(PER_CATEGORY);
            }
            if !group.items.is_empty() {
                out.push(group);
            }
        }
        Some(())
    })();
    // Görevler en altta (uygulamaya en yakın), Windows'taki gibi.
    out.sort_by_key(|g| g.title.is_empty());
    out
}

impl Item {
    /// Görev çubuğunun yaptığı gibi: kısayolun hedefi + argümanları.
    pub fn run(&self) {
        unsafe {
            let Ok(pidl) = self.link.GetIDList() else { return };
            let mut args = [0u16; 2048];
            let mut dir = [0u16; 260];
            let _ = self.link.GetArguments(&mut args);
            let _ = self.link.GetWorkingDirectory(&mut dir);
            let mut sei = SHELLEXECUTEINFOW {
                cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
                fMask: SEE_MASK_INVOKEIDLIST | SEE_MASK_FLAG_NO_UI,
                lpIDList: pidl as *mut _,
                lpParameters: PCWSTR(args.as_ptr()),
                lpDirectory: if dir[0] == 0 { PCWSTR::null() } else { PCWSTR(dir.as_ptr()) },
                nShow: SW_SHOWNORMAL.0,
                ..Default::default()
            };
            let _ = ShellExecuteExW(&mut sei);
            CoTaskMemFree(Some(pidl as _));
        }
    }
}

#[allow(dead_code)]
pub fn debug_ids(e: &Entry) -> Vec<String> {
    ids(e).iter().map(|i| format!("{:016x} {i}", crc64(i))).collect()
}
