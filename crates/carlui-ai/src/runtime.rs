//! Bu modül "modelleri hangi motorla, ekran kartında nasıl çalıştırırız?"
//! sorusunu çözüyor. ONNX Runtime (MIT, Microsoft) kurucuya konmaz: ilk
//! yapay zekâ modeli indirilirken resmi NuGet paketinden (DirectML derlemesi)
//! `onnxruntime.dll` çıkarılır. DirectML Windows 10/11'de hazır gelir ve her
//! üreticinin (NVIDIA, AMD, Intel) ekran kartında çalışır; açılamazsa işlemciye
//! düşülür.

use ort::session::{builder::GraphOptimizationLevel, Session};
use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, OnceLock},
};

pub const ORT_VERSION: &str = "1.24.4";
pub(crate) const ORT_URL: &str = "https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime.directml/1.24.4/microsoft.ml.onnxruntime.directml.1.24.4.nupkg";
pub(crate) const ORT_SIZE: u64 = 12_458_649;
pub(crate) const ORT_SHA256: &str = "57e9f11b73437bef7a309496135d4c1f96b1a8e9ddba60013fa27bfc1d788681";

pub fn dll() -> PathBuf {
    crate::store::root().join("onnxruntime").join("onnxruntime.dll")
}

pub fn installed() -> bool {
    dll().is_file()
}

/// Motoru indirir (yaklaşık 12 MB). Model kurulumundan önce çağrılır.
pub fn install(cancel: &AtomicBool, progress: &dyn Fn(f32)) -> Result<(), String> {
    if installed() {
        return Ok(());
    }
    let d = dll().parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let pkg = d.join("ort.zip");
    crate::net::fetch(ORT_URL, &pkg, ORT_SIZE, ORT_SHA256, cancel, &|n| progress((n as f32 / ORT_SIZE as f32).min(0.99)))?;
    let x = d.join("x");
    let _ = std::fs::create_dir_all(&x);
    let ok = crate::net::unzip(&pkg, &x, Some("runtimes/win-x64/native/onnxruntime.dll"));
    let src = x.join("runtimes").join("win-x64").join("native").join("onnxruntime.dll");
    let r = if let Err(e) = ok {
        Err(e)
    } else if src.is_file() { std::fs::rename(&src, dll()).map_err(|e| e.to_string()) } else { Err(t!("ONNX Runtime açılamadı", "Couldn't extract ONNX Runtime").into()) };
    let _ = std::fs::remove_dir_all(&x);
    progress(1.0);
    r
}

static ENV: OnceLock<Result<ort::environment::Environment, String>> = OnceLock::new();

fn env() -> Result<&'static ort::environment::Environment, String> {
    ENV.get_or_init(|| {
        if !installed() {
            return Err(t!("Yapay zekâ motoru kurulu değil", "The AI engine isn't installed").into());
        }
        ort::init_from(dll()).map_err(|e| e.to_string())?.with_name("CarlUI").build().map_err(|e| e.to_string())
    })
    .as_ref()
    .map_err(|e| e.clone())
}

/// Ekran kartı kullanılsın mı (ayarlardan). Varsayılan: evet.
pub fn gpu_allowed() -> bool {
    std::env::var("CARLUI_AI_CPU").is_err() && carlui_core::config::load().ai.gpu
}

fn builder(env: &ort::environment::Environment, gpu: bool, fuse: bool) -> Result<ort::session::builder::SessionBuilder, String> {
    let e = |e: ort::Error| e.to_string();
    let mut b = Session::builder(env).map_err(e)?.with_optimization_level(GraphOptimizationLevel::Level3).map_err(|e| e.to_string())?;
    if gpu {
        // DirectML bellek desenini ve paralel yürütmeyi desteklemez.
        if !fuse {
            // BiRefNet gibi büyük ağlarda DirectML birleştirmesi bellek tükenmesine yol açıyor.
            b = b.with_config_entry("ep.dml.disable_graph_fusion", "1").map_err(|e| e.to_string())?;
        }
        b = b.with_memory_pattern(false).map_err(|e| e.to_string())?.with_parallel_execution(false).map_err(|e| e.to_string())?;
        b = b.with_execution_providers([ort::ep::DirectML::default().build().error_on_failure()]).map_err(|e| e.to_string())?;
    } else {
        let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        b = b.with_intra_threads(n.saturating_sub(1).max(1)).map_err(|e| e.to_string())?;
    }
    Ok(b)
}

pub enum Src<'a> {
    Bytes(&'a [u8]),
    File(&'a std::path::Path),
}

/// Oturum açar: önce ekran kartı, olmazsa işlemci.
pub fn session(src: Src) -> Result<Session, String> {
    session_opts(src, true)
}

/// `fuse = false`: DirectML grafik birleştirmesi kapalı (büyük ağlar için).
pub fn session_opts(src: Src, fuse: bool) -> Result<Session, String> {
    let env = env()?;
    let open = |gpu: bool| -> Result<Session, String> {
        let mut b = builder(env, gpu, fuse)?;
        match &src {
            Src::Bytes(m) => b.commit_from_memory(m).map_err(|e| e.to_string()),
            Src::File(p) => b.commit_from_file(p).map_err(|e| e.to_string()),
        }
    };
    if gpu_allowed() {
        match open(true) {
            Ok(s) => return Ok(s),
            Err(e) => carlui_core::warn!("DirectML açılamadı, işlemciye geçiliyor: {e}"),
        }
    }
    open(false)
}

/// Yalnızca işlemcide oturum (ekran kartında çalıştırma bellek vb. yüzünden
/// başarısız olursa yedek olarak).
pub fn session_cpu(src: Src) -> Result<Session, String> {
    let mut b = builder(env()?, false, true)?;
    match src {
        Src::Bytes(m) => b.commit_from_memory(m).map_err(|e| e.to_string()),
        Src::File(p) => b.commit_from_file(p).map_err(|e| e.to_string()),
    }
}
