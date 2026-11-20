//! Bu modül "aklıma gelen bir şeyi hemen bir yere yazayım" sorununu çözüyor:
//! aramaya "not süt al" yazıp Enter'a basınca metin tarihiyle birlikte
//! `%APPDATA%\CarlUI\notes.md` dosyasına eklenir. "notlar" yazınca son
//! notlar, "notlar süt" yazınca içinde "süt" geçenler listelenir; Enter notu
//! panoya kopyalar. Dosya düz Markdown'dır, istenen düzenleyiciyle açılabilir.
//!
//! "not" kelimesi "Not Defteri" aramasını bozmasın diye not alma sonucu
//! uygulamaların altında gösterilir (bkz. `search::build`).

use crate::search::{norm, score, Act, Icon, Item};
use std::{io::Write, path::Path};

const LIST_MAX: usize = 8;

/// "not …" / "note …" ile başlıyorsa notun metni.
pub fn note_text(raw: &str) -> Option<&str> {
    let t = raw.trim_start();
    let lower = t.to_lowercase();
    let rest = ["not ", "note "].iter().find_map(|p| lower.starts_with(p).then(|| &t[p.len()..]))?;
    let rest = rest.trim();
    (!rest.is_empty()).then_some(rest)
}

/// Not alma sonucu.
pub fn take_item(text: &str) -> Item {
    let mut it = Item::new(t!(format!("Not al: {text}"), format!("Take a note: {text}")), t!("Enter ile notlara ekle · \"notlar\" ile bul", "Enter to add to notes · find with \"notes\""), Icon::Glyph('\u{E70B}'), Act::Note(text.to_string()), "Not");
    it.key = "note:take".into();
    it
}

/// Dosyaya eklenecek satır: `- [21.11.2026 14:05] metin` (satır sonları boşluğa döner).
pub fn line(stamp: &str, text: &str) -> String {
    let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("- [{stamp}] {one}\n")
}

fn now() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:02}.{:02}.{} {:02}:{:02}", t.wDay, t.wMonth, t.wYear, t.wHour, t.wMinute)
}

pub fn save_to(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(line(&now(), text).as_bytes())
}

pub fn save(text: &str) -> bool {
    match save_to(&carlui_core::paths::notes_file(), text) {
        Ok(()) => true,
        Err(e) => {
            carlui_core::warn!("not kaydedilemedi: {e}");
            false
        }
    }
}

/// Dosyadaki notlar, en yenisi başta: (tarih, metin).
pub fn parse(md: &str) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = md
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            let l = l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")).unwrap_or(l);
            if l.is_empty() || l.starts_with('#') {
                return None;
            }
            if let Some(r) = l.strip_prefix('[') {
                if let Some((stamp, text)) = r.split_once("] ") {
                    return Some((stamp.to_string(), text.trim().to_string()));
                }
            }
            Some((String::new(), l.to_string()))
        })
        .collect();
    v.reverse();
    v
}

/// "notlar" / "notlar süt" / "notes" listesi; başka bir şeyse None.
pub fn listing(q: &str) -> Option<Vec<Item>> {
    let rest = ["notlar", "notes"].iter().find_map(|p| {
        let r = q.strip_prefix(p)?;
        (r.is_empty() || r.starts_with(' ')).then(|| r.trim())
    })?;
    let path = carlui_core::paths::notes_file();
    let all = parse(&std::fs::read_to_string(&path).unwrap_or_default());
    Some(filter_items(&all, rest, &path.display().to_string()))
}

fn filter_items(all: &[(String, String)], q: &str, path: &str) -> Vec<Item> {
    let mut found: Vec<(i32, usize)> = all
        .iter()
        .enumerate()
        .filter_map(|(i, (_, text))| {
            if q.is_empty() {
                return Some((0, i));
            }
            let n = norm(text);
            let s = score(&n, q).max(if n.contains(q) { 500 } else { 0 });
            (s > 0).then_some((s, i))
        })
        .collect();
    // Puan eşitse yeni not önde (liste zaten yeniden eskiye).
    found.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut v: Vec<Item> = found
        .into_iter()
        .take(LIST_MAX)
        .map(|(_, i)| {
            let (stamp, text) = &all[i];
            let sub = if stamp.is_empty() { t!("Not · Enter ile kopyala", "Note · Enter to copy").to_string() } else { t!(format!("{stamp} · Enter ile kopyala"), format!("{stamp} · Enter to copy")) };
            let mut it = Item::new(text.clone(), sub, Icon::Glyph('\u{E70B}'), Act::Copy(text.clone()), "Not");
            it.key = "note:copy".into();
            it
        })
        .collect();
    if v.is_empty() {
        let msg = if all.is_empty() { t!("Henüz not yok · \"not <metin>\" yazın", "No notes yet · type \"note <text>\"") } else { t!("Eşleşen not yok", "No matching notes") };
        v.push(Item::new(msg, "", Icon::Glyph('\u{E70B}'), Act::None, "Not"));
    }
    if !all.is_empty() {
        let mut it = Item::new(t!("Not dosyasını aç", "Open the notes file"), path, Icon::Glyph('\u{E8E5}'), Act::Open(path.to_string()), "Not");
        it.path = path.to_string();
        v.push(it);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_metni_ayrilir() {
        assert_eq!(note_text("not süt al"), Some("süt al"));
        assert_eq!(note_text("  Note buy milk "), Some("buy milk"));
        assert_eq!(note_text("not"), None);
        assert_eq!(note_text("not   "), None);
        assert_eq!(note_text("notepad"), None);
        assert_eq!(note_text("notlar"), None);
    }

    #[test]
    fn satir_tek_satira_iner() {
        assert_eq!(line("21.11.2026 14:05", "a\nb  c"), "- [21.11.2026 14:05] a b c\n");
    }

    #[test]
    fn dosya_yazilir_ve_okunur() {
        let dir = std::env::temp_dir().join(format!("carlui-notes-{}", std::process::id()));
        let p = dir.join("notes.md");
        save_to(&p, "birinci").unwrap();
        save_to(&p, "ikinci not").unwrap();
        let v = parse(&std::fs::read_to_string(&p).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].1, "ikinci not");
        assert!(!v[0].0.is_empty());
    }

    #[test]
    fn notlarda_arama() {
        let all = parse("# Notlar\n- [1.1.2026 10:00] süt al\n- [2.1.2026 10:00] faturayı öde\nelle yazılmış satır\n");
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].1, "elle yazılmış satır");
        let v = filter_items(&all, "sut", "x");
        assert_eq!(v[0].act, Act::Copy("süt al".into()));
        assert_eq!(v.last().unwrap().act, Act::Open("x".into()));
        assert_eq!(filter_items(&all, "", "x").len(), 4);
        assert_eq!(filter_items(&all, "zzz", "x")[0].act, Act::None);
    }
}
