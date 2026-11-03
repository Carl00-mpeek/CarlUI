//! Bu modül pano geçmişini saklıyor: `%LOCALAPPDATA%\CarlUI\clip\history.json`
//! ve resimler için yanındaki `images\` klasörü. Aynı içerik tekrar kopyalanınca
//! yeni kayıt açılmaz, mevcut kayıt en üste taşınır. Sabitlenenler temizlemede
//! ve sınır aşımında silinmez. İlk çalıştırmada eski Pano uygulamasının geçmişi
//! (Belgeler\Pano) içeri aktarılır.

use crate::{capture::Content, image};
use carlui_core::{info, paths, warn};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Text,
    Image,
    Files,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Clip {
    pub id: String,
    pub kind: Kind,
    /// Metin; dosyalarda satır satır yollar; resimde boş.
    #[serde(default)]
    pub text: String,
    /// Resim dosyasının adı (images\ içinde).
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub w: u32,
    #[serde(default)]
    pub h: u32,
    pub hash: String,
    #[serde(default)]
    pub pinned: bool,
    /// Şifre/anahtar gibi görünüyor: listede maskelenir, süresi dolunca silinir
    /// (sabitlenirse kalır). Bkz. `secret`.
    #[serde(default)]
    pub secret: bool,
    /// Unix zamanı (ms).
    pub time: i64,
}

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// FNV-1a: içerik özeti (aynı kopyayı tanımak için).
fn hash(kind: Kind, bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325 ^ kind as u64;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn new_id() -> String {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    format!("{:x}{:04x}", now_ms(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) & 0xFFFF)
}

pub struct Store {
    pub items: Vec<Clip>,
    dir: PathBuf,
    pub max_items: usize,
    /// Yer tutucu (henüz yüklenmedi) iken diske yazılmaz; geçmiş boşla ezilmesin.
    loaded: bool,
}

impl Store {
    /// Yükleme arka planda sürerken kullanılan boş depo.
    pub fn placeholder(max_items: usize) -> Self {
        Self { items: Vec::new(), dir: paths::data_dir().join("clip"), max_items: max_items.max(10), loaded: false }
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn load(max_items: usize) -> Self {
        let dir = paths::data_dir().join("clip");
        let _ = std::fs::create_dir_all(dir.join("images"));
        let file = dir.join("history.json");
        let existed = file.exists();
        let items = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let mut s = Self { items, dir, max_items: max_items.max(10), loaded: true };
        if !existed {
            s.import_legacy();
        }
        s
    }

    pub fn image_path(&self, c: &Clip) -> PathBuf {
        self.dir.join("images").join(&c.image)
    }

    /// Listede gösterilen küçük resim (yoksa asıl resim kullanılır).
    pub fn thumb_path(&self, c: &Clip) -> PathBuf {
        self.dir.join("images").join(c.image.replace(".png", ".t.png"))
    }

    /// Kaydı en üste taşır (yeniden kullanıldı).
    pub fn touch(&mut self, id: &str) {
        if let Some(i) = self.items.iter().position(|c| c.id == id) {
            let mut c = self.items.remove(i);
            c.time = now_ms();
            self.items.insert(0, c);
            self.save();
        }
    }

    fn save(&self) {
        if !self.loaded {
            return;
        }
        let file = self.dir.join("history.json");
        let tmp = self.dir.join("history.json.tmp");
        if let Ok(j) = serde_json::to_vec(&self.items) {
            if std::fs::write(&tmp, j).is_ok() {
                let _ = std::fs::rename(&tmp, &file);
            }
        }
    }

    /// Yeni kopyayı ekler; aynısı varsa en üste taşır. Değişiklik olduysa true.
    pub fn add(&mut self, c: Content) -> bool {
        let (kind, h) = match &c {
            Content::Text(t) => (Kind::Text, hash(Kind::Text, t.as_bytes())),
            Content::Image { png, .. } => (Kind::Image, hash(Kind::Image, png)),
            Content::Files(f) => (Kind::Files, hash(Kind::Files, f.join("\n").as_bytes())),
        };
        if let Some(i) = self.items.iter().position(|x| x.hash == h) {
            if i == 0 {
                return false;
            }
            let mut it = self.items.remove(i);
            it.time = now_ms();
            self.items.insert(0, it);
            self.save();
            return true;
        }
        let id = new_id();
        let mut clip = Clip { id: id.clone(), kind, text: String::new(), image: String::new(), w: 0, h: 0, hash: h, pinned: false, secret: false, time: now_ms() };
        match c {
            Content::Text(t) => {
                clip.secret = crate::secret::looks_secret(&t);
                clip.text = t;
            }
            Content::Files(f) => clip.text = f.join("\n"),
            Content::Image { png, w, h } => {
                clip.image = format!("{id}.png");
                clip.w = w;
                clip.h = h;
                if std::fs::write(self.image_path(&clip), &png).is_err() {
                    warn!("pano: resim kaydedilemedi");
                    return false;
                }
                if let Some(t) = image::wic().and_then(|wic| image::thumb_png(&wic, &png, 120)) {
                    let _ = std::fs::write(self.thumb_path(&clip), t);
                }
            }
        }
        self.items.insert(0, clip);
        self.trim();
        self.save();
        true
    }

    /// Sınırı aşan en eski, sabitlenmemiş kayıtları siler.
    fn trim(&mut self) {
        while self.items.len() > self.max_items {
            let Some(i) = self.items.iter().rposition(|c| !c.pinned) else { break };
            let c = self.items.remove(i);
            self.delete_file(&c);
        }
    }

    fn delete_file(&self, c: &Clip) {
        if !c.image.is_empty() {
            let _ = std::fs::remove_file(self.image_path(c));
            let _ = std::fs::remove_file(self.thumb_path(c));
        }
    }

    pub fn remove(&mut self, id: &str) {
        if let Some((_, c)) = self.take(id) {
            self.forget(&c);
        }
    }

    /// Kaydı listeden çıkarır ama resim dosyasını bırakır ("Geri al" için).
    /// Geri alınmazsa `forget` ile dosyası da silinmeli.
    pub fn take(&mut self, id: &str) -> Option<(usize, Clip)> {
        let i = self.items.iter().position(|c| c.id == id)?;
        let c = self.items.remove(i);
        self.save();
        Some((i, c))
    }

    /// `take` ile çıkarılan kaydı eski yerine koyar.
    pub fn restore(&mut self, at: usize, c: Clip) {
        if self.items.iter().any(|x| x.hash == c.hash) {
            // Bu arada aynısı yeniden kopyalandı; ikinci kopya açma.
            return;
        }
        self.items.insert(at.min(self.items.len()), c);
        self.save();
    }

    /// `take` ile çıkarılan kaydın dosyasını kalıcı olarak siler.
    pub fn forget(&self, c: &Clip) {
        if !self.items.iter().any(|x| x.image == c.image && !c.image.is_empty()) {
            self.delete_file(c);
        }
    }

    /// Süresi dolan (sabitlenmemiş) gizli kayıtları siler. Bir şey silindiyse true.
    pub fn expire_secrets(&mut self, max_age_ms: i64) -> bool {
        let now = now_ms();
        let before = self.items.len();
        self.items.retain(|c| !(c.secret && !c.pinned && now - c.time >= max_age_ms));
        let gone = before != self.items.len();
        if gone {
            self.save();
        }
        gone
    }

    /// Sabitlenmemiş her şeyi siler.
    pub fn clear(&mut self) {
        let (keep, gone): (Vec<Clip>, Vec<Clip>) = self.items.drain(..).partition(|c| c.pinned);
        for c in &gone {
            self.delete_file(c);
        }
        self.items = keep;
        self.save();
    }

    pub fn toggle_pin(&mut self, id: &str) {
        if let Some(c) = self.items.iter_mut().find(|c| c.id == id) {
            c.pinned = !c.pinned;
            self.save();
        }
    }

    /// Metni düzenler (yerinde); boşsa değişmez.
    pub fn set_text(&mut self, id: &str, text: String) -> bool {
        if text.trim().is_empty() {
            return false;
        }
        let h = hash(Kind::Text, text.as_bytes());
        let Some(c) = self.items.iter_mut().find(|c| c.id == id && c.kind == Kind::Text) else { return false };
        c.secret = crate::secret::looks_secret(&text);
        c.text = text;
        c.hash = h;
        c.time = now_ms();
        self.save();
        true
    }

    pub fn get(&self, id: &str) -> Option<&Clip> {
        self.items.iter().find(|c| c.id == id)
    }

    /// Eski Pano uygulamasının geçmişini (Belgeler\Pano) içeri aktarır.
    fn import_legacy(&mut self) {
        let docs = std::env::var("USERPROFILE").map(|h| PathBuf::from(h).join("Documents")).unwrap_or_default();
        let Some(wic) = image::wic() else { return };
        let mut got: Vec<(i64, Content, bool)> = Vec::new();
        // Yeni biçim: Pano\history.json + Pano\images\*.png
        if let Some(list) = std::fs::read(docs.join("Pano").join("history.json")).ok().and_then(|b| serde_json::from_slice::<Vec<serde_json::Value>>(&b).ok()) {
            for v in list {
                let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
                let time = parse_iso(&s("Time")).unwrap_or(0);
                let pinned = v.get("Pinned").and_then(|x| x.as_bool()).unwrap_or(false);
                let c = match s("Type").as_str() {
                    "image" => std::fs::read(docs.join("Pano").join("images").join(s("ImageFile"))).ok().and_then(|b| image::to_png(&wic, &b)).map(|(png, w, h)| Content::Image { png, w, h }),
                    _ if !s("Text").is_empty() => Some(Content::Text(s("Text"))),
                    _ => None,
                };
                if let Some(c) = c {
                    got.push((time, c, pinned));
                }
            }
        }
        // Eski biçim: Pano.json [{Kind, Value}]. Zaman bilgisi yok: dosyanın son
        // yazılma zamanından geriye doğru birer saniye arayla sıralanır.
        let mtime = std::fs::metadata(docs.join("Pano.json"))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(1);
        if let Some(list) = std::fs::read(docs.join("Pano.json")).ok().and_then(|b| serde_json::from_slice::<Vec<serde_json::Value>>(&b).ok()) {
            for (i, v) in list.iter().enumerate() {
                let kind = v.get("Kind").and_then(|x| x.as_str()).unwrap_or_default().to_lowercase();
                let val = v.get("Value").and_then(|x| x.as_str()).unwrap_or_default();
                let c = match kind.as_str() {
                    "image" => image::base64_decode(val).and_then(|b| image::to_png(&wic, &b)).map(|(png, w, h)| Content::Image { png, w, h }),
                    "files" | "filedrop" => Some(Content::Files(val.lines().map(String::from).collect())),
                    _ if !val.trim().is_empty() => Some(Content::Text(val.to_string())),
                    _ => None,
                };
                if let Some(c) = c {
                    got.push((mtime - i as i64 * 1000, c, false));
                }
            }
        }
        if got.is_empty() {
            return;
        }
        // Eskiden yeniye ekle ki en yenisi üstte kalsın.
        got.sort_by_key(|g| g.0);
        let n = got.len();
        for (time, c, pinned) in got {
            // add() yeni, taşınmış ya da zaten üstteki kaydı her durumda en üste bırakır.
            self.add(c);
            if let Some(first) = self.items.first_mut() {
                first.pinned |= pinned;
                if time > 0 {
                    first.time = time;
                }
            }
        }
        self.save();
        info!("eski Pano geçmişinden {n} kayıt içeri aktarıldı ({} kayıt oldu)", self.items.len());
    }
}

/// "2026-11-01T09:16:29.8386364+03:00" → unix ms (yaklaşık; saat dilimi uygulanır).
fn parse_iso(s: &str) -> Option<i64> {
    let n = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (n(0, 4)?, n(5, 7)?, n(8, 10)?, n(11, 13)?, n(14, 16)?, n(17, 19)?);
    // Gün sayısı (Howard Hinnant'ın algoritması).
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let mut secs = days * 86400 + h * 3600 + mi * 60 + se;
    if let Some(p) = s.rfind(['+', '-']).filter(|&p| p >= 19) {
        let sign = if &s[p..p + 1] == "-" { -1 } else { 1 };
        let tz = n(p + 1, p + 3).unwrap_or(0) * 3600 + n(p + 4, p + 6).unwrap_or(0) * 60;
        secs -= sign * tz;
    }
    Some(secs * 1000)
}

/// "şimdi", "5 dk", "3 sa", "dün", "4 gün".
pub fn ago(time: i64) -> String {
    let s = ((now_ms() - time) / 1000).max(0);
    match s {
        0..=59 => t!("şimdi", "now").into(),
        60..=3599 => format!("{} {}", s / 60, t!("dk", "min")),
        3600..=86399 => format!("{} {}", s / 3600, t!("sa", "h")),
        86400..=172799 => t!("dün", "yesterday").into(),
        _ => format!("{} {}", s / 86400, t!("gün", "days")),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn iso() {
        assert_eq!(super::parse_iso("1970-01-02T00:00:00+00:00"), Some(86_400_000));
        assert_eq!(super::parse_iso("1970-01-01T03:00:00+03:00"), Some(0));
        assert_eq!(super::parse_iso("2026-11-01T09:01:56.2113293+03:00"), Some(1_793_512_916_000));
    }
}
