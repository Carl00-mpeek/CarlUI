//! CarlUI ortak çekirdeği.
//!
//! Bu crate, bağımsız süreçler olarak çalışan modüllerin (dock, tray, bildirim…)
//! ortak dilini sağlar: tek yapılandırma dosyası, tema, log ve süreçler arası
//! olay yolu. Modüller birbirini doğrudan çağırmaz; yalnızca buradan haberleşir.

// `t!` makrosu diğer modüllerde de görünsün diye ilk sırada.
#[macro_use]
pub mod lang;
pub mod accent;
pub mod ai;
pub mod bus;
pub mod config;
pub mod cursor;
pub mod hotkey;
pub mod keys;
pub mod layouts;
pub mod live;
pub mod live_weather;
pub mod log;
pub mod looks;
pub mod paths;
pub mod power;
pub mod quick;
pub mod record;
pub mod theme;
pub mod tools;
pub mod vaultlink;
pub mod vcard;
pub mod weather;
pub mod win;
pub mod wired;

/// Modül bu kodla çıkarsa gözetmen onu çöküş saymadan hemen yeniden açar
/// (ör. açık/koyu tema değişince cam yüzeyler baştan kurulsun diye).
pub const RELOAD_EXIT: i32 = 75;
