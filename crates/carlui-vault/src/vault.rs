//! Kasa dosyası ve açık oturum. Anahtar düzeni:
//!
//! ```text
//! ana parola ──Argon2id──▶ parola anahtarı ─┐
//! kurtarma anahtarı (isteğe bağlı) ─────────┼─sarar─▶ ana anahtar (rastgele)
//!                                           │
//! ana anahtar ─sarar─▶ kayıt anahtarı (her kayda ayrı, rastgele)
//! ana anahtar ─mühürler─▶ kayıt adı/kategorisi (liste için)
//! kayıt anahtarı ─mühürler─▶ kayıt alanları ve ek dosya
//! ```
//!
//! Dosya `%APPDATA%\CarlUI\vault\vault.json`; ek dosyalar `files\<id>.bin`.
//! Diskte düz metin olarak yalnızca biçim sürümü, Argon2 ayarları, kayıt
//! kimlikleri ve sayısı durur. Yazma atomiktir (geçici dosya + yer değiştirme),
//! bir önceki hali `vault.json.bak` olarak kalır.
//!
//! Kasa açıkken bellekte ana anahtar ve kayıt adları vardır; bir kaydın
//! alanları yalnızca `decrypt` çağrılınca çözülür ve dönen `Body` düşünce
//! silinir. `lock` ana anahtarı ve adları bellekten siler.

use crate::{
    crypto::{self, Error, KdfParams, Key, Result, Sealed},
    entry::{Attachment, Body, Category, Meta},
    hex, recovery,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

const FORMAT: u32 = 1;
const AAD_MK_PW: &[u8] = b"carlui-vault/1/mk/password";
const AAD_MK_REC: &[u8] = b"carlui-vault/1/mk/recovery";
/// Ek dosya üst sınırı.
pub const MAX_FILE: u64 = 25 * 1024 * 1024;

#[derive(Serialize, Deserialize, Clone)]
struct EntryRec {
    id: String,
    meta: Sealed,
    dek: Sealed,
    body: Sealed,
}

#[derive(Serialize, Deserialize, Clone)]
struct VaultFile {
    format: u32,
    kdf: KdfParams,
    mk_password: Sealed,
    #[serde(default)]
    mk_recovery: Option<Sealed>,
    entries: Vec<EntryRec>,
}

/// Listede görünen kayıt.
pub struct Item {
    pub id: String,
    pub meta: Meta,
}

struct Open {
    mk: Key,
    items: Vec<Item>,
}

pub struct Vault {
    dir: PathBuf,
    file: VaultFile,
    open: Option<Open>,
}

fn aad(kind: &str, id: &str) -> Vec<u8> {
    format!("carlui-vault/1/{kind}/{id}").into_bytes()
}

fn to_json<T: Serialize>(v: &T) -> Result<Zeroizing<Vec<u8>>> {
    serde_json::to_vec(v).map(Zeroizing::new).map_err(|e| Error::Format(e.to_string()))
}

fn from_json<T: for<'a> Deserialize<'a>>(b: &[u8]) -> Result<T> {
    serde_json::from_slice(b).map_err(|_| Error::Corrupt)
}

fn io(e: std::io::Error) -> Error {
    Error::Io(e.to_string())
}

/// Önce geçici dosyaya yaz, sonra yerine koy: yarıda kalan yazma kasayı bozmasın.
fn write_atomic(path: &Path, data: &[u8], backup: bool) -> Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data).map_err(io)?;
    if backup && path.exists() {
        let _ = std::fs::copy(path, path.with_extension("json.bak"));
    }
    std::fs::rename(&tmp, path).map_err(io)
}

/// `%APPDATA%\CarlUI\vault`; `CARLUI_VAULT_DIR` (deneme için) önüne geçer.
pub fn default_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("CARLUI_VAULT_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("CarlUI").join("vault")
}

impl Vault {
    fn path(&self) -> PathBuf {
        self.dir.join("vault.json")
    }

    fn blob(&self, id: &str) -> PathBuf {
        self.dir.join("files").join(format!("{id}.bin"))
    }

    pub fn exists(dir: &Path) -> bool {
        dir.join("vault.json").exists()
    }

