//! Windows Hello ile kilit açma. Sorun: her açılışta uzun ana parolayı yazmak
//! zahmetli; ama parolayı bir yere kaydetmek kasanın güvenliğini bozar.
//! Çözüm: Windows Hello (yüz, parmak izi, PIN) bu kullanıcıya bağlı, TPM'de
//! duran bir RSA anahtarı tutar. Kasaya özgü sabit bir metni imzalatırız;
//! RSA PKCS#1 v1.5 imzası belirlenimci olduğu için her seferinde aynı imza
//! çıkar. İmzanın SHA-256 özeti ana anahtarı saran anahtar olur (bkz.
//! `vault::Vault::enable_hello`); sarmal ayrıca DPAPI ile bu Windows
//! kullanıcısına kilitlenir. İmza hiçbir yere yazılmaz.
//!
//! Hello penceresi masaüstü uygulamalarında bazen arkada açılır; imza
//! beklenirken ayrı bir iş parçacığı onu bulup öne getirir.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant};
use windows::{
    core::{HSTRING, PCWSTR},
    Security::Credentials::{KeyCredentialCreationOption, KeyCredentialManager, KeyCredentialStatus},
    Storage::Streams::{DataReader, DataWriter},
    Win32::{
        Foundation::{HLOCAL, HWND, LocalFree},
        Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB},
        System::Com::{CoInitializeEx, COINIT_MULTITHREADED},
        UI::WindowsAndMessaging::{AllowSetForegroundWindow, FindWindowW, SetForegroundWindow, ASFW_ANY},
    },
};

/// Hello'daki anahtarın adı (kullanıcı başına bir tane).
const CREDENTIAL: &str = "CarlUI.Vault";

/// 0 bilinmiyor, 1 var, 2 yok.
static AVAILABLE: AtomicU8 = AtomicU8::new(0);
static PROBING: AtomicBool = AtomicBool::new(false);

/// Hello kullanılabilir mi; henüz bilinmiyorsa None (bkz. `probe`).
pub fn available() -> Option<bool> {
    match AVAILABLE.load(Ordering::Relaxed) {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

/// Hello desteğini arka planda yoklar (birkaç yüz ms sürebilir); bitince `done`.
pub fn probe(done: impl FnOnce() + Send + 'static) {
    // Her çizimde çağrılır: yoklama sürerken ikinci iş parçacığı açılmasın.
    if available().is_some() || PROBING.swap(true, Ordering::Relaxed) {
        return;
    }
    std::thread::spawn(move || {
        let ok = com(|| KeyCredentialManager::IsSupportedAsync()?.get()).unwrap_or(false);
        AVAILABLE.store(if ok { 1 } else { 2 }, Ordering::Relaxed);
        done();
    });
}

fn com<T>(f: impl FnOnce() -> windows::core::Result<T>) -> windows::core::Result<T> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    f()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelloError {
    /// Kullanıcı vazgeçti.
    Canceled,
    /// Hello kurulu değil ya da anahtar silinmiş.
    Missing,
    Other(String),
}

impl std::fmt::Display for HelloError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HelloError::Canceled => f.write_str(carlui_core::t!("Windows Hello iptal edildi.", "Windows Hello was canceled.")),
            HelloError::Missing => f.write_str(carlui_core::t!("Windows Hello anahtarı bulunamadı; ana parolayla açıp yeniden etkinleştirin.", "Windows Hello key not found; unlock with the master password and turn it on again.")),
            HelloError::Other(e) => write!(f, "Windows Hello: {e}"),
        }
    }
}

fn status(s: KeyCredentialStatus) -> HelloError {
    match s {
        KeyCredentialStatus::UserCanceled => HelloError::Canceled,
        KeyCredentialStatus::NotFound => HelloError::Missing,
        s => HelloError::Other(format!("status {}", s.0)),
    }
}

/// Hello penceresi arkada açılırsa öne getirir (en çok 20 sn arar).
fn raise_prompt() {
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
    std::thread::spawn(|| {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(120));
            let h = unsafe { FindWindowW(windows::core::w!("Credential Dialog Xaml Host"), PCWSTR::null()) };
            if let Ok(h) = h {
                if !h.is_invalid() {
                    unsafe {
                        let _ = SetForegroundWindow(h);
                    }
                    return;
                }
            }
        }
    });
}

/// `challenge`'ı Hello anahtarıyla imzalar. `create` ise anahtar yoksa (ya da
/// yeniden etkinleştirmede) yenisi oluşturulur. Hello penceresi açılır; çağıran
/// iş parçacığı imza bitene kadar bekler (arayüz iş parçacığında çağırmayın).
pub fn sign(challenge: &[u8], create: bool, _owner: HWND) -> Result<Vec<u8>, HelloError> {
    let name = HSTRING::from(CREDENTIAL);
    raise_prompt();
    com(|| {
        let got = if create { KeyCredentialManager::RequestCreateAsync(&name, KeyCredentialCreationOption::ReplaceExisting)?.get()? } else { KeyCredentialManager::OpenAsync(&name)?.get()? };
        let st = got.Status()?;
        if st != KeyCredentialStatus::Success {
            return Ok(Err(status(st)));
        }
        let cred = got.Credential()?;
        let w = DataWriter::new()?;
        w.WriteBytes(challenge)?;
        let res = cred.RequestSignAsync(&w.DetachBuffer()?)?.get()?;
        let st = res.Status()?;
        if st != KeyCredentialStatus::Success {
            return Ok(Err(status(st)));
        }
        let buf = res.Result()?;
        let r = DataReader::FromBuffer(&buf)?;
        let mut out = vec![0u8; buf.Length()? as usize];
        r.ReadBytes(&mut out)?;
        Ok(Ok(out))
    })
    .unwrap_or_else(|e| Err(HelloError::Other(e.message().to_string())))
}

/// Hello anahtarını siler (kasada Hello kapatılınca).
pub fn forget() {
    std::thread::spawn(|| {
        let _ = com(|| KeyCredentialManager::DeleteAsync(&HSTRING::from(CREDENTIAL))?.get());
    });
}

fn blob_out(b: &CRYPT_INTEGER_BLOB) -> Vec<u8> {
    unsafe {
        let v = std::slice::from_raw_parts(b.pbData, b.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(b.pbData as _));
        v
    }
}

/// DPAPI: veriyi bu Windows kullanıcısına kilitler.
pub fn protect(data: &[u8]) -> Option<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe { CryptProtectData(&input, PCWSTR::null(), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out) }.ok()?;
    Some(blob_out(&out))
}

pub fn unprotect(data: &[u8]) -> Option<zeroize::Zeroizing<Vec<u8>>> {
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe { CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out) }.ok()?;
    Some(zeroize::Zeroizing::new(blob_out(&out)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpapi_gidip_gelir() {
        let p = protect(b"gizli").unwrap();
        assert_ne!(&p[..], b"gizli");
        assert_eq!(&unprotect(&p).unwrap()[..], b"gizli");
        assert!(unprotect(b"bozuk veri").is_none());
    }
}
