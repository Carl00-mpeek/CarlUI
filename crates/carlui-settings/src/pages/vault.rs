//! Carl Vault: otomatik kilit süresi, uyku/oturum kilidinde kilitleme, panodan
//! silme süresi, pano belirteci ve ekran yakalamadan gizleme. Kısayol Kısayollar
//! sayfasında.

use crate::{
    ui::{Flow, P},
    App,
};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use windows::{
    Security::Credentials::KeyCredentialManager,
    Win32::{
        Foundation::HWND,
        Graphics::Gdi::InvalidateRect,
        System::Com::{CoInitializeEx, COINIT_MULTITHREADED},
    },
};

/// 0 bilinmiyor, 1 var, 2 yok.
static HELLO: AtomicU8 = AtomicU8::new(0);
static PROBING: AtomicBool = AtomicBool::new(false);

/// Bu bilgisayarda Windows Hello kurulu mu. İlk çağrıda arka planda yoklanır
/// (birkaç yüz ms); bitince sayfa yeniden çizilir. Yokken ayar gösterilmez.
fn hello_available(app: &App) -> bool {
    match HELLO.load(Ordering::Relaxed) {
        1 => return true,
        2 => return false,
        _ => {}
    }
    if !PROBING.swap(true, Ordering::Relaxed) {
        let h = app.hwnd.0 as isize;
        std::thread::spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            let ok = KeyCredentialManager::IsSupportedAsync().and_then(|op| op.get()).unwrap_or(false);
            HELLO.store(if ok { 1 } else { 2 }, Ordering::Relaxed);
            unsafe {
                let _ = InvalidateRect(HWND(h as _), None, false);
            }
        });
    }
    false
}

pub fn paint(app: &App, p: &mut P, f: &mut Flow) {
    let c = &app.cfg.vault;
    f.section(p, t!("Otomatik kilit", "Auto-lock"));
    f.slider(
        p,
        t!("Boşta kalınca kilitle", "Lock when idle"),
        t!("Bu süre klavye/fare kullanılmazsa kasa kilitlenir.", "The vault locks after this long without keyboard/mouse use."),
        c.auto_lock_minutes.max(1) as f32,
        1.0,
        60.0,
        1.0,
        |v| t!(format!("{v:.0} dk"), format!("{v:.0} min")),
        |c, v| c.vault.auto_lock_minutes = v as u32,
    );
    f.toggle(p, t!("Uykuda ve oturum kilitlenince kilitle", "Lock on sleep and session lock"), t!("Bilgisayar uykuya geçerken ya da Win+L ile kilitlenirken.", "When the computer sleeps or is locked with Win+L."), c.lock_on_sleep, |c| c.vault.lock_on_sleep ^= true);
    f.section(p, t!("Gizlilik", "Privacy"));
    f.toggle(
        p,
        t!("Panoya değer yerine belirteç koy", "Put a token on the clipboard, not the value"),
        t!("Kopyalanan alan Win+V'de kasa kaydı olarak görünür; seçince hedef pencereye kasa yazar. Gerçek değer panoya girmez.", "The copied field shows up in Win+V as a vault item; picking it makes the vault type it into the target window. The real value never enters the clipboard."),
        c.token_paste,
        |c| c.vault.token_paste ^= true,
    );
    if c.token_paste {
        f.slider(
            p,
            t!("Belirteç süresi", "Token lifetime"),
            t!("Belirteç bu süre içinde Win+V'den seçilmezse geçersiz olur.", "The token expires if it isn't picked from Win+V within this time."),
            c.token_seconds.clamp(15, 300) as f32,
            15.0,
            300.0,
            15.0,
            |v| t!(format!("{v:.0} sn"), format!("{v:.0} s")),
            |c, v| c.vault.token_seconds = v as u32,
        );
    }
    f.toggle(
        p,
        t!("Tarayıcıda doldurma", "Fill in the browser"),
        t!("CarlUI eklentisinde bir kayıt seçince giriş sayfası doldurulur. Yalnızca adresi o siteye uyan kayıtlar görünür.", "Picking an item in the CarlUI extension fills the sign-in page. Only items whose address matches that site are shown."),
        c.browser_fill,
        |c| c.vault.browser_fill ^= true,
    );
    f.toggle(
        p,
        t!("Uygulamalarda doldurma", "Fill in apps"),
        t!("Steam, Discord gibi uygulamaların giriş ekranında \"Vault\" düğmesi çıkar; kayıt seçince giriş bilgisi yazılır. Oyunlarda görünmez.", "A \"Vault\" button appears on the sign-in screen of apps like Steam or Discord; picking an item types the login. Never shown over games."),
        c.app_fill,
        |c| c.vault.app_fill ^= true,
    );
    f.slider(
        p,
        t!("Kopyalananı panodan sil", "Clear copied value"),
        t!("Kasadan kopyalanan alan bu süre sonra panodan silinir.", "A field copied from the vault is cleared from the clipboard after this time."),
        c.clear_clipboard_seconds as f32,
        0.0,
        120.0,
        5.0,
        |v| if v < 1.0 { t!("Silinmez", "Never").into() } else { t!(format!("{v:.0} sn"), format!("{v:.0} s")) },
        |c, v| c.vault.clear_clipboard_seconds = v as u64,
    );
    f.toggle(p, t!("Ekran görüntüsünde gizle", "Hide from screenshots"), t!("Kasa penceresi ekran görüntülerinde ve ekran paylaşımında görünmez.", "The vault window doesn't appear in screenshots or screen sharing."), c.hide_from_capture, |c| c.vault.hide_from_capture ^= true);
    if hello_available(app) {
        f.toggle(
            p,
            t!("Windows Hello ile açma", "Unlock with Windows Hello"),
            t!("Kasanın Güvenlik sayfasında etkinleştirilince kasa yüz, parmak izi ya da PIN ile açılır. Kapatılırsa düğme gizlenir; ana parola her zaman çalışır.", "Once turned on in the vault's Security page, the vault opens with your face, fingerprint or PIN. Turned off, the button is hidden; the master password always works."),
            c.windows_hello,
            |c| c.vault.windows_hello ^= true,
        );
    }
    f.flush(p);
    f.y += 6.0;
    f.note(p, t!("Kasa yalnızca bu bilgisayarda, şifreli durur. Ana parola ve kurtarma anahtarı kasanın Güvenlik sayfasından değişir.", "The vault is stored encrypted only on this computer. Change the master password and recovery key on the vault's Security page."));
}
