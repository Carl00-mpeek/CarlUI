//! Dışa aktarma (yedek). Sorun: kasa yalnızca bu bilgisayarda durur; disk
//! bozulursa ya da bilgisayar değişirse kayıtlar kaybolmasın, istenirse başka
//! bir parola yöneticisine de taşınabilsin. Çözüm üç biçim:
//!
//! - `.carlvault` (varsayılan, şifreli): bütün kayıtlar, ek dosyalar ve
//!   passkey'ler tek dosyada. Yedek parolası (ana parola ya da üretilmiş güçlü
//!   parola) Argon2id ile yeni bir tuzla anahtara çevrilir, içerik
//!   XChaCha20-Poly1305 ile mühürlenir. Başlık (biçim, KDF ayarları) ek veri
//!   olarak mühre bağlıdır: kurcalanırsa dosya açılmaz.
//!
//!   ```text
//!   CARLVAULT/1\n
//!   {"kdf":{…},"created":…,"count":…}\n
//!   nonce (24 bayt) | şifreli metin + etiket
//!   ```
//!
//!   Geri yükleme içe aktarma akışından yapılır (`open`): önizleme açılır,
//!   kasada zaten olan kayıtlar tekrar eklenmez.
//!
//! - CSV ve JSON (Bitwarden düzeni, şifresiz): başka uygulamalara taşımak
//!   için. Ek dosyalar ve passkey'ler bu biçimlere girmez. Dosyayı ele geçiren
//!   herkes içeriği okuyabilir; arayüz bunu açıkça söyler.

use crate::{
    card::Part,
    crypto::{self, Error, KdfParams, Result, Sealed},
    entry::{Body, Category, FieldKind, Meta},
    import::{Batch, Imported},
    vault::Vault,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use zeroize::{Zeroize, Zeroizing};

/// Şifreli yedeğin dosya uzantısı.
pub const EXT: &str = "carlvault";
const MAGIC: &[u8] = b"CARLVAULT/1\n";
const NONCE_LEN: usize = 24;
/// Okunacak en büyük yedek (ek dosyalar dahil).
pub const MAX_BACKUP: u64 = 1024 * 1024 * 1024;

/// Dışa aktarma biçimi.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Kind {
    /// `.carlvault`: şifreli, her şey dahil.
    #[default]
    Encrypted,
    /// Bitwarden CSV düzeni, şifresiz.
    Csv,
    /// Bitwarden JSON düzeni, şifresiz.
    Json,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Encrypted, Kind::Csv, Kind::Json];

    pub fn ext(self) -> &'static str {
        match self {
            Kind::Encrypted => EXT,
            Kind::Csv => "csv",
            Kind::Json => "json",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Kind::Encrypted => crate::t!("Şifreli (.carlvault)", "Encrypted (.carlvault)"),
            Kind::Csv => "CSV",
            Kind::Json => "JSON",
        }
    }

    pub fn encrypted(self) -> bool {
        self == Kind::Encrypted
    }
}

/// Dışa aktarılacak bir kayıt (çözülmüş).
pub struct Entry {
    pub meta: Meta,
    pub body: Body,
    /// Ek dosyanın içeriği (varsa).
    pub file: Option<Zeroizing<Vec<u8>>>,
}

/// Kasadan toplanan kayıtlar.
pub struct Collected {
    pub entries: Vec<Entry>,
    /// Kayıtta görünen ama diskte okunamayan ek dosyalar (yedeğe girmedi).
    pub missing_files: usize,
}

/// Açık kasadaki bütün kayıtları çözer. `files`: ek dosyalar da okunsun mu.
/// Okunamayan ek dosya yedeği engellemez; kayıttan düşülür ve sayılır.
pub fn collect(v: &Vault, files: bool) -> Result<Collected> {
    let mut entries = Vec::with_capacity(v.items().len());
    let mut missing_files = 0;
    for it in v.items() {
        let mut body = v.decrypt(&it.id)?;
        let mut file = None;
        if files && body.file.is_some() {
            match v.read_file(&it.id) {
                Ok(d) => file = Some(d),
                Err(_) => {
                    missing_files += 1;
                    body.file = None;
                }
            }
        }
        entries.push(Entry { meta: it.meta.clone(), body, file });
    }
    Ok(Collected { entries, missing_files })
}

// ---------------------------------------------------------------- .carlvault

#[derive(Serialize, Deserialize)]
struct Header {
    kdf: KdfParams,
    created: u64,
    count: usize,
}

