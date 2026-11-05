//! Bu modül "sürekli dinlerken işlemciyi yormadan birkaç komut sözcüğünü
//! nasıl tanırız?" sorusunu çözüyor: Vosk (Apache-2.0, Alpha Cephei). Resmi
//! `libvosk.dll` model klasöründen yüklenir (kurucuda yok), işlevleri adla
//! alınır. Tanıyıcı yalnızca `command::WORDS_TR` sözcükleriyle (dilbilgisi)
//! kurulur; akış halinde beslenir, cümle bitince sonucu verir.

use std::{
    ffi::{c_char, c_float, c_int, c_void, CStr, CString},
    path::Path,
};
use windows::{
    core::{HSTRING, PCSTR},
    Win32::{
        Foundation::HMODULE,
        Storage::FileSystem::GetShortPathNameW,
        System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_WITH_ALTERED_SEARCH_PATH},
    },
};

type ModelNew = unsafe extern "C" fn(*const c_char) -> *mut c_void;
type ModelFree = unsafe extern "C" fn(*mut c_void);
type FindWord = unsafe extern "C" fn(*mut c_void, *const c_char) -> c_int;
type RecNewGrm = unsafe extern "C" fn(*mut c_void, c_float, *const c_char) -> *mut c_void;
type RecFree = unsafe extern "C" fn(*mut c_void);
type Accept = unsafe extern "C" fn(*mut c_void, *const c_float, c_int) -> c_int;
type Result_ = unsafe extern "C" fn(*mut c_void) -> *const c_char;
type LogLevel = unsafe extern "C" fn(c_int);

struct Api {
    model_free: ModelFree,
    rec_free: RecFree,
    accept: Accept,
    result: Result_,
}

pub struct Recognizer {
    api: Api,
    model: *mut c_void,
    rec: *mut c_void,
}

// Tanıyıcı tek iş parçacığında kullanılır; başka bir iş parçacığına taşınabilir.
unsafe impl Send for Recognizer {}

unsafe fn sym<T>(lib: HMODULE, name: &str) -> Result<T, String> {
    let c = CString::new(name).unwrap();
    let p = GetProcAddress(lib, PCSTR(c.as_ptr() as _)).ok_or_else(|| format!("libvosk: {name} yok"))?;
    Ok(std::mem::transmute_copy(&p))
}

/// Kaldi dosya yollarını ANSI olarak açar; Türkçe harfli kullanıcı klasöründe
/// bozulmasın diye kısa (8.3) yol verilir.
fn ansi_path(p: &Path) -> CString {
    let mut buf = [0u16; 1024];
    let n = unsafe { GetShortPathNameW(&HSTRING::from(p.as_os_str()), Some(&mut buf)) } as usize;
    let s = if n > 0 && n < buf.len() { String::from_utf16_lossy(&buf[..n]) } else { p.to_string_lossy().into_owned() };
    CString::new(s).unwrap_or_default()
}

impl Recognizer {
    /// `dll`: libvosk.dll, `model`: final.mdl'nin bulunduğu klasör.
    pub fn open(dll: &Path, model: &Path, words: &[&str]) -> Result<Self, String> {
        unsafe {
            let lib = LoadLibraryExW(&HSTRING::from(dll.as_os_str()), None, LOAD_WITH_ALTERED_SEARCH_PATH).map_err(|e| format!("libvosk yüklenemedi: {}", e.message()))?;
            if let Ok(f) = sym::<LogLevel>(lib, "vosk_set_log_level") {
                f(-1);
            }
            let model_new: ModelNew = sym(lib, "vosk_model_new")?;
            let find: FindWord = sym(lib, "vosk_model_find_word")?;
            let rec_new: RecNewGrm = sym(lib, "vosk_recognizer_new_grm")?;
            let api = Api { model_free: sym(lib, "vosk_model_free")?, rec_free: sym(lib, "vosk_recognizer_free")?, accept: sym(lib, "vosk_recognizer_accept_waveform_f")?, result: sym(lib, "vosk_recognizer_result")? };
            let mp = ansi_path(model);
            let m = model_new(mp.as_ptr());
            if m.is_null() {
                return Err("Vosk modeli açılamadı".into());
            }
            // Modelin sözlüğünde olmayan sözcük dilbilgisini bozar; ayıkla.
            let known: Vec<&str> = words
                .iter()
                .copied()
                .filter(|w| {
                    let c = CString::new(*w).unwrap();
                    find(m, c.as_ptr()) >= 0
                })
                .collect();
            if known.len() < words.len() {
                carlui_core::info!("vosk: sözlükte olmayan: {:?}", words.iter().filter(|w| !known.contains(w)).collect::<Vec<_>>());
            }
            let mut list: Vec<serde_json::Value> = known.iter().map(|w| (*w).into()).collect();
            list.push("[unk]".into());
            let grm = CString::new(serde_json::Value::Array(list).to_string()).unwrap();
            let rec = rec_new(m, crate::whisper::SR as f32, grm.as_ptr());
            if rec.is_null() {
                (api.model_free)(m);
                return Err("Vosk tanıyıcısı kurulamadı".into());
            }
            Ok(Self { api, model: m, rec })
        }
    }

    /// 16 kHz ses (−1..1) verir; bir cümle bittiyse tanınan metni döndürür.
    pub fn feed(&mut self, pcm: &[f32]) -> Option<String> {
        if pcm.is_empty() {
            return None;
        }
        // Vosk'un float girişi 16 bit ölçeğinde bekler.
        let scaled: Vec<f32> = pcm.iter().map(|v| v * 32767.0).collect();
        unsafe {
            if (self.api.accept)(self.rec, scaled.as_ptr(), scaled.len() as c_int) == 0 {
                return None;
            }
            let p = (self.api.result)(self.rec);
            if p.is_null() {
                return None;
            }
            let j: serde_json::Value = serde_json::from_str(&CStr::from_ptr(p).to_string_lossy()).ok()?;
            let t = j["text"].as_str()?.replace("[unk]", "").trim().to_string();
            (!t.is_empty()).then_some(t)
        }
    }
}

impl Drop for Recognizer {
    fn drop(&mut self) {
        unsafe {
            (self.api.rec_free)(self.rec);
            (self.api.model_free)(self.model);
        }
    }
}
