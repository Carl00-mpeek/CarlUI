//! Bu modül "geçen gün kopyaladığım o metin neydi?" sorununu Spotlight'tan
//! şu yöntemle çözüyor: "pano" yazınca pano geçmişindeki son metinler, "pano
//! adres" yazınca içinde "adres" geçenler listelenir; Enter metni yeniden
//! panoya koyar. Geçmişin kendisi adada (pano modülünde) durur; ada
//! başlarken `set_source` ile metin listesini veren bir fonksiyon bağlar.
//! Gizli (şifre gibi görünen) kayıtlar buraya hiç gelmez.

use crate::search::{norm, Act, Icon, Item};
use std::sync::OnceLock;

type Source = Box<dyn Fn() -> Vec<String> + Send + Sync>;
static SOURCE: OnceLock<Source> = OnceLock::new();
const MAX: usize = 8;

/// Pano geçmişindeki metinleri (yeniden eskiye) veren fonksiyonu bağlar.
pub fn set_source(f: impl Fn() -> Vec<String> + Send + Sync + 'static) {
    let _ = SOURCE.set(Box::new(f));
}

fn preview(s: &str) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > 90 { one.chars().take(90).collect::<String>() + "…" } else { one }
}

/// "pano", "pano kelime", "clip kelime".
pub(crate) fn items(q: &str) -> Option<Vec<Item>> {
    let rest = ["pano", "clip"].iter().find_map(|k| {
        let r = q.strip_prefix(k)?;
        (r.is_empty() || r.starts_with(' ')).then(|| r.trim())
    })?;
    let texts = SOURCE.get().map(|f| f()).unwrap_or_default();
    Some(
        texts
            .into_iter()
            .filter(|t| rest.is_empty() || norm(t).contains(rest))
            .take(MAX)
            .map(|t| {
                let mut it = Item::new(preview(&t), "Pano geçmişi · Enter ile yeniden kopyala", Icon::Glyph('\u{E77F}'), Act::Copy(t), "Pano");
                it.complete = String::new();
                it
            })
            .collect(),
    )
}