    /// Kilitli kasayı okur.
    pub fn load(dir: &Path) -> Result<Vault> {
        let raw = std::fs::read(dir.join("vault.json")).map_err(io)?;
        let file: VaultFile = serde_json::from_slice(&raw).map_err(|e| Error::Format(e.to_string()))?;
        if file.format != FORMAT {
            return Err(Error::Format(format!("sürüm {}", file.format)));
        }
        Ok(Vault { dir: dir.to_path_buf(), file, open: None })
    }

    /// Yeni, boş ve açık bir kasa oluşturur.
    pub fn create(dir: &Path, password: &str, kdf: KdfParams) -> Result<Vault> {
        std::fs::create_dir_all(dir).map_err(io)?;
        let mk = Key::random()?;
        let pk = kdf.derive(password.as_bytes())?;
        let file = VaultFile { format: FORMAT, mk_password: crypto::wrap_key(&pk, AAD_MK_PW, &mk)?, kdf, mk_recovery: None, entries: Vec::new() };
        let v = Vault { dir: dir.to_path_buf(), file, open: Some(Open { mk, items: Vec::new() }) };
        v.save()?;
        Ok(v)
    }

    fn save(&self) -> Result<()> {
        let data = serde_json::to_vec_pretty(&self.file).map_err(|e| Error::Format(e.to_string()))?;
        write_atomic(&self.path(), &data, true)
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub fn has_recovery(&self) -> bool {
        self.file.mk_recovery.is_some()
    }

    fn opened(&self) -> Result<&Open> {
        self.open.as_ref().ok_or(Error::WrongKey)
    }

    /// Ana anahtarla açar: yalnızca kayıt adları çözülür.
    fn open_with(&mut self, mk: Key) -> Result<()> {
        let mut items = Vec::with_capacity(self.file.entries.len());
        for e in &self.file.entries {
            let b = crypto::open(&mk, &aad("meta", &e.id), &e.meta)?;
            items.push(Item { id: e.id.clone(), meta: from_json(&b)? });
        }
        items.sort_by_cached_key(|i| i.meta.name.to_lowercase());
        self.open = Some(Open { mk, items });
        Ok(())
    }

    pub fn unlock(&mut self, password: &str) -> Result<()> {
        let pk = self.file.kdf.derive(password.as_bytes())?;
        let mk = crypto::unwrap_key(&pk, AAD_MK_PW, &self.file.mk_password)?;
        self.open_with(mk)
    }

    /// Parola doğru mu (kasa durumunu değiştirmez; parola değiştirirken sorulur).
    pub fn verify_password(&self, password: &str) -> bool {
        self.file.kdf.derive(password.as_bytes()).and_then(|pk| crypto::unwrap_key(&pk, AAD_MK_PW, &self.file.mk_password)).is_ok()
    }

    pub fn unlock_recovery(&mut self, text: &str) -> Result<()> {
        let rk = recovery::parse(text).ok_or(Error::WrongKey)?;
        let w = self.file.mk_recovery.as_ref().ok_or(Error::WrongKey)?;
        let mk = crypto::unwrap_key(&rk, AAD_MK_REC, w)?;
        self.open_with(mk)
    }

    /// Ana anahtarı ve adları bellekten siler.
    pub fn lock(&mut self) {
        self.open = None;
    }

    /// Yeni parola: yeni tuz ile parola sarmalı yenilenir (kayıtlara dokunulmaz).
    pub fn change_password(&mut self, password: &str) -> Result<()> {
        let kdf = KdfParams { salt: crypto::random::<16>()?.to_vec(), ..self.file.kdf.clone() };
        self.change_password_kdf(password, kdf)
    }

    fn change_password_kdf(&mut self, password: &str, kdf: KdfParams) -> Result<()> {
        let pk = kdf.derive(password.as_bytes())?;
        let w = crypto::wrap_key(&pk, AAD_MK_PW, &self.opened()?.mk)?;
        self.file.kdf = kdf;
        self.file.mk_password = w;
        self.save()
    }

    /// Yeni kurtarma anahtarı üretir (eskisi geçersiz olur) ve gösterilmek üzere döner.
    pub fn new_recovery(&mut self) -> Result<Zeroizing<String>> {
        let rk = recovery::new()?;
        self.file.mk_recovery = Some(crypto::wrap_key(&rk, AAD_MK_REC, &self.opened()?.mk)?);
        self.save()?;
        Ok(recovery::format(&rk))
    }

    pub fn remove_recovery(&mut self) -> Result<()> {
        self.opened()?;
        self.file.mk_recovery = None;
        self.save()
    }

    pub fn items(&self) -> &[Item] {
        self.open.as_ref().map(|o| o.items.as_slice()).unwrap_or(&[])
    }

    fn rec(&self, id: &str) -> Result<&EntryRec> {
        self.file.entries.iter().find(|e| e.id == id).ok_or(Error::Corrupt)
    }

    fn dek(&self, id: &str) -> Result<Key> {
        let mk = &self.opened()?.mk;
        crypto::open(mk, &aad("dek", id), &self.rec(id)?.dek).and_then(|b| Key::from_bytes(&b).ok_or(Error::Corrupt))
    }

    /// Yalnızca bu kaydın alanlarını çözer.
    pub fn decrypt(&self, id: &str) -> Result<Body> {
        let dek = self.dek(id)?;
        let b = crypto::open(&dek, &aad("body", id), &self.rec(id)?.body)?;
        from_json(&b)
    }

    fn seal_rec(&self, id: &str, dek: &Key, meta: &Meta, body: &Body) -> Result<EntryRec> {
        let mk = &self.opened()?.mk;
        Ok(EntryRec {
            id: id.to_string(),
            meta: crypto::seal(mk, &aad("meta", id), &to_json(meta)?)?,
            dek: crypto::wrap_key(mk, &aad("dek", id), dek)?,
            body: crypto::seal(dek, &aad("body", id), &to_json(body)?)?,
        })
    }

    fn put_item(&mut self, id: &str, meta: Meta) {
        if let Some(o) = self.open.as_mut() {
            o.items.retain(|i| i.id != id);
            o.items.push(Item { id: id.to_string(), meta });
            o.items.sort_by_cached_key(|i| i.meta.name.to_lowercase());
        }
    }

    /// Yeni kayıt: kendi rastgele anahtarıyla. Kimliği döner.
    pub fn add(&mut self, meta: Meta, body: &Body) -> Result<String> {
        let id = hex::encode(&crypto::random::<12>()?);
        let rec = self.seal_rec(&id, &Key::random()?, &meta, body)?;
        self.file.entries.push(rec);
        self.save()?;
        self.put_item(&id, meta);
        Ok(id)
    }

    /// Kaydı yeniden mühürler (yeni nonce'larla, aynı kayıt anahtarıyla).
    pub fn update(&mut self, id: &str, meta: Meta, body: &Body) -> Result<()> {
        let rec = self.seal_rec(id, &self.dek(id)?, &meta, body)?;
        let slot = self.file.entries.iter_mut().find(|e| e.id == id).ok_or(Error::Corrupt)?;
        *slot = rec;
        self.save()?;
        self.put_item(id, meta);
        Ok(())
    }

    /// Kayıtların kategorisini değiştirir (yalnızca `Meta` yeniden mühürlenir);
    /// dosya bir kez yazılır, hata olursa hiçbiri değişmez. Değişen sayısını döner.
    pub fn set_categories(&mut self, ids: &[String], category: Category) -> Result<usize> {
        let before = self.file.entries.clone();
        let mut metas = Vec::new();
        for id in ids {
            let Some(item) = self.items().iter().find(|i| &i.id == id) else { continue };
            let mut meta = item.meta.clone();
            meta.category = category;
            let sealed = crypto::seal(&self.opened()?.mk, &aad("meta", id), &to_json(&meta)?)?;
            self.file.entries.iter_mut().find(|e| &e.id == id).ok_or(Error::Corrupt)?.meta = sealed;
            metas.push((id.clone(), meta));
        }
        if metas.is_empty() {
            return Ok(0);
        }
        if let Err(e) = self.save() {
            self.file.entries = before;
            return Err(e);
        }
        let n = metas.len();
        for (id, meta) in metas {
            self.put_item(&id, meta);
        }
        Ok(n)
    }

    /// Çok kaydı tek seferde ekler (içe aktarma): hepsi mühürlenir, dosya bir kez
    /// yazılır. Eklenen kayıt sayısını döner.
    pub fn add_many(&mut self, items: Vec<(Meta, Body)>) -> Result<usize> {
        let mut recs = Vec::with_capacity(items.len());
        let mut metas = Vec::with_capacity(items.len());
        for (meta, body) in &items {
            let id = hex::encode(&crypto::random::<12>()?);
            recs.push(self.seal_rec(&id, &Key::random()?, meta, body)?);
            metas.push((id, meta.clone()));
        }
        let n = recs.len();
        self.file.entries.extend(recs);
        if let Err(e) = self.save() {
            let keep = self.file.entries.len() - n;
            self.file.entries.truncate(keep);
            return Err(e);
        }
        if let Some(o) = self.open.as_mut() {
            o.items.extend(metas.into_iter().map(|(id, meta)| Item { id, meta }));
            o.items.sort_by_cached_key(|i| i.meta.name.to_lowercase());
        }
        Ok(n)
    }

    pub fn delete(&mut self, id: &str) -> Result<()> {
        self.opened()?;
        self.file.entries.retain(|e| e.id != id);
        self.save()?;
        let _ = std::fs::remove_file(self.blob(id));
        if let Some(o) = self.open.as_mut() {
            o.items.retain(|i| i.id != id);
        }
        Ok(())
    }

    /// Ek dosyayı kaydın anahtarıyla mühürleyip yazar; gövdeye konacak bilgiyi döner.
    pub fn attach(&self, id: &str, name: &str, data: &[u8]) -> Result<Attachment> {
        if data.len() as u64 > MAX_FILE {
            return Err(Error::Io(carlui_core::t!("Dosya 25 MB'tan büyük.", "File is larger than 25 MB.").into()));
        }
        let dek = self.dek(id)?;
        let s = crypto::seal(&dek, &aad("file", id), data)?;
        let path = self.blob(id);
        std::fs::create_dir_all(path.parent().unwrap_or(&self.dir)).map_err(io)?;
        write_atomic(&path, &serde_json::to_vec(&s).map_err(|e| Error::Format(e.to_string()))?, false)?;
        Ok(Attachment { name: name.to_string(), size: data.len() as u64 })
    }

    pub fn read_file(&self, id: &str) -> Result<Zeroizing<Vec<u8>>> {
        let dek = self.dek(id)?;
        let raw = std::fs::read(self.blob(id)).map_err(io)?;
        let s: Sealed = serde_json::from_slice(&raw).map_err(|_| Error::Corrupt)?;
        crypto::open(&dek, &aad("file", id), &s)
    }

    pub fn remove_file(&self, id: &str) {
        let _ = std::fs::remove_file(self.blob(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{Field, FieldKind};

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("carlui-vault-test-{}", hex::encode(&crypto::random::<8>().unwrap())));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn sample(name: &str, secret: &str) -> (Meta, Body) {
        let mut f = Field::new("Parola", FieldKind::Secret);
        f.value = secret.into();
        (Meta { name: name.into(), category: Category::Password, updated: 1 }, Body { fields: vec![f], file: None })
    }

    #[test]
    fn roundtrip_through_disk() {
        let d = tmp();
        let mut v = Vault::create(&d, "doğru-parola", KdfParams::fast()).unwrap();
        let (m, b) = sample("GitHub", "s3cr3t-ğüş");
        let id = v.add(m, &b).unwrap();
        let raw = std::fs::read_to_string(d.join("vault.json")).unwrap();
        assert!(!raw.contains("GitHub") && !raw.contains("s3cr3t"), "düz metin diske yazılmamalı");

        let mut v2 = Vault::load(&d).unwrap();
        assert!(!v2.is_open());
        assert!(v2.decrypt(&id).is_err(), "kilitliyken çözülmemeli");
        v2.unlock("doğru-parola").unwrap();
        assert_eq!(v2.items()[0].meta.name, "GitHub");
        assert_eq!(v2.decrypt(&id).unwrap().fields[0].value, "s3cr3t-ğüş");

        let (m, mut b) = sample("GitHub 2", "yeni");
        b.fields.push(Field::new("Not", FieldKind::Multiline));
        v2.update(&id, m, &b).unwrap();
        v2.lock();
        assert!(v2.items().is_empty());
        let mut v3 = Vault::load(&d).unwrap();
        v3.unlock("doğru-parola").unwrap();
        assert_eq!(v3.items()[0].meta.name, "GitHub 2");
        assert_eq!(v3.decrypt(&id).unwrap().fields.len(), 2);
        v3.delete(&id).unwrap();
        assert!(Vault::load(&d).unwrap().file.entries.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn wrong_password() {
        let d = tmp();
        Vault::create(&d, "parola1", KdfParams::fast()).unwrap();
        let mut v = Vault::load(&d).unwrap();
        assert_eq!(v.unlock("parola2").err(), Some(Error::WrongKey));
        assert_eq!(v.unlock("").err(), Some(Error::WrongKey));
        assert!(!v.is_open());
        v.unlock("parola1").unwrap();
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn corrupted_data() {
        let d = tmp();
        let mut v = Vault::create(&d, "pw", KdfParams::fast()).unwrap();
        let (m, b) = sample("A", "x");
        let id = v.add(m, &b).unwrap();
        // Gövdenin bir baytını değiştir: kasa açılır ama kayıt çözülmez.
        let mut f = Vault::load(&d).unwrap().file;
        f.entries[0].body.c[3] ^= 0x40;
        std::fs::write(d.join("vault.json"), serde_json::to_vec(&f).unwrap()).unwrap();
        let mut v = Vault::load(&d).unwrap();
        v.unlock("pw").unwrap();
        assert_eq!(v.decrypt(&id).err(), Some(Error::Corrupt));
        // Adın bozulması kasayı açtırmaz (sessizce yanlış ad göstermez).
        f.entries[0].meta.c[0] ^= 1;
        std::fs::write(d.join("vault.json"), serde_json::to_vec(&f).unwrap()).unwrap();
        assert_eq!(Vault::load(&d).unwrap().unlock("pw").err(), Some(Error::Corrupt));
        // Parola sarmalının bozulması yanlış parola gibi reddedilir.
        f.entries.clear();
        f.mk_password.c[0] ^= 1;
        std::fs::write(d.join("vault.json"), serde_json::to_vec(&f).unwrap()).unwrap();
        assert_eq!(Vault::load(&d).unwrap().unlock("pw").err(), Some(Error::WrongKey));
        // Geçersiz JSON.
        std::fs::write(d.join("vault.json"), b"{bozuk").unwrap();
        assert!(matches!(Vault::load(&d).err(), Some(Error::Format(_))));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn tampered_ids_and_kdf() {
        let d = tmp();
        let mut v = Vault::create(&d, "pw", KdfParams::fast()).unwrap();
        let (m, b) = sample("A", "x");
        v.add(m, &b).unwrap();
        let good = Vault::load(&d).unwrap().file;
        // Kimlik dosya yoluna çevrilirse (files\<id>.bin) kasa hiç açılmaz:
        // ad AAD'ye bağlı olduğundan çözülmez, yol kullanılmadan reddedilir.
        let mut f = good.clone();
        f.entries[0].id = r"..\..\kotu".into();
        std::fs::write(d.join("vault.json"), serde_json::to_vec(&f).unwrap()).unwrap();
        assert_eq!(Vault::load(&d).unwrap().unlock("pw").err(), Some(Error::Corrupt));
        // Argon2 ayarları kurcalanırsa (bellek 4 GiB) donmadan reddedilir.
        let mut f = good.clone();
        f.kdf.m_kib = 4 * 1024 * 1024;
        std::fs::write(d.join("vault.json"), serde_json::to_vec(&f).unwrap()).unwrap();
        assert!(matches!(Vault::load(&d).unwrap().unlock("pw").err(), Some(Error::Format(_))));
        // Biçim sürümü değiştirilirse yüklenmez.
        let mut f = good.clone();
        f.format = 99;
        std::fs::write(d.join("vault.json"), serde_json::to_vec(&f).unwrap()).unwrap();
        assert!(Vault::load(&d).is_err());
        // Kesilmiş dosya.
        let raw = serde_json::to_vec(&good).unwrap();
        std::fs::write(d.join("vault.json"), &raw[..raw.len() / 2]).unwrap();
        assert!(Vault::load(&d).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn per_entry_keys_are_separate() {
        let d = tmp();
        let mut v = Vault::create(&d, "pw", KdfParams::fast()).unwrap();
        let (m1, b1) = sample("Bir", "birinci");
        let (m2, b2) = sample("İki", "ikinci");
        let a = v.add(m1, &b1).unwrap();
        let b = v.add(m2, &b2).unwrap();
        let ka = v.dek(&a).unwrap();
        let kb = v.dek(&b).unwrap();
        assert_ne!(ka, kb, "her kaydın kendi anahtarı olmalı");
        // A'nın anahtarı B'nin gövdesini açamaz.
        assert!(crypto::open(&ka, &aad("body", &b), &v.rec(&b).unwrap().body).is_err());
        // Gövdeler yer değiştirilirse (AAD kimliğe bağlı) çözülmez.
        let mut f = v.file.clone();
        let (x, y) = (f.entries[0].body.clone(), f.entries[1].body.clone());
        f.entries[0].body = y;
        f.entries[1].body = x;
        std::fs::write(d.join("vault.json"), serde_json::to_vec(&f).unwrap()).unwrap();
        let mut v2 = Vault::load(&d).unwrap();
        v2.unlock("pw").unwrap();
        assert!(v2.decrypt(&a).is_err() && v2.decrypt(&b).is_err());
        // Bir kaydın gövdesi bozuksa diğeri etkilenmez.
        let mut f = v.file.clone();
        let i = f.entries.iter().position(|e| e.id == a).unwrap();
        f.entries[i].body.c[0] ^= 1;
        std::fs::write(d.join("vault.json"), serde_json::to_vec(&f).unwrap()).unwrap();
        let mut v3 = Vault::load(&d).unwrap();
        v3.unlock("pw").unwrap();
        assert!(v3.decrypt(&a).is_err());
        assert_eq!(v3.decrypt(&b).unwrap().fields[0].value, "ikinci");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn recovery_and_password_change() {
        let d = tmp();
        let mut v = Vault::create(&d, "eski", KdfParams::fast()).unwrap();
        let (m, b) = sample("A", "değer");
        let id = v.add(m, &b).unwrap();
        let key = v.new_recovery().unwrap();
        let mut v2 = Vault::load(&d).unwrap();
        assert_eq!(v2.unlock_recovery("0000-1111").err(), Some(Error::WrongKey));
        v2.unlock_recovery(&key).unwrap();
        assert_eq!(v2.decrypt(&id).unwrap().fields[0].value, "değer");
        v2.change_password_kdf("yeni", KdfParams::fast()).unwrap();
        let mut v3 = Vault::load(&d).unwrap();
        assert_eq!(v3.unlock("eski").err(), Some(Error::WrongKey));
        v3.unlock("yeni").unwrap();
        // Yeni kurtarma anahtarı eskisini geçersiz kılar; kaldırınca hiçbiri çalışmaz.
        let key2 = v3.new_recovery().unwrap();
        assert!(Vault::load(&d).unwrap().unlock_recovery(&key).is_err());
        assert!(Vault::load(&d).unwrap().unlock_recovery(&key2).is_ok());
        v3.remove_recovery().unwrap();
        assert!(Vault::load(&d).unwrap().unlock_recovery(&key2).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn add_many_writes_once() {
        let d = tmp();
        let mut v = Vault::create(&d, "pw", KdfParams::fast()).unwrap();
        let items: Vec<_> = (0..50).map(|i| sample(&format!("K{i:02}"), "x")).collect();
        assert_eq!(v.add_many(items).unwrap(), 50);
        assert_eq!(v.items().len(), 50);
        let mut v2 = Vault::load(&d).unwrap();
        v2.unlock("pw").unwrap();
        assert_eq!(v2.items().len(), 50);
        assert_eq!(v2.items()[0].meta.name, "K00");
        let id = v2.items()[7].id.clone();
        assert_eq!(v2.decrypt(&id).unwrap().fields[0].value, "x");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn import_round_trip() {
        // Dosyadan kasaya: içe aktarılan kayıtlar şifreli yazılır, geri açılır
        // ve ikinci aktarımda "kasada var" sayılır (karttaki biçim yükseltmesi dahil).
        use crate::import;
        let csv = "name,url,username,password,note\nRoblox,https://www.roblox.com/login,carl,Gizli-Parola-123,\n";
        let json = r#"{"encrypted":false,"items":[{"type":3,"name":"Ziraat","card":{"cardholderName":"CARL","number":"4111 1111 1111 1111","expMonth":"4","expYear":"2027","code":"123"}}]}"#;
        let d = tmp();
        let mut v = Vault::create(&d, "pw", KdfParams::fast()).unwrap();
        let mut keys = Vec::new();
        for data in [csv.as_bytes(), json.as_bytes()] {
            let b = import::parse(data).unwrap();
            let items: Vec<_> = b
                .items
                .into_iter()
                .map(|i| {
                    keys.push(import::key(i.meta.category, &i.meta.name, &i.body));
                    let mut body = i.body.clone();
                    crate::card::upgrade(i.meta.category, &mut body);
                    (i.meta.clone(), body)
                })
                .collect();
            v.add_many(items).unwrap();
        }
        assert_eq!(v.items().len(), 2);
        for f in std::fs::read_dir(&d).unwrap() {
            let raw = std::fs::read(f.unwrap().path()).unwrap();
            assert!(!raw.windows(10).any(|w| w == b"Gizli-Paro"), "parola düz yazılmamalı");
            assert!(!raw.windows(8).any(|w| w == b"41111111"), "kart no düz yazılmamalı");
        }
        let mut v2 = Vault::load(&d).unwrap();
        v2.unlock("pw").unwrap();
        for (it, k) in v2.items().iter().zip(&keys) {
            let body = v2.decrypt(&it.id).unwrap();
            assert_eq!(&import::key(it.meta.category, &it.meta.name, &body), k, "{} tekrar eklenirdi", it.meta.name);
        }
        let card = v2.items().iter().find(|i| i.meta.category == crate::entry::Category::Card).unwrap();
        let body = v2.decrypt(&card.id).unwrap();
        assert_eq!(crate::card::Part::NumberSpaced.value(&body).unwrap().as_str(), "4111 1111 1111 1111");
        assert_eq!(crate::card::Part::MmYyyy.value(&body).unwrap().as_str(), "04/2027");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn set_categories_persists() {
        let d = tmp();
        let mut v = Vault::create(&d, "pw", KdfParams::fast()).unwrap();
        let a = v.add(sample("Gmail", "x").0, &sample("Gmail", "x").1).unwrap();
        let b = v.add(sample("GitHub", "y").0, &sample("GitHub", "y").1).unwrap();
        assert_eq!(v.set_categories(&[a.clone(), "yok".into()], Category::Email).unwrap(), 1);
        let mut v2 = Vault::load(&d).unwrap();
        v2.unlock("pw").unwrap();
        let cat = |id: &str| v2.items().iter().find(|i| i.id == id).unwrap().meta.category;
        assert_eq!((cat(&a), cat(&b)), (Category::Email, Category::Password));
        assert_eq!(v2.decrypt(&a).unwrap().fields[0].value, "x", "içerik bozulmaz");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn attachments() {
        let d = tmp();
        let mut v = Vault::create(&d, "pw", KdfParams::fast()).unwrap();
        let (m, b) = sample("Dosya", "");
        let id = v.add(m, &b).unwrap();
        let data = b"PDF gizli icerik".to_vec();
        let att = v.attach(&id, "belge.pdf", &data).unwrap();
        assert_eq!(att.size, data.len() as u64);
        let raw = std::fs::read(v.blob(&id)).unwrap();
        assert!(!raw.windows(5).any(|w| w == b"gizli"));
        assert_eq!(&v.read_file(&id).unwrap()[..], &data[..]);
        let (m2, b2) = sample("Başka", "");
        let other = v.add(m2, &b2).unwrap();
        std::fs::copy(v.blob(&id), v.blob(&other)).unwrap();
        assert!(v.read_file(&other).is_err(), "başka kaydın dosyası açılmamalı");
        let _ = std::fs::remove_dir_all(&d);
    }
}
