//! Carl Vault: otomatik kilit süresi, uyku/oturum kilidinde kilitleme, panodan
//! silme süresi, pano belirteci ve ekran yakalamadan gizleme. Kısayol Kısayollar
//! sayfasında.

use crate::{
    ui::{Flow, P},
    App,
};

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
        t!("Kopyalanan alan Win+V'de kasa kaydı olarak görünür; seçince hedef pencereye kasa yazar. Gerçek değer panoya girmez, 60 sn sonra geçersiz olur.", "The copied field shows up in Win+V as a vault item; picking it makes the vault type it into the target window. The real value never enters the clipboard and expires after 60 s."),
        c.token_paste,
        |c| c.vault.token_paste ^= true,
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
    f.y += 6.0;
    f.note(p, t!("Kasa yalnızca bu bilgisayarda, şifreli durur. Ana parola ve kurtarma anahtarı kasanın Güvenlik sayfasından değişir.", "The vault is stored encrypted only on this computer. Change the master password and recovery key on the vault's Security page."));
}
