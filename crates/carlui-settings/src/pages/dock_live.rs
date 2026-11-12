//! Dock sayfasındaki "Canlı dock" bölümü: evcil hayvan (yerleşikler ve
//! kullanıcı paketleri), sahne, yoğunluk ve pilde duraklatma. Açıklamalar
//! satır başlığının yanındaki "i" baloncuğunda. Ayarlar Görünümler'e (looks)
//! girmez.

use crate::{
    ui::{edit, Flow, P},
    App,
};
use carlui_core::{
    config::DockPosition,
    live::{self, LiveIntensity, LiveScene},
};
use std::{cell::RefCell, time::Instant};

/// Bir seçim satırındaki en çok kullanıcı paketi; fazlası alt satırlara iner
/// (seçim satırı kaydırılamıyor). `list_user_packs` zaten en çok 32 döndürür.
const PER_ROW: usize = 5;

thread_local! {
    /// Paket listesi: her boyamada diske gitmemek için birkaç saniye saklanır.
    static PACKS: RefCell<Option<(Instant, Vec<(String, String)>)>> = const { RefCell::new(None) };
}

fn user_packs() -> Vec<(String, String)> {
    PACKS.with(|c| {
        let mut c = c.borrow_mut();
        match c.as_ref() {
            Some((t, v)) if t.elapsed().as_secs() < 5 => v.clone(),
            _ => {
                let v = live::list_user_packs();
                *c = Some((Instant::now(), v.clone()));
                v
            }
        }
    })
}

/// Yeni paket yazılınca liste bir sonraki boyamada diskten yeniden okunsun.
pub fn refresh_packs() {
    PACKS.with(|c| *c.borrow_mut() = None);
}

/// Seçim düğmesine sığsın diye uzun adları kısaltır.
fn short(s: &str) -> String {
    if s.chars().count() <= 12 {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(11).collect::<String>())
    }
}

/// Yerleşik hayvanların görünen adları (`live::BUILTIN_PETS` sırasıyla).
fn builtin_label(id: &str) -> &'static str {
    match id {
        "cat" => t!("Kedi", "Cat"),
        "lizard" => t!("Kertenkele", "Lizard"),
        "spider" => t!("Örümcek", "Spider"),
        "snake" => t!("Yılan", "Snake"),
        "dolphin" => t!("Yunus", "Dolphin"),
        _ => "?",
    }
}

fn pet_rows(app: &App, p: &mut P, f: &mut Flow, pet: &str) {
    let mut items = vec![(t!("Yok", "None"), edit(|c| c.dock.live.pet.clear()))];
    for id in live::BUILTIN_PETS {
        items.push((builtin_label(id), edit(move |c| c.dock.live.pet = id.into())));
    }
    let sel = if pet.is_empty() { 0 } else { live::BUILTIN_PETS.iter().position(|b| *b == pet).map_or(usize::MAX, |i| i + 1) };
    f.choice(
        p,
        t!("Evcil hayvan", "Pet"),
        t!(
            "Dock'un üstünde yaşayan küçük bir hayvan. Tıklamayı hiç engellemez; yalnızca alttaki ana dock'ta görünür.\nKedi, kertenkele ve örümcek oturur, uyur, gezinir, imlece bakar ve bir uygulama açılınca uyanır.\nYılan dock'un üstünde süzülür, kenarına dolanır, kıvrılıp başını kaldırır ve başıyla imleci izler.\nYunus camın içinde soluk yüzer, ara sıra sudan sıçrayıp geri dalar.",
            "A small pet living on the dock. It never blocks clicks and only appears on the bottom main dock.\nThe cat, lizard and spider sit, sleep, wander, watch the cursor and wake up when an app opens.\nThe snake glides along the dock, wraps around its edge, coils up, raises its head and follows the cursor with it.\nThe dolphin swims faintly inside the glass and now and then leaps out and dives back in."
        ),
        &items,
        sel,
        78.0,
    );

    let packs = user_packs();
    let is_user = !pet.is_empty() && !live::is_builtin(pet);
    for (i, chunk) in packs.chunks(PER_ROW).enumerate() {
        let labels: Vec<String> = chunk.iter().map(|(_, n)| short(n)).collect();
        let row: Vec<(&str, _)> = chunk
            .iter()
            .zip(&labels)
            .map(|((dir, _), label)| {
                let dir = dir.clone();
                (label.as_str(), edit(move |c| c.dock.live.pet = dir.clone()))
            })
            .collect();
        let sel = chunk.iter().position(|(d, _)| d == pet).unwrap_or(usize::MAX);
        let (label, info) = if i == 0 {
            (
                t!("Paketlerim", "My packs"),
                t!(
                    "Kendi hayvan paketlerin: pets klasöründe, içinde pet.json ve PNG şeritleri olan her klasör bir paket olur. Ad bilinmiyorsa klasör adı görünür.",
                    "Your own pet packs: every folder in the pets folder that holds a pet.json and PNG strips becomes a pack. If it has no name, the folder name is shown."
                ),
            )
        } else {
            ("", "")
        };
        f.choice(p, label, info, &row, sel, 96.0);
    }
    if is_user && !packs.iter().any(|(d, _)| d == pet) {
        f.note(p, t!("Seçili paket bulunamadı; pets klasörünü denetle.", "The selected pack was not found; check the pets folder."));
    }
    if !pet.is_empty() {
        f.note(p, &format!("{} {}", t!("Kendi paketlerin:", "Your own packs:"), live::pets_dir().display()));
        if app.cfg.dock.position != DockPosition::Bottom {
            f.note(p, t!("Evcil hayvan yalnızca dock alttayken görünür.", "The pet only appears when the dock is at the bottom."));
        }
    }
}

