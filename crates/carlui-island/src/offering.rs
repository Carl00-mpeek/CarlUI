//! Bu modül tarayıcı eklentisinden gelen indirme tekliflerine adanın nasıl
//! karar verdirdiğini çözüyor: teklif sıraya girer, ada "İndirme bilgisi"
//! kartını gösterir (bkz. `island/offer.rs`). "İndir" CarlUI'nın indirme
//! yöneticisini başlatır, "Farklı kaydet…" Windows'un kaydetme penceresini
//! açar, "Tarayıcıya bırak" duraklatılmış tarayıcı indirmesini sürdürür.
//! Oyundayken kart açılmaz; indirme sessizce CarlUI'ya alınır.

use crate::{bridge::Choice, *};
use std::path::PathBuf;
use windows::Win32::{
    System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
    UI::Shell::{Common::COMDLG_FILTERSPEC, FileSaveDialog, IFileSaveDialog, IShellItem, SHCreateItemFromParsingName, FOS_OVERWRITEPROMPT, SIGDN_FILESYSPATH},
};

impl App {
    pub(crate) fn offers_arrived(&mut self) {
        for o in bridge::take_offers() {
            if self.fullscreen || self.game {
                let mut o = o;
                o.answer(Choice::CarlUI);
                self.dl_start_with(o.url.clone(), o.headers.clone(), Some(o.name.clone()), None);
            } else {
                self.offers.push_back(o);
            }
        }
        self.refresh(true);
    }

    pub(crate) fn offer_choice(&mut self, b: u8) {
        let Some(mut o) = self.offers.pop_front() else { return };
        if o.at.elapsed() > bridge::OFFER_TTL {
            // Köprü çoktan "tarayıcı" demiş; ikinci kez indirmeyelim.
            self.push_note(Note { app: t!("İndirmeler", "Downloads").into(), title: t!("İndirme tarayıcıda sürdü", "Download continued in the browser").into(), body: o.name.clone(), ..Default::default() });
            self.refresh(true);
            return;
        }
        match b {
            0 => {
                o.answer(Choice::CarlUI);
                self.dl_start_with(o.url.clone(), o.headers.clone(), Some(o.name.clone()), None);
            }
            1 => match save_as(self.island.hwnd, &o.name) {
                Some(p) => {
                    o.answer(Choice::CarlUI);
                    self.dl_start_with(o.url.clone(), o.headers.clone(), None, Some(p));
                }
                None => self.offers.push_front(o),
            },
            _ => {
                info!("indirme tarayıcıya bırakıldı: {}", o.name);
                o.answer(Choice::Browser);
            }
        }
        self.refresh(true);
    }
}

/// Windows'un "Farklı kaydet" penceresi; İndirilenler'de açılır.
fn save_as(owner: HWND, name: &str) -> Option<PathBuf> {
    unsafe {
        let d: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let _ = d.SetFileName(&HSTRING::from(name));
        let _ = d.SetTitle(&HSTRING::from(t!("Farklı kaydet", "Save as")));
        if let Some(ext) = name.rsplit_once('.').map(|p| p.1.to_string()).filter(|e| !e.is_empty() && e.len() <= 8) {
            let spec = HSTRING::from(format!("*.{ext}"));
            let label = HSTRING::from(t!(format!("{} dosyası", ext.to_uppercase()), format!("{} file", ext.to_uppercase())));
            let any = HSTRING::from(t!("Tüm dosyalar", "All files"));
            let all = [COMDLG_FILTERSPEC { pszName: PCWSTR(label.as_ptr()), pszSpec: PCWSTR(spec.as_ptr()) }, COMDLG_FILTERSPEC { pszName: PCWSTR(any.as_ptr()), pszSpec: w!("*.*") }];
            let _ = d.SetFileTypes(&all);
        }
        if let Some(dir) = carlui_core::win::downloads_dir() {
            if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(dir.as_os_str()), None) {
                let _ = d.SetFolder(&item);
            }
        }
        let _ = d.SetOptions(d.GetOptions().unwrap_or_default() | FOS_OVERWRITEPROMPT);
        d.Show(owner).ok()?;
        let p = d.GetResult().ok()?.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = p.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as _));
        s.map(PathBuf::from)
    }
}
