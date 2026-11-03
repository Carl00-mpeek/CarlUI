//! Bu modül "emoji klavyesini (Win+.) açıp kaydırmadan emoji bul" sorununu
//! şu yöntemle çözüyor: ":" ile başlayan arama (":gül", ":kalp", ":ateş")
//! Türkçe ve İngilizce adlarıyla eşleşen emojileri listeler; Enter emojiyi
//! panoya kopyalar ve önceki pencereye dönülür (Ctrl+V ile yapıştırılır).

use crate::search::{norm, score, Act, Icon, Item};

/// (emoji, aranacak adlar — ilki başlık)
const EMOJI: &[(&str, &str)] = &[
    ("😂", "sevinçten ağlayan, gülmek, kahkaha, joy, lol"),
    ("🤣", "yerlere yatmak, gülmek, rofl"),
    ("😀", "gülen yüz, gülümseme, smile, grin"),
    ("😁", "sırıtan, gülmek, grin"),
    ("😊", "mutlu, utangaç gülümseme, blush, happy"),
    ("🙂", "hafif gülümseme, slight smile"),
    ("😉", "göz kırpan, wink"),
    ("😍", "aşık, kalp gözlü, love eyes, heart eyes"),
    ("🥰", "sevgi dolu, kalpli yüz, in love"),
    ("😘", "öpücük, kiss"),
    ("😎", "havalı, gözlüklü, cool, sunglasses"),
    ("🤔", "düşünen, hmm, thinking"),
    ("🙄", "göz devirme, eye roll"),
    ("😏", "sinsi gülümseme, smirk"),
    ("😐", "ifadesiz, neutral"),
    ("😴", "uyuyan, uyku, sleep"),
    ("😅", "terli gülme, sweat smile"),
    ("😇", "melek, angel, innocent"),
    ("🤩", "yıldız gözlü, hayran, star struck"),
    ("🥳", "parti, kutlama, party"),
    ("😢", "ağlayan, üzgün, cry, sad"),
    ("😭", "hüngür hüngür ağlayan, sob"),
    ("😡", "kızgın, öfkeli, angry"),
    ("🤬", "küfreden, sinirli, cursing"),
    ("😱", "korkmuş, çığlık, scream, shock"),
    ("😳", "şaşkın, kızarmış, flushed"),
    ("🥺", "yalvaran, pleading, puppy eyes"),
    ("😬", "dişlerini sıkan, grimace"),
    ("🤯", "beyni patlamış, mind blown"),
    ("🥶", "donmuş, soğuk, cold"),
    ("🥵", "sıcak, terlemiş, hot"),
    ("🤢", "midesi bulanan, nauseated"),
    ("🤮", "kusan, vomit"),
    ("🤡", "palyaço, clown"),
    ("💀", "kurukafa, ölmek, skull, dead"),
    ("👻", "hayalet, ghost"),
    ("👽", "uzaylı, alien"),
    ("🤖", "robot"),
    ("💩", "kaka, poop"),
    ("🙈", "görmedim maymun, see no evil"),
    ("👍", "beğen, başparmak, onay, tamam, thumbs up, like, ok"),
    ("👎", "beğenme, thumbs down, dislike"),
    ("👌", "tamam işareti, ok hand"),
    ("✌️", "zafer, barış, victory, peace"),
    ("🤞", "parmak çapraz, şans, fingers crossed"),
    ("👏", "alkış, clap"),
    ("🙌", "eller havada, yaşasın, raised hands"),
    ("🙏", "dua, lütfen, teşekkür, pray, please, thanks"),
    ("💪", "güç, kas, strong, muscle"),
    ("👋", "el sallama, merhaba, selam, wave, hi"),
    ("🤝", "tokalaşma, anlaşma, handshake"),
    ("👀", "gözler, bakmak, eyes"),
    ("🫡", "selam veren, saygı, salute"),
    ("❤️", "kırmızı kalp, kalp, aşk, sevgi, heart, love"),
    ("🧡", "turuncu kalp, orange heart"),
    ("💛", "sarı kalp, yellow heart"),
    ("💚", "yeşil kalp, green heart"),
    ("💙", "mavi kalp, blue heart"),
    ("💜", "mor kalp, purple heart"),
    ("🖤", "siyah kalp, black heart"),
    ("🤍", "beyaz kalp, white heart"),
    ("💔", "kırık kalp, broken heart"),
    ("💕", "iki kalp, kalpler, hearts"),
    ("🔥", "ateş, alev, yangın, fire, lit"),
    ("✨", "parıltı, ışıltı, sparkles"),
    ("⭐", "yıldız, star"),
    ("🌙", "ay, hilal, gece, moon"),
    ("☀️", "güneş, sun"),
    ("🌈", "gökkuşağı, rainbow"),
    ("⚡", "şimşek, elektrik, yıldırım, lightning"),
    ("❄️", "kar tanesi, snow"),
    ("🌧️", "yağmur, rain"),
    ("🌸", "sakura, kiraz çiçeği, cherry blossom"),
    ("🌹", "gül, rose"),
    ("🌻", "ayçiçeği, sunflower"),
    ("🍀", "yonca, şans, clover, luck"),
    ("🎉", "konfeti, kutlama, tebrikler, party, tada"),
    ("🎂", "doğum günü pastası, birthday cake"),
    ("🎁", "hediye, gift"),
    ("🏆", "kupa, şampiyon, trophy"),
    ("⚽", "futbol, top, soccer"),
    ("🏀", "basketbol, basketball"),
    ("🎮", "oyun, oyun kolu, game, controller"),
    ("🎵", "müzik, nota, music"),
    ("🎧", "kulaklık, headphones"),
    ("📷", "kamera, fotoğraf, camera"),
    ("💻", "bilgisayar, laptop"),
    ("📱", "telefon, phone"),
    ("💡", "fikir, ampul, idea"),
    ("📌", "raptiye, sabitle, pin"),
    ("📎", "ataç, paperclip"),
    ("✅", "tamamlandı, onay, check, done"),
    ("❌", "çarpı, hayır, iptal, cross, no"),
    ("⚠️", "uyarı, dikkat, warning"),
    ("❓", "soru, question"),
    ("❗", "ünlem, önemli, exclamation"),
    ("💯", "yüz, tam puan, hundred"),
    ("🚀", "roket, hızlı, rocket"),
    ("✈️", "uçak, plane"),
    ("🚗", "araba, car"),
    ("🏠", "ev, home, house"),
    ("☕", "kahve, çay, coffee"),
    ("🍕", "pizza"),
    ("🍔", "hamburger, burger"),
    ("🍟", "patates kızartması, fries"),
    ("🍺", "bira, beer"),
    ("🍷", "şarap, wine"),
    ("🍫", "çikolata, chocolate"),
    ("🍎", "elma, apple"),
    ("🐱", "kedi, cat"),
    ("🐶", "köpek, dog"),
    ("🦊", "tilki, fox"),
    ("🐼", "panda"),
    ("🦋", "kelebek, butterfly"),
    ("🐍", "yılan, snake"),
    ("🇹🇷", "türkiye bayrağı, türk bayrağı, turkey flag"),
    ("💰", "para, money"),
    ("💸", "para uçuyor, harcama, money flying"),
    ("⏰", "alarm, saat, clock"),
    ("📅", "takvim, calendar"),
    ("🔒", "kilit, lock"),
    ("🔑", "anahtar, key"),
    ("🗿", "moai, taş yüz, moai"),
    ("🤷", "omuz silkme, bilmem, shrug"),
    ("🤦", "facepalm, yüzünü kapatan"),
];

