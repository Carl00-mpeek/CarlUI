//! CarlUI ortak çekirdeği.
//!
//! Bu crate, bağımsız süreçler olarak çalışan modüllerin (dock, tray, bildirim…)
//! ortak dilini sağlar: tek yapılandırma dosyası, tema, log ve süreçler arası
//! olay yolu. Modüller birbirini doğrudan çağırmaz; yalnızca buradan haberleşir.

// `t!` makrosu diğer modüllerde de görünsün diye ilk sırada.
#[macro_use]
pub mod lang;
pub mod bus;
pub mod config;
pub mod cursor;
pub mod keys;
pub mod log;
pub mod paths;
pub mod power;
pub mod quick;
pub mod theme;
pub mod vaultlink;
pub mod vcard;
pub mod win;