#[derive(Serialize)]
struct ItemOut<'a> {
    meta: &'a Meta,
    body: &'a Body,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<&'a str>,
}

#[derive(Deserialize)]
struct ItemIn {
    meta: Meta,
    body: Body,
    #[serde(default)]
    file: Option<String>,
}

#[derive(Serialize)]
struct PayloadOut<'a> {
    items: Vec<ItemOut<'a>>,
}

#[derive(Deserialize)]
struct PayloadIn {
    items: Vec<ItemIn>,
}

fn fmt_err(e: impl std::fmt::Display) -> Error {
    Error::Format(e.to_string())
}

/// Dosya şifreli bir Carl Vault yedeği mi?
pub fn is_backup(data: &[u8]) -> bool {
    data.starts_with(MAGIC)
}

/// Kayıtları yedek parolasıyla mühürler; dosyanın içeriğini döner.
pub fn encrypt(entries: &[Entry], password: &str) -> Result<Vec<u8>> {
    encrypt_with(entries, password, KdfParams::new()?)
}

fn encrypt_with(entries: &[Entry], password: &str, kdf: KdfParams) -> Result<Vec<u8>> {
    let key = kdf.derive(password.as_bytes())?;
    let encoded: Vec<Option<Zeroizing<String>>> = entries.iter().map(|e| e.file.as_ref().map(|d| base64(d))).collect();
    let payload = PayloadOut { items: entries.iter().zip(&encoded).map(|(e, f)| ItemOut { meta: &e.meta, body: &e.body, file: f.as_ref().map(|s| s.as_str()) }).collect() };
    let plain = Zeroizing::new(serde_json::to_vec(&payload).map_err(fmt_err)?);
    let header = serde_json::to_vec(&Header { kdf, created: crate::entry::now(), count: entries.len() }).map_err(fmt_err)?;
    let mut aad = MAGIC.to_vec();
    aad.extend_from_slice(&header);
    let sealed = crypto::seal(&key, &aad, &plain)?;
    let mut out = Vec::with_capacity(aad.len() + 1 + sealed.n.len() + sealed.c.len());
    out.extend_from_slice(&aad);
    out.push(b'\n');
    out.extend_from_slice(&sealed.n);
    out.extend_from_slice(&sealed.c);
    Ok(out)
}

/// Açılmış yedek: içe aktarma önizlemesine gider. `files[i]`, `batch.items[i]`
/// kaydının ek dosyası.
pub struct Restored {
    pub batch: Batch,
    pub files: Vec<Option<Zeroizing<Vec<u8>>>>,
}

/// Yedeği açar. Parola yanlışsa ya da dosya değiştirilmişse `WrongKey`
/// (ikisi ayırt edilemez), biçim bozuksa `Format`.
pub fn open(data: &[u8], password: &str) -> Result<Restored> {
    let rest = data.strip_prefix(MAGIC).ok_or_else(|| fmt_err("carlvault"))?;
    let end = rest.iter().position(|&b| b == b'\n').ok_or_else(|| fmt_err("header"))?;
    let header: Header = serde_json::from_slice(&rest[..end]).map_err(|_| fmt_err("header"))?;
    let aad = &data[..MAGIC.len() + end];
    let body = &rest[end + 1..];
    if body.len() < NONCE_LEN + 16 {
        return Err(fmt_err("payload"));
    }
    let key = header.kdf.derive(password.as_bytes())?;
    let sealed = Sealed { n: body[..NONCE_LEN].to_vec(), c: body[NONCE_LEN..].to_vec() };
    let plain = crypto::open(&key, aad, &sealed).map_err(|_| Error::WrongKey)?;
    let payload: PayloadIn = serde_json::from_slice(&plain).map_err(|_| Error::Corrupt)?;
    let mut items = Vec::with_capacity(payload.items.len());
    let mut files = Vec::with_capacity(payload.items.len());
    let mut skipped = 0;
    for mut it in payload.items {
        let file = match it.file.take() {
            Some(mut s) => {
                let d = unbase64(&s);
                s.zeroize();
                if d.is_none() {
                    skipped += 1;
                }
                d
            }
            None => None,
        };
        let mut body = it.body.clone();
        if file.is_none() {
            body.file = None;
        }
        items.push(Imported { meta: it.meta.clone(), body });
        files.push(file);
    }
    let batch = Batch { source: crate::t!("Carl Vault yedeği", "Carl Vault backup").to_string(), items, skipped };
    Ok(Restored { batch, files })
}

// ---------------------------------------------------------------- CSV / JSON

