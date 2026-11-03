//! Dosya imzası. İki yol denenir:
//! 1. Dosyanın içine gömülü Authenticode imzası (WinVerifyTrust).
//! 2. Windows'un kendi dosyalarının çoğu tek tek değil, sistem kataloğu
//!    üzerinden imzalıdır: dosyanın özeti kataloglarda aranır.
//! Kontrol yavaş olabildiği için ayrı bir iş parçacığında yapılır ve sonuç yol
//! başına önbelleğe alınır; arayüz bu sırada "…" gösterir.

use std::{
    collections::{HashMap, HashSet},
    sync::{mpsc, Arc, Mutex},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Security::{Cryptography::Catalog::*, WinTrust::*},
        Storage::FileSystem::*,
    },
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sign {
    Pending,
    /// Dosyanın kendi imzası var.
    Signed,
    /// Windows kataloğunda kayıtlı (Microsoft dosyaları).
    Catalog,
    Unsigned,
    /// Dosyaya ulaşılamadı (yol yok / erişim yok).
    Unknown,
}

impl Sign {
    pub fn label(self) -> &'static str {
        match self {
            Sign::Pending => "…",
            Sign::Signed => "İmzalı",
            Sign::Catalog => "Windows",
            Sign::Unsigned => "İmzasız",
            Sign::Unknown => "—",
        }
    }

    /// Sıralamada önce gelen küçük olur: imzasızlar en üste.
    pub fn rank(self) -> u8 {
        match self {
            Sign::Unsigned => 0,
            Sign::Unknown => 1,
            Sign::Pending => 2,
            Sign::Signed => 3,
            Sign::Catalog => 4,
        }
    }
}

fn embedded(path: &HSTRING) -> bool {
    unsafe {
        let mut file = WINTRUST_FILE_INFO { cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32, pcwszFilePath: PCWSTR(path.as_ptr()), ..Default::default() };
        let mut data = WINTRUST_DATA {
            cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
            dwUIChoice: WTD_UI_NONE,
            fdwRevocationChecks: WTD_REVOKE_NONE,
            dwUnionChoice: WTD_CHOICE_FILE,
            Anonymous: WINTRUST_DATA_0 { pFile: &mut file },
            dwStateAction: WTD_STATEACTION_VERIFY,
            dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
            ..Default::default()
        };
        let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
        let r = WinVerifyTrust(HWND(INVALID_HANDLE_VALUE.0), &mut action, &mut data as *mut _ as *mut _);
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        WinVerifyTrust(HWND(INVALID_HANDLE_VALUE.0), &mut action, &mut data as *mut _ as *mut _);
        r == 0
    }
}

fn in_catalog(path: &HSTRING) -> bool {
    unsafe {
        let Ok(h) = CreateFileW(path, FILE_GENERIC_READ.0, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, None, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, None) else { return false };
        let mut found = false;
        let mut ctx = 0isize;
        if CryptCATAdminAcquireContext2(&mut ctx, None, w!("SHA256"), None, 0).is_ok() {
            let mut len = 0u32;
            let _ = CryptCATAdminCalcHashFromFileHandle2(ctx, h, &mut len, None, 0);
            let mut hash = vec![0u8; len as usize];
            if len > 0 && CryptCATAdminCalcHashFromFileHandle2(ctx, h, &mut len, Some(hash.as_mut_ptr()), 0).is_ok() {
                let cat = CryptCATAdminEnumCatalogFromHash(ctx, &hash, 0, None);
                if cat != 0 {
                    found = true;
                    let _ = CryptCATAdminReleaseCatalogContext(ctx, cat, 0);
                }
            }
            let _ = CryptCATAdminReleaseContext(ctx, 0);
        }
        let _ = CloseHandle(h);
        found
    }
}

pub fn check(path: &str) -> Sign {
    if path.is_empty() || !std::path::Path::new(path).exists() {
        return Sign::Unknown;
    }
    let p = HSTRING::from(path);
    if embedded(&p) {
        Sign::Signed
    } else if in_catalog(&p) {
        Sign::Catalog
    } else {
        Sign::Unsigned
    }
}

/// Arka planda imza kontrolü yapan önbellek.
#[derive(Clone)]
pub struct Checker {
    cache: Arc<Mutex<HashMap<String, Sign>>>,
    queued: Arc<Mutex<HashSet<String>>>,
    tx: mpsc::Sender<String>,
}

impl Checker {
    /// `done`: bir sonuç geldiğinde çağrılır (arayüzü yenilemek için).
    pub fn new(done: impl Fn() + Send + 'static) -> Self {
        let cache: Arc<Mutex<HashMap<String, Sign>>> = Arc::default();
        let (tx, rx) = mpsc::channel::<String>();
        let c = cache.clone();
        std::thread::Builder::new()
            .name("imza".into())
            .spawn(move || {
                unsafe {
                    let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
                }
                while let Ok(path) = rx.recv() {
                    let s = check(&path);
                    if let Ok(mut m) = c.lock() {
                        m.insert(path.to_lowercase(), s);
                    }
                    done();
                }
            })
            .ok();
        Self { cache, queued: Arc::default(), tx }
    }

    pub fn get(&self, path: &str) -> Sign {
        if path.is_empty() {
            return Sign::Unknown;
        }
        let key = path.to_lowercase();
        if let Some(s) = self.cache.lock().ok().and_then(|m| m.get(&key).copied()) {
            return s;
        }
        if let Ok(mut q) = self.queued.lock() {
            if q.insert(key) {
                let _ = self.tx.send(path.to_string());
            }
        }
        Sign::Pending
    }
}