fn scene_row(p: &mut P, f: &mut Flow, scene: LiveScene) {
    let sel = match scene {
        LiveScene::None => 0,
        LiveScene::Rain => 1,
        LiveScene::Snow => 2,
        LiveScene::Night => 3,
        LiveScene::Clouds => 4,
        LiveScene::Clock => 5,
        LiveScene::Aquarium => 6,
    };
    f.choice(
        p,
        t!("Sahne", "Scene"),
        t!(
            "Dock'un camında hafif bir hava: yağmur damlaları, kar, yıldızlar ve ateşböcekleri, bulutlar. Simgelerin arkasında kalır.\nSaat: şafak, gündüz, akşam ve gece kendiliğinden değişir.\nAkvaryum: camın içinde yavaşça yüzen balıklar ve yükselip yüzeyde patlayan kabarcıklar.",
            "A light weather inside the dock glass: raindrops, snow, stars and fireflies, clouds. Stays behind the icons.\nClock: dawn, day, dusk and night change on their own.\nAquarium: fish swimming slowly inside the glass and bubbles that rise and pop at the surface."
        ),
        &[
            (t!("Yok", "None"), edit(|c| c.dock.live.scene = LiveScene::None)),
            (t!("Yağmur", "Rain"), edit(|c| c.dock.live.scene = LiveScene::Rain)),
            (t!("Kar", "Snow"), edit(|c| c.dock.live.scene = LiveScene::Snow)),
            (t!("Gece", "Night"), edit(|c| c.dock.live.scene = LiveScene::Night)),
            (t!("Bulut", "Clouds"), edit(|c| c.dock.live.scene = LiveScene::Clouds)),
            (t!("Saat", "Clock"), edit(|c| c.dock.live.scene = LiveScene::Clock)),
            (t!("Akvaryum", "Aquarium"), edit(|c| c.dock.live.scene = LiveScene::Aquarium)),
        ],
        sel,
        72.0,
    );
}

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let c = &app.cfg.dock.live;
    f.section(p, t!("Canlı dock", "Living dock"));
    pet_rows(app, p, f, c.pet.trim());
    super::dock_live_photo::paint(app, p, f);
    scene_row(p, f, c.scene);

    if c.is_off() {
        return;
    }
    let intensity = match c.intensity {
        LiveIntensity::Subtle => 0,
        LiveIntensity::Normal => 1,
    };
    f.choice(
        p,
        t!("Yoğunluk", "Intensity"),
        t!(
            "Hafif: daha az ve daha soluk parçacık, akvaryumda daha az balık. Simgelerin okunması için üst sınır hep vardır.",
            "Subtle: fewer, fainter particles and fewer fish in the aquarium. There is always a cap so icons stay readable."
        ),
        &[(t!("Hafif", "Subtle"), edit(|c| c.dock.live.intensity = LiveIntensity::Subtle)), (t!("Normal", "Normal"), edit(|c| c.dock.live.intensity = LiveIntensity::Normal))],
        intensity,
        90.0,
    );
    f.toggle(
        p,
        t!("Pilde duraklat", "Pause on battery"),
        t!(
            "Pilde çalışırken her şey durur. Pil tasarrufu, kilit ekranı, kapalı ekran ve tam ekran oyunlarda zaten hep durur; \"hareketi azalt\" açıkken tek durağan kare kalır.",
            "Everything stops while on battery. It always stops in battery saver, on the lock screen, with the display off and in full-screen games; with reduced motion on, a single still frame stays."
        ),
        c.pause_on_battery,
        |c| c.dock.live.pause_on_battery ^= true,
    );
}