/// Bir kaydın Bitwarden alanlarına ayrılmış hali.
#[derive(Default)]
struct Split {
    user: String,
    password: String,
    url: String,
    totp: String,
    notes: String,
    /// Diğer dolu alanlar: (ad, değer, gizli mi).
    extra: Vec<(String, String, bool)>,
}

impl Drop for Split {
    fn drop(&mut self) {
        self.user.zeroize();
        self.password.zeroize();
        self.url.zeroize();
        self.totp.zeroize();
        self.notes.zeroize();
        for (l, v, _) in &mut self.extra {
            l.zeroize();
            v.zeroize();
        }
    }
}

fn is_login(c: Category) -> bool {
    matches!(c, Category::Password | Category::Email | Category::TwoFactor)
}

fn split(e: &Entry) -> Split {
    let login = is_login(e.meta.category);
    let mut s = Split::default();
    let mut notes: Vec<&str> = Vec::new();
    for f in &e.body.fields {
        let v = f.value.trim();
        if v.is_empty() {
            continue;
        }
        let slot = match f.kind {
            FieldKind::Multiline => {
                notes.push(v);
                continue;
            }
            FieldKind::Text if login => &mut s.user,
            FieldKind::Secret if login => &mut s.password,
            FieldKind::Url if login => &mut s.url,
            FieldKind::Totp => &mut s.totp,
            _ => {
                s.extra.push((f.label.trim().to_string(), v.to_string(), f.kind.masked()));
                continue;
            }
        };
        if slot.is_empty() {
            slot.push_str(v);
        } else {
            s.extra.push((f.label.trim().to_string(), v.to_string(), f.kind.masked()));
        }
    }
    s.notes = notes.join("\n\n");
    s
}

fn csv_cell(out: &mut String, v: &str) {
    if v.contains([',', '"', '\n', '\r']) {
        out.push('"');
        out.push_str(&v.replace('"', "\"\""));
        out.push('"');
    } else {
        out.push_str(v);
    }
}

/// Bitwarden CSV düzeni (girişler ve notlar; diğer alanlar `fields` sütununa).
pub fn to_csv(entries: &[Entry]) -> Zeroizing<String> {
    let mut out = Zeroizing::new(String::from("folder,favorite,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp\r\n"));
    for e in entries {
        let s = split(e);
        let login = is_login(e.meta.category);
        let fields = Zeroizing::new(s.extra.iter().map(|(l, v, _)| format!("{l}: {v}")).collect::<Vec<_>>().join("\n"));
        let kind = if login { "login" } else { "note" };
        let row: [&str; 11] = [e.meta.category.title(), "", kind, &e.meta.name, &s.notes, &fields, "0", &s.url, &s.user, &s.password, &s.totp];
        for (i, c) in row.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            csv_cell(&mut out, c);
        }
        out.push_str("\r\n");
    }
    out
}

/// Bitwarden JSON düzeni (şifresiz): giriş, not, kart.
pub fn to_json(entries: &[Entry]) -> Result<Zeroizing<String>> {
    let mut folders: Vec<Category> = entries.iter().map(|e| e.meta.category).collect();
    folders.dedup();
    let mut seen = Vec::new();
    folders.retain(|c| {
        if seen.contains(c) {
            false
        } else {
            seen.push(*c);
            true
        }
    });
    let folder_id = |c: Category| format!("carlui-{}", format!("{c:?}").to_lowercase());
    let mut items = Vec::with_capacity(entries.len());
    for e in entries {
        let s = split(e);
        let fields: Vec<Value> = s.extra.iter().map(|(l, v, hidden)| json!({ "name": l, "value": v, "type": if *hidden { 1 } else { 0 } })).collect();
        let notes = if s.notes.is_empty() { Value::Null } else { Value::from(s.notes.as_str()) };
        let mut it = json!({
            "folderId": folder_id(e.meta.category),
            "favorite": false,
            "name": e.meta.name,
            "notes": notes,
            "fields": fields,
            "reprompt": 0,
        });
        let o = it.as_object_mut().ok_or_else(|| fmt_err("json"))?;
        let part = |p: Part| p.value(&e.body).map(|v| v.to_string()).unwrap_or_default();
        if is_login(e.meta.category) {
            o.insert("type".into(), 1.into());
            let uris = if s.url.is_empty() { json!([]) } else { json!([{ "match": null, "uri": s.url }]) };
            let totp = if s.totp.is_empty() { Value::Null } else { Value::from(s.totp.as_str()) };
            o.insert("login".into(), json!({ "username": s.user, "password": s.password, "totp": totp, "uris": uris }));
        } else if e.meta.category == Category::Card && !part(Part::Number).is_empty() {
            o.insert("type".into(), 3.into());
            let month = part(Part::Month).trim_start_matches('0').to_string();
            o.insert("card".into(), json!({ "cardholderName": part(Part::Holder), "number": part(Part::Number), "expMonth": month, "expYear": part(Part::Year4), "code": part(Part::Cvv) }));
            // Kartın kendisi yapılandırılmış alanlarda; geri kalanlar özel alan kalır.
            let left: Vec<Value> = e
                .body
                .fields
                .iter()
                .filter(|f| f.fmt.is_free() && f.kind != FieldKind::Multiline && !f.value.trim().is_empty())
                .map(|f| json!({ "name": f.label.trim(), "value": f.value.trim(), "type": if f.kind.masked() { 1 } else { 0 } }))
                .collect();
            o.insert("fields".into(), Value::Array(left));
        } else {
            o.insert("type".into(), 2.into());
            o.insert("secureNote".into(), json!({ "type": 0 }));
        }
        items.push(it);
    }
    let doc = json!({
        "encrypted": false,
        "folders": folders.iter().map(|c| json!({ "id": folder_id(*c), "name": c.title() })).collect::<Vec<_>>(),
        "items": items,
    });
    serde_json::to_string_pretty(&doc).map(Zeroizing::new).map_err(fmt_err)
}

