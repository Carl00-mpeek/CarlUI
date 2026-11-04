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
//! Aşama 4, QR (`carlui-island` `shots/qr.rs`): ekran görüntüsü seçimindeki QR
//! türüne ayrılır; 2FA ise "Carl Vault" düğmesi otpauth adresini yerel boruyla
//! (`Request::ImportTotp`) yollar, kasa doldurulmuş yeni kayıt açar (kilitliyse
//! açılınca). Kayıt kendiliğinden kaydedilmez.
//!
//! Aşama 5, sağlamlaştırma: kurcalanmış dosya (kayıt kimliği, KDF ayarları,
//! sürüm, kesik dosya) reddedilir (`vault` ve `crypto` testleri); yanlış parola
//! beklemesi diske yazılır, pencereyi kapatmak sıfırlamaz (`throttle`). Elle
//! bellek dökümü: kilitliyken ana parola ve kayıt değerleri bulunmadı.
//!
//! Kart alanları (`card`): rakam dışı yazılamaz, numara gruplanır, son kullanma
//! "AA/YY"; hızlı kopyalama siteye göre parçaları (ay, yıl, AA/YYYY …) verir.
//!
//! İçe aktarma (`import`): başka şifre yöneticilerinin ve 2FA uygulamalarının
//! şifresiz dışa aktarımları (CSV / JSON / otpauth listesi) ve Google
//! Authenticator aktarım QR'ı önizlenip tek seferde kasaya yazılır.
//!
//! Parola üretici (`gen`): işletim sistemi rastgeleliğiyle, her türden en az
//! bir karakter; düzenleyicide zar düğmesi ve güç göstergesi.
//!
//! Yeni girişi kaydetme (`autofill::classify`, pencere tarafı `app/capture.rs`):
//! eklenti gönderilen formu sorar; kasada yoksa ya da parola değişmişse sayfa
//! "kaydedilsin mi?" der, kullanıcı onaylarsa kayıt eklenir/güncellenir.
//!
//! Passkey (`passkey`, pencere tarafı `app/passkeys.rs`): eklenti sitenin
//! WebAuthn isteğini yakalar; kasa onay kartını gösterir, ES256 anahtarını
//! kaydın şifreli gövdesinde saklar ve girişte imzalar.

pub mod autofill;
pub mod card;
pub mod gen;
pub mod crypto;
pub mod entry;
mod hex;
pub mod import;
pub mod passkey;
pub mod recovery;
pub mod throttle;
pub mod token;
pub mod totp;
pub mod vault;