const MAX: usize = 8;

/// ":" sonrası metne uyan emojiler. Boşsa en sık kullanılanlardan bir seçki.
pub(crate) fn matches(rest: &str) -> Vec<Item> {
    let q = norm(rest.trim());
    let mut ranked: Vec<(i32, usize)> = EMOJI
        .iter()
        .enumerate()
        .filter_map(|(i, (_, names))| {
            if q.is_empty() {
                return (i < MAX).then_some((0, i));
            }
            let s = names.split(", ").map(|n| score(&norm(n), &q)).max().unwrap_or(0);
            (s >= 500).then_some((s, i))
        })
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    ranked
        .into_iter()
        .take(MAX)
        .map(|(_, i)| {
            let (e, names) = EMOJI[i];
            let name = names.split(", ").next().unwrap_or("");
            let mut it = Item::new(format!("{e}   {name}"), "Emoji · Enter ile kopyala", Icon::Glyph('\u{E76E}'), Act::Copy(e.to_string()), "Emoji");
            it.key = format!("emoji:{e}");
            it
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds() {
        assert_eq!(matches("gül")[0].act, Act::Copy("🌹".into()));
        assert_eq!(matches("ates")[0].act, Act::Copy("🔥".into()));
        assert!(matches("kalp").len() >= 5);
        assert!(matches("").len() == MAX);
        assert!(matches("zzzz").is_empty());
    }
}