// ---------------------------------------------------------------- base64

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(d: &[u8]) -> Zeroizing<String> {
    let mut s = Zeroizing::new(String::with_capacity(d.len().div_ceil(3) * 4));
    for c in d.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                s.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

fn unbase64(s: &str) -> Option<Zeroizing<Vec<u8>>> {
    let s = s.trim_end_matches('=').as_bytes();
    let mut out = Zeroizing::new(Vec::with_capacity(s.len() * 3 / 4));
    let mut acc = 0u32;
    let mut bits = 0;
    for &c in s {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        acc = (acc << 6 | v as u32) & 0xFF_FFFF;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    acc.zeroize();
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{Attachment, Field};

    fn field(label: &str, value: &str, kind: FieldKind) -> Field {
        let mut f = Field::new(label, kind);
        f.value = value.into();
        f
    }

    fn sample() -> Vec<Entry> {
        let login = Entry {
            meta: Meta { name: "GitHub".into(), category: Category::Password, updated: 1 },
            body: Body {
                fields: vec![
                    field("Kullanıcı adı", "carl", FieldKind::Text),
                    field("Parola", "p,a\"ss\nğüş", FieldKind::Secret),
                    field("Adres", "https://github.com/login", FieldKind::Url),
                    field("2FA anahtarı", "otpauth://totp/GitHub:carl?secret=JBSWY3DPEHPK3PXP&issuer=GitHub", FieldKind::Totp),
                    field("Not", "iki\nsatır", FieldKind::Multiline),
                ],
                file: Some(Attachment { name: "a.bin".into(), size: 5 }),
                passkeys: Vec::new(),
            },
            file: Some(Zeroizing::new(vec![0, 1, 2, 0xFE, 0xFF])),
        };
        let note = Entry {
            meta: Meta { name: "Gizli not".into(), category: Category::Note, updated: 2 },
            body: Body { fields: vec![field("Not", "merhaba", FieldKind::Multiline)], file: None, passkeys: Vec::new() },
            file: None,
        };
        let lic = Entry {
            meta: Meta { name: "Lisans".into(), category: Category::License, updated: 3 },
            body: Body { fields: vec![field("Anahtar", "AAAA-BBBB", FieldKind::Secret)], file: None, passkeys: Vec::new() },
            file: None,
        };
        vec![login, note, lic]
    }

    #[test]
    fn base64_roundtrip() {
        for n in 0..20usize {
            let d: Vec<u8> = (0..n as u8).map(|x| x.wrapping_mul(37)).collect();
            assert_eq!(&unbase64(&base64(&d)).unwrap()[..], &d[..]);
        }
        assert_eq!(base64(b"Man").as_str(), "TWFu");
        assert_eq!(base64(b"Ma").as_str(), "TWE=");
        assert!(unbase64("A*").is_none());
    }

    #[test]
    fn carlvault_roundtrip() {
        let data = encrypt_with(&sample(), "yedek-parola", KdfParams::fast()).unwrap();
        assert!(is_backup(&data));
        let s = String::from_utf8_lossy(&data);
        assert!(!s.contains("GitHub") && !s.contains("carl\"") && !s.contains("merhaba"), "düz metin yazılmamalı");
        let r = open(&data, "yedek-parola").unwrap();
        assert_eq!(r.batch.items.len(), 3);
        assert_eq!(r.batch.skipped, 0);
        assert_eq!(r.batch.items[0].meta.name, "GitHub");
        assert_eq!(r.batch.items[0].body.fields[1].value, "p,a\"ss\nğüş");
        assert_eq!(r.batch.items[0].body.file.as_ref().unwrap().name, "a.bin");
        assert_eq!(&r.files[0].as_ref().unwrap()[..], &[0, 1, 2, 0xFE, 0xFF]);
        assert!(r.files[1].is_none());
    }

    #[test]
    fn carlvault_rejects_wrong_password_and_tamper() {
        let data = encrypt_with(&sample(), "yedek-parola", KdfParams::fast()).unwrap();
        assert!(matches!(open(&data, "yanlis"), Err(Error::WrongKey)));
        // Başlık (ör. sayı) değişirse mühür tutmaz.
        let s = String::from_utf8_lossy(&data).into_owned();
        let at = s.find("\"count\":3").unwrap();
        let mut bad = data.clone();
        bad[at + 8] = b'4';
        assert!(matches!(open(&bad, "yedek-parola"), Err(Error::WrongKey)));
        let mut bad = data.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert!(matches!(open(&bad, "yedek-parola"), Err(Error::WrongKey)));
        assert!(matches!(open(&data[..MAGIC.len() + 5], "yedek-parola"), Err(Error::Format(_))));
        assert!(matches!(open(b"{}", "x"), Err(Error::Format(_))));
    }

    #[test]
    fn csv_reimports() {
        let csv = to_csv(&sample());
        let b = crate::import::parse(csv.as_bytes()).unwrap();
        assert_eq!(b.source, "Bitwarden (CSV)");
        assert_eq!(b.items.len(), 3);
        let gh = &b.items[0];
        assert_eq!(gh.meta.name, "GitHub");
        let val = |k: FieldKind| gh.body.fields.iter().find(|f| f.kind == k).map(|f| f.value.clone()).unwrap_or_default();
        assert_eq!(val(FieldKind::Secret), "p,a\"ss\nğüş");
        assert_eq!(val(FieldKind::Text), "carl");
        assert!(val(FieldKind::Totp).starts_with("otpauth://"));
        assert_eq!(b.items[1].meta.category, Category::Note);
        assert!(b.items[2].body.fields.iter().any(|f| f.value.contains("AAAA-BBBB")), "diğer alanlar nota");
    }

    #[test]
    fn json_reimports() {
        let j = to_json(&sample()).unwrap();
        let b = crate::import::parse(j.as_bytes()).unwrap();
        assert_eq!(b.source, "Bitwarden (JSON)");
        assert_eq!(b.items.len(), 3);
        assert_eq!(b.items[0].meta.name, "GitHub");
        assert!(b.items[0].body.fields.iter().any(|f| f.value == "p,a\"ss\nğüş"));
        assert!(b.items[2].body.fields.iter().any(|f| f.value.contains("AAAA-BBBB")));
    }

    #[test]
    fn collect_reads_vault() {
        let d = std::env::temp_dir().join(format!("carlui-export-test-{}", crate::hex::encode(&crypto::random::<8>().unwrap())));
        let mut v = Vault::create(&d, "ana-parola", KdfParams::fast()).unwrap();
        let mut entries = sample();
        let e = entries.remove(0);
        let id = v.add(e.meta.clone(), &e.body).unwrap();
        v.attach(&id, "a.bin", &[9, 8, 7]).unwrap();
        let n = entries.remove(0);
        let id2 = v.add(n.meta.clone(), &n.body).unwrap();
        let mut b2 = n.body.clone();
        b2.file = Some(Attachment { name: "yok.bin".into(), size: 1 });
        v.update(&id2, n.meta.clone(), &b2).unwrap();
        let c = collect(&v, true).unwrap();
        assert_eq!(c.entries.len(), 2);
        assert_eq!(c.missing_files, 1);
        let gh = c.entries.iter().find(|e| e.meta.name == "GitHub").unwrap();
        assert_eq!(&gh.file.as_ref().unwrap()[..], &[9, 8, 7]);
        let _ = std::fs::remove_dir_all(&d);
    }
}
