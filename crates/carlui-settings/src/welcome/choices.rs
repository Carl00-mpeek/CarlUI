//! Turun yeni seçimleri (vurgu rengi, dock hayvanı, sahne) ve son sayfadaki
//! "neler yapabilirsin" kartları. `model` gibi pencereden bağımsızdır; her
//! seçim ayarda yalnızca kendi alanını değiştirir (testler buna bakar).

use super::model;
use carlui_core::{
    config::{AccentSource, Config},
    live::{self, LiveScene},
    theme::Palette,
};

/// Vurgu rengini hazır paletlerden biri yapar (duvar kâğıdı/özel renk bırakılır).
pub fn set_accent(c: &mut Config, p: Palette) {
    c.theme.palette = p;
    c.theme.accent_source = AccentSource::Palette;
}

/// Şu an seçili palet; vurgu duvar kâğıdından ya da özel renkten geliyorsa `None`.
pub fn accent_selected(c: &Config) -> Option<Palette> {
    (c.theme.accent_source == AccentSource::Palette).then_some(c.theme.palette)
}

/// Tur kartlarındaki hayvanlar (ana türler; renkler ayrı satırda).
pub const PETS: [&str; 8] = ["cat", "dog", "mouse", "squirrel", "lizard", "spider", "snake", "dolphin"];

pub fn pet_label(id: &str) -> &'static str {
    match id {
        "" => t!("Yok", "None"),
        "cat" => t!("Kedi", "Cat"),
        "dog" => t!("Köpek", "Dog"),
        "mouse" => t!("Fare", "Mouse"),
        "squirrel" => t!("Sincap", "Squirrel"),
        "lizard" => t!("Kertenkele", "Lizard"),
        "spider" => t!("Örümcek", "Spider"),
        "snake" => t!("Yılan", "Snake"),
        "dolphin" => t!("Yunus", "Dolphin"),
        // Kullanıcı paketi ya da tanınmayan kimlik.
        _ => t!("Özel", "Custom"),
    }
}

/// Seçili hayvanın ana kartı: renk varyantları kedi/köpeğe, fare ve sincap
/// kendilerine bağlanır (Ayarlar'daki dock sayfasıyla aynı kural).
pub fn pet_main(pet: &str) -> &str {
    let pet = pet.trim();
    if pet == "mouse" || pet == "squirrel" {
        pet
    } else {
        live::pet_base(pet)
    }
}

/// Ana kart tıklanınca: "" hayvanı kapatır; aynı türün rengi seçiliyse korunur.
pub fn choose_pet(c: &mut Config, id: &str) {
    if id.is_empty() {
        c.dock.live.pet.clear();
    } else if pet_main(&c.dock.live.pet) != id {
        c.dock.live.pet = id.into();
    }
}

/// Renk satırındaki seçim: varyant kimliği olduğu gibi yazılır.
pub fn choose_variant(c: &mut Config, id: &str) {
    if live::is_builtin(id) {
        c.dock.live.pet = id.into();
    }
}

/// Kedi ve köpeğin renkleri: (kimlik, ad).
pub fn variants(pet: &str) -> Option<[(&'static str, &'static str); 4]> {
    match pet_main(pet) {
        "cat" => Some([("cat", t!("Turuncu", "Orange")), ("cat_black", t!("Kara", "Black")), ("cat_gray", t!("Gri", "Gray")), ("cat_white", t!("Beyaz", "White"))]),
        "dog" => Some([("dog", t!("Altın", "Golden")), ("dog_black", t!("Kara", "Black")), ("dog_brown", t!("Kahverengi", "Brown")), ("dog_white", t!("Beyaz", "White"))]),
        _ => None,
    }
}

/// Sahne satırı (Ayarlar'daki sırayla).
pub fn scenes() -> [(LiveScene, &'static str); 7] {
    [
        (LiveScene::None, t!("Yok", "None")),
        (LiveScene::Rain, t!("Yağmur", "Rain")),
        (LiveScene::Snow, t!("Kar", "Snow")),
        (LiveScene::Night, t!("Gece", "Night")),
        (LiveScene::Clouds, t!("Bulut", "Clouds")),
        (LiveScene::Clock, t!("Saat", "Clock")),
        (LiveScene::Aquarium, t!("Akvaryum", "Aquarium")),
    ]
}

pub fn set_scene(c: &mut Config, s: LiveScene) {
    c.dock.live.scene = s;
}

/// Son sayfadaki bir "neler yapabilirsin" kartı.
pub struct Highlight {
    pub glyph: char,
    pub title: &'static str,
    pub desc: &'static str,
    /// (etiket, tuş) satırları; ayardaki gerçek kısayollar.
    pub keys: Vec<(&'static str, String)>,
}

/// Dört kart: dock, ada, arama, kısayollar. Tuşlar ayardan okunur; kapalı
/// modülün ya da boş kısayolun satırı çıkmaz.
pub fn highlights(c: &Config) -> Vec<Highlight> {
    let dock = if c.dock.taskbar {
        Highlight { glyph: '\u{E8A9}', title: t!("Görev çubuğu", "Taskbar"), desc: t!("Uygulamaları sabitle, sürükleyerek sırala; sağ tıkla daha fazlası.", "Pin apps and drag to reorder; right-click for more."), keys: vec![] }
    } else {
        Highlight { glyph: '\u{E8A9}', title: "Dock", desc: t!("Uygulamaları sabitle, sürükleyerek sırala; sağ tıkla daha fazlası.", "Pin apps and drag to reorder; right-click for more."), keys: vec![] }
    };
    let island = Highlight { glyph: '\u{E7F4}', title: t!("Ada", "Island"), desc: t!("Saat, çalan müzik ve bildirimler ekranın üstünde tek yerde.", "Clock, what's playing and notifications in one place at the top."), keys: vec![] };
    let search_key = c.shortcuts.search.trim();
    let search = if c.launcher.enabled {
        let keys = if search_key.is_empty() { vec![] } else { vec![(t!("Aç", "Open"), search_key.to_string())] };
        Highlight { glyph: '\u{E721}', title: t!("Arama", "Search"), desc: t!("Uygulama, dosya ve web'i tek kutudan bul.", "Find apps, files and the web from one box."), keys }
    } else {
        Highlight { glyph: '\u{E721}', title: t!("Arama", "Search"), desc: t!("Kapalı; Ayarlar > Arama'dan açabilirsin.", "Off; turn it on in Settings > Search."), keys: vec![] }
    };
    let keys: Vec<_> = model::shortcut_list(c, false).into_iter().take(2).collect();
    let shortcuts = Highlight { glyph: '\u{E765}', title: t!("Kısayollar", "Shortcuts"), desc: t!("Hepsi Ayarlar > Kısayollar'dan değişir.", "All of them change in Settings > Shortcuts."), keys };
    vec![dock, island, search, shortcuts]
}
