//! "Canlı dock" ayarları: dock üstünde yaşayan küçük evcil hayvan ve dock'un
//! içinde pencere camı gibi görünen sahneler (yağmur, kar, gece, bulut).
//!
//! Her şey varsayılan olarak kapalıdır. Eski ayar dosyalarında `[dock.live]`
//! bölümü yoksa `serde(default)` ile kapalı değerler gelir.

use serde::{Deserialize, Serialize};

/// Dock'un camında görünen sahne.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LiveScene {
    #[default]
    None,
    Rain,
    Snow,
    /// Gece: yıldızlar ve ateşböcekleri.
    Night,
    /// Gündüz: süzülen bulutlar ve hafif güneş pusu.
    Clouds,
    /// Saate göre: şafak, gündüz, akşam, gece kendiliğinden değişir.
    Clock,
    /// Akvaryum: süzülen balıklar ve yükselen kabarcıklar.
    Aquarium,
}

/// Efektlerin yoğunluğu (ikonların okunurluğu için üst sınırla).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LiveIntensity {
    #[default]
    Subtle,
    Normal,
}

/// Eski sürümlerdeki `coat` (kedi rengi) anahtarı artık yok sayılır; serde
/// bilinmeyen alanı sessizce atlar.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct LiveConfig {
    /// Evcil hayvan: "" = yok, `BUILTIN_PETS`'teki bir ad = yerleşik hayvan,
    /// başka bir ad = `%LOCALAPPDATA%\CarlUI\pets\<ad>\` altındaki kullanıcı paketi.
    pub pet: String,
    pub scene: LiveScene,
    pub intensity: LiveIntensity,
    /// Pilde çalışırken her şeyi duraklat.
    pub pause_on_battery: bool,
    /// Sahne gerçek havayı izlesin (Open-Meteo); kapalıyken hiç istek yapılmaz.
    pub weather: crate::live_weather::LiveWeather,
}

impl Default for LiveConfig {
    fn default() -> Self {
        Self { pet: String::new(), scene: LiveScene::None, intensity: LiveIntensity::Subtle, pause_on_battery: true, weather: Default::default() }
    }
}

impl LiveConfig {
    /// Evcil hayvan seçili mi (boşluklar yok sayılır).
    pub fn has_pet(&self) -> bool {
        !self.pet.trim().is_empty()
    }

    /// Özellik tamamen kapalı mı (kapalıyken hiçbir kaynak ayrılmaz).
    pub fn is_off(&self) -> bool {
        !self.has_pet() && self.scene == LiveScene::None && !self.weather.active()
    }
}

/// Yerleşik kedinin `pet` değeri.
pub const BUILTIN_CAT: &str = "cat";

/// Yerleşik hayvanların `pet` değerleri (ayar sayfasındaki sırayla).
pub const BUILTIN_PETS: [&str; 14] = [
    BUILTIN_CAT, "cat_black", "cat_gray", "cat_white", "dog", "dog_black", "dog_brown", "dog_white", "mouse", "squirrel", "lizard", "spider", "snake", "dolphin",
];

/// Renk/tür varyantının temel hayvanı: `cat_*` -> "cat", `dog_*`, "mouse" ve "squirrel" -> "dog"
/// (köpekle aynı şerit düzeni); diğerleri kendisi.
pub fn pet_base(id: &str) -> &str {
    if id.starts_with("cat_") {
        "cat"
    } else if id.starts_with("dog_") || id == "mouse" || id == "squirrel" {
        "dog"
    } else {
        id
    }
}

/// `pet` değeri yerleşik bir hayvan mı?
pub fn is_builtin(pet: &str) -> bool {
    BUILTIN_PETS.contains(&pet)
}

/// Kullanıcı paketlerinin klasörü: `%LOCALAPPDATA%\CarlUI\pets`.
pub fn pets_dir() -> std::path::PathBuf {
    crate::paths::data_dir().join("pets")
}

