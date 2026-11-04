//! Carl Vault: yerel, şifreli kişisel parola ve gizli bilgi kasası.
//!
//! Bu crate'in kütüphane kısmı arayüzden bağımsız kasa çekirdeğidir (şifreleme,
//! dosya biçimi, kayıtlar); pencere `main.rs` ve `ui/` altındadır. Sunucu yoktur;
//! her şey `%APPDATA%\CarlUI\vault\` altında, bu bilgisayarda kalır. Ana parola
//! hiçbir yere gönderilmez ve saklanmaz.
//!
//! Güvenlik sınırı: kasa kilitliyken diskteki dosyayı ele geçiren biri parolayı
//! tahmin etmeden içeriği okuyamaz. Kasa açıkken bilgisayarı yöneten kötü
//! amaçlı yazılıma karşı mutlak koruma vaat edilmez; bu yüzden kayıtlar
//! yalnızca istenince çözülür, bellek temizlenir ve kasa kendiliğinden kilitlenir.
//!
//! Aşama 2, pano aktarımı (`token`, `carlui_core::vaultlink`): bir alan
//! kopyalanınca panoya gerçek değer yerine kayıt+alan başına tek kullanımlık,
//! 60 sn ömürlü rastgele bir belirteç konur. CarlUI panosu (Win+V) onu geçmişe
//! yazmadan gösterir; seçilince ada yerel borudan isteği yollar, kasa istekçiyi
//! ve hedef pencereyi doğrulayıp değeri o pencereye klavye girdisi olarak yazar.
//!
//! Aşama 3, 2FA kodu (`totp`): `FieldKind::Totp` alanındaki base32 anahtar ya
//! da otpauth adresinden RFC 6238 kodu; yalnızca kayıt açıkken üretilir.
//!
//! Sonraki aşamalar (henüz yazılmadı):
//! - Aşama 4, ekran görüntüsü aracında QR algılama → 2FA kaydı olarak aktarım.
//! - Aşama 5, güvenlik testleri (bellek dökümü, dosya kurcalama, hız sınırı).

pub mod autofill;
pub mod crypto;
pub mod entry;
mod hex;
pub mod recovery;
pub mod token;
pub mod totp;
pub mod vault;
