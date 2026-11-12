//! Canlı hava durumu: Open-Meteo verisini sahne için sade bir özete çevirir.
//! Ağ çağrıları (`net`) arka plan iş parçacığından kullanılmalıdır.

pub mod debug;
pub mod map;
pub mod net;
pub mod parse;
pub mod state;

pub use debug::debug_override;
pub use map::from_readings;
pub use net::{fetch, forecast_url, search, search_url};
pub use parse::{parse_forecast, parse_places};

/// Gökyüzü durumu.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sky {
    Clear,
    Cloudy,
    Rain,
    Snow,
}

/// Sahnenin ihtiyaç duyduğu hava özeti.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Weather {
    pub sky: Sky,
    /// 0.0..=1.0 (yağışta şiddet, açık/bulutluda bulut oranı)
    pub intensity: f32,
    pub is_day: bool,
    pub thunder: bool,
}

/// Konum arama sonucu.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub name: String,
    /// İl/bölge (admin1); boş olabilir.
    pub region: String,
    pub country: String,
    pub lat: f64,
    pub lon: f64,
}