/// Paket klasör adı yalnızca `[A-Za-z0-9_-]`, 1-32 karakter (klasör dışına
/// çıkılamasın, yerleşik adla karışmasın) ve Windows'un ayrılmış aygıt adı
/// olmamalı ("con", "nul", "com1"… klasör olarak açılamaz).
pub fn valid_pack_name(name: &str) -> bool {
    (1..=32).contains(&name.len()) && !is_builtin(name) && !is_reserved_name(name) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Windows'un ayrılmış aygıt adı mı (CON, PRN, AUX, NUL, COM0-9, LPT0-9)?
/// Büyük/küçük harf fark etmez; Windows sondaki nokta ve boşlukları ve
/// uzantıyı yok saydığı için "con.", "nul " ve "aux.txt" de ayrılmış sayılır.
pub fn is_reserved_name(name: &str) -> bool {
    let stem = name.trim_end_matches(['.', ' ']);
    let stem = stem.split('.').next().unwrap_or(stem).trim_end_matches(' ');
    let u = stem.to_ascii_uppercase();
    matches!(u.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((u.starts_with("COM") || u.starts_with("LPT")) && u.len() == 4 && u.as_bytes()[3].is_ascii_digit())
}

/// Kurulu kullanıcı paketleri: (klasör adı, görünen ad). Yalnızca içinde
/// `pet.json` olan geçerli adlı klasörler; doğrulamanın tamamı dock'ta.
pub fn list_user_packs() -> Vec<(String, String)> {
    let Ok(rd) = std::fs::read_dir(pets_dir()) else { return Vec::new() };
    let mut out: Vec<(String, String)> = rd
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| {
            let dir = e.file_name().to_string_lossy().to_string();
            if !valid_pack_name(&dir) {
                return None;
            }
            let json = e.path().join("pet.json");
            let len = std::fs::metadata(&json).ok()?.len();
            if len > 16 * 1024 {
                return None;
            }
            let shown = std::fs::read(&json)
                .ok()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                .and_then(|v| v.get("name")?.as_str().map(|s| s.trim().chars().take(64).collect::<String>()))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| dir.clone());
            Some((dir, shown))
        })
        .take(32)
        .collect();
    out.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn paket_adi_denetimi() {
        assert!(valid_pack_name("tilki_2"));
        assert!(valid_pack_name("My-Pet"));
        assert!(!valid_pack_name(""));
        for b in BUILTIN_PETS {
            assert!(!valid_pack_name(b), "{b}");
        }
        assert!(!valid_pack_name("..\\x"));
        assert!(!valid_pack_name("a/b"));
        assert!(!valid_pack_name("çok"));
        assert!(!valid_pack_name(&"a".repeat(33)));
    }

    #[test]
    fn ayrilmis_windows_adlari() {
        for n in ["con", "CON", "nul", "Aux", "prn", "com1", "COM9", "lpt1", "Lpt0"] {
            assert!(!valid_pack_name(n), "{n}");
            assert!(is_reserved_name(n), "{n}");
        }
        // Sondaki nokta/boşluk ve uzantı Windows'ta yok sayılır.
        for n in ["con.", "nul ", "aux. .", "com1.txt", "LPT2.png", "con ."] {
            assert!(is_reserved_name(n), "{n}");
        }
        // Benzeyen ama ayrılmış olmayan adlar geçer.
        for n in ["console", "con_1", "null", "com10", "lpt", "comx", "auxi", "my-con"] {
            assert!(!is_reserved_name(n), "{n}");
            assert!(valid_pack_name(n), "{n}");
        }
    }

    #[test]
    fn eski_dosya_kapali_gelir() {
        // `[dock.live]` olmayan eski bir dosya.
        let c: Config = toml::from_str("[dock]\nicon_size = 40.0\n").unwrap();
        assert_eq!(c.dock.icon_size, 40.0);
        assert!(c.dock.live.is_off());
        assert!(c.dock.live.pause_on_battery);
        assert_eq!(c.dock.live, LiveConfig::default());
    }

    #[test]
    fn kismi_bolum_okunur() {
        let c: Config = toml::from_str("[dock.live]\nscene = \"rain\"\n").unwrap();
        assert_eq!(c.dock.live.scene, LiveScene::Rain);
        assert!(!c.dock.live.has_pet());
        assert_eq!(c.dock.live.intensity, LiveIntensity::Subtle);
    }

    #[test]
    fn gidip_gelir() {
        let mut c = Config::default();
        c.dock.live = LiveConfig { pet: "dolphin".into(), scene: LiveScene::Aquarium, intensity: LiveIntensity::Normal, pause_on_battery: false, weather: crate::live_weather::LiveWeather { enabled: true, place: "Ankara".into(), lat: 39.93, lon: 32.86, day_night: true } };
        let s = toml::to_string(&c).unwrap();
        let back: Config = toml::from_str(&s).unwrap();
        assert_eq!(back.dock.live, c.dock.live);
    }

    #[test]
    fn temel_hayvan() {
        for (id, base) in [("cat", "cat"), ("cat_gray", "cat"), ("dog", "dog"), ("dog_brown", "dog"), ("mouse", "dog"), ("squirrel", "dog"), ("lizard", "lizard"), ("", "")] {
            assert_eq!(pet_base(id), base, "{id}");
        }
        // Her yerleşik adın temeli yine yerleşik bir ad olmalı.
        for b in BUILTIN_PETS {
            assert!(is_builtin(pet_base(b)), "{b}");
        }
    }

    #[test]
    fn eski_kedi_rengi_yok_sayilir() {
        let c: Config = toml::from_str("[dock.live]\npet = \"cat\"\ncoat = \"black\"\nscene = \"snow\"\n").unwrap();
        assert_eq!(c.dock.live.pet, "cat");
        assert_eq!(c.dock.live.scene, LiveScene::Snow);
    }
}
