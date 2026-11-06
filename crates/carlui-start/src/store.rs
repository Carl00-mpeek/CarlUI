//! Menünün içeriği: sabitlenmiş uygulamalar ve klasörler. Kullanıcının kendi
//! düzeni olduğu için ayarların yanında, `%APPDATA%\CarlUI\start.json`'da
//! tutulur. Dosya ilk kez oluşturulurken Dosya Gezgini, Windows Ayarları ve
//! dock'ta sabitlenmiş uygulamalarla doldurulur.

use carlui_core::{config, paths, warn};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Tile {
    pub name: String,
    /// Exe, .lnk ya da `shell:AppsFolder\<kimlik>`; ortam değişkeni içerebilir.
    pub path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub args: String,
}

impl Tile {
    pub fn same(&self, o: &Tile) -> bool {
        self.path.eq_ignore_ascii_case(&o.path) || self.name.to_lowercase() == o.name.to_lowercase()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Item {
    App(Tile),
    Folder { name: String, apps: Vec<Tile> },
}

impl Item {
    pub fn name(&self) -> &str {
        match self {
            Item::App(t) => &t.name,
            Item::Folder { name, .. } => name,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Store {
    pub items: Vec<Item>,
    /// Tam Kaldırıcı kısayolu bir kez eklendi. Kullanıcı sonradan menüden
    /// çıkarırsa geri gelmesin diye ayrıca tutulur.
    #[serde(default)]
    pub uninstaller: bool,
}

fn file() -> PathBuf {
    paths::config_file().with_file_name("start.json")
}

fn tile(name: &str, path: &str) -> Tile {
    Tile { name: name.into(), path: path.into(), args: String::new() }
}

fn uninstaller() -> Tile {
    tile(t!("Tam Kaldırıcı", "Uninstaller"), &paths::sibling_exe("carlui-uninstall.exe").to_string_lossy())
}

impl Store {
    pub fn load() -> Self {
        match std::fs::read(file()) {
            Ok(b) => match serde_json::from_slice::<Self>(&b) {
                Ok(mut s) => {
                    if s.adopt(uninstaller()) {
                        s.save();
                    }
                    s
                }
                Err(e) => {
                    warn!("start.json okunamadı ({e}); boş menüyle devam");
                    Self::default()
                }
            },
            Err(_) => {
                let s = Self::seed();
                s.save();
                s
            }
        }
    }

    /// Eski menülere kaldırıcı kısayolunu bir kereliğine ekler; değiştiyse doğru.
    fn adopt(&mut self, t: Tile) -> bool {
        if self.uninstaller {
            return false;
        }
        self.uninstaller = true;
        if !self.contains(None, &t) {
            self.items.push(Item::App(t));
        }
        true
    }

    fn seed() -> Self {
        let mut items = vec![
            Item::App(tile(t!("Dosya Gezgini", "File Explorer"), r"shell:AppsFolder\Microsoft.Windows.Explorer")),
            Item::App(tile(t!("Ayarlar", "Settings"), r"shell:AppsFolder\windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel")),
        ];
        for p in config::load().dock.pinned {
            if !p.path.to_lowercase().ends_with("explorer.exe") {
                items.push(Item::App(Tile { name: p.name, path: p.path, args: p.args }));
            }
        }
        items.push(Item::App(uninstaller()));
        Self { items, uninstaller: true }
    }

    pub fn save(&self) {
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            if let Err(e) = std::fs::write(file(), json) {
                warn!("start.json yazılamadı: {e}");
            }
        }
    }

    /// Ana listede ya da `folder` içinde bu uygulama sabitli mi? Dock'tan gelen
    /// sabitlemeler exe yolu tuttuğu için ad da eşleşme sayılır.
    pub fn contains(&self, folder: Option<usize>, t: &Tile) -> bool {
        let same = |x: &Tile| x.same(t);
        match folder.and_then(|f| self.items.get(f)) {
            Some(Item::Folder { apps, .. }) => apps.iter().any(same),
            _ => self.items.iter().any(|i| matches!(i, Item::App(t) if same(t))),
        }
    }

    /// Ekle/çıkar (uygulama ekleme listesindeki onay işareti).
    pub fn toggle(&mut self, folder: Option<usize>, t: Tile) {
        let key = t.clone();
        let same = |x: &Tile| x.same(&key);
        match folder.and_then(|f| self.items.get_mut(f)) {
            Some(Item::Folder { apps, .. }) => match apps.iter().position(same) {
                Some(i) => {
                    apps.remove(i);
                }
                None => apps.push(t),
            },
            _ => match self.items.iter().position(|i| matches!(i, Item::App(x) if same(x))) {
                Some(i) => {
                    self.items.remove(i);
                }
                None => self.items.push(Item::App(t)),
            },
        }
    }

    /// Yeni boş klasör; konumu döner.
    pub fn new_folder(&mut self) -> usize {
        let n = self.items.iter().filter(|i| matches!(i, Item::Folder { .. })).count();
        let base = t!("Yeni klasör", "New folder");
        let name = if n == 0 { base.to_string() } else { format!("{base} {}", n + 1) };
        self.items.push(Item::Folder { name, apps: Vec::new() });
        self.items.len() - 1
    }

    /// Klasörü dağıt: içindekiler klasörün yerine ana listeye geçer.
    pub fn dissolve(&mut self, f: usize) {
        if let Some(Item::Folder { apps, .. }) = self.items.get(f).cloned() {
            self.items.splice(f..=f, apps.into_iter().map(Item::App));
        }
    }

    /// Klasördeki uygulamayı ana listeye, klasörün hemen arkasına taşır.
    pub fn move_out(&mut self, f: usize, i: usize) {
        if let Some(Item::Folder { apps, .. }) = self.items.get_mut(f) {
            if i < apps.len() {
                let t = apps.remove(i);
                self.items.insert(f + 1, Item::App(t));
            }
        }
    }

    pub fn remove(&mut self, folder: Option<usize>, i: usize) {
        match folder {
            Some(f) => {
                if let Some(Item::Folder { apps, .. }) = self.items.get_mut(f) {
                    if i < apps.len() {
                        apps.remove(i);
                    }
                }
            }
            None if i < self.items.len() => {
                self.items.remove(i);
            }
            None => {}
        }
    }

    /// Sürükle-bırak: `from` öğesini `to` konumuna taşır.
    pub fn reorder(&mut self, folder: Option<usize>, from: usize, to: usize) {
        fn mv<T>(v: &mut Vec<T>, from: usize, to: usize) {
            if from < v.len() {
                let x = v.remove(from);
                v.insert(to.min(v.len()), x);
            }
        }
        match folder {
            Some(f) => {
                if let Some(Item::Folder { apps, .. }) = self.items.get_mut(f) {
                    mv(apps, from, to);
                }
            }
            None => mv(&mut self.items, from, to),
        }
    }

    /// Ana listede bir uygulamayı başka bir öğenin üstüne bırakmak: hedef
    /// uygulamaysa ikisinden yeni klasör olur, klasörse içine girer.
    /// Yeni klasör oluştuysa konumu döner.
    pub fn merge(&mut self, from: usize, onto: usize) -> Option<usize> {
        if from == onto || from >= self.items.len() || onto >= self.items.len() {
            return None;
        }
        let Item::App(src) = self.items[from].clone() else { return None };
        let made = match &mut self.items[onto] {
            Item::Folder { apps, .. } => {
                apps.push(src);
                false
            }
            Item::App(dst) => {
                let dst = dst.clone();
                self.items[onto] = Item::Folder { name: t!("Klasör", "Folder").into(), apps: vec![dst, src] };
                true
            }
        };
        self.items.remove(from);
        let at = if from < onto { onto - 1 } else { onto };
        made.then_some(at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kaldirici_bir_kez_eklenir_ve_geri_gelmez() {
        let t = tile("Tam Kaldırıcı", "C:/x/carlui-uninstall.exe");
        let mut s: Store = serde_json::from_str(r#"{"items":[]}"#).unwrap();
        assert!(s.adopt(t.clone()));
        assert_eq!(s.items.len(), 1);
        s.remove(None, 0);
        assert!(!s.adopt(t));
        assert!(s.items.is_empty());
    }

    #[test]
    fn zaten_sabitliyse_cift_eklenmez() {
        let t = tile("Tam Kaldırıcı", "C:/x/carlui-uninstall.exe");
        let mut s = Store { items: vec![Item::App(t.clone())], uninstaller: false };
        assert!(s.adopt(t));
        assert_eq!(s.items.len(), 1);
    }
}
