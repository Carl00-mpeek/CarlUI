# Yapılacaklar (Carl Vault ve çevresi)

Son durum: 2026-11-04. Aşama 1–5 bitti ve main'e push edildi.

## Aşama 5 sonucu

- Dosya kurcalama: kayıt kimliği, KDF ayarları (ör. 4 GiB bellek), sürüm ve kesik dosya reddediliyor; çökme yok.
- Hız sınırı: 3. hatadan sonra 5, 10, 20 … sn (en çok 5 dk). Sayaç `unlock.json`'da, yeniden başlatınca sürüyor.
- Bellek dökümü: kilitliyken ve düzenleyici kapalıyken ana parola, parola, e-posta ve not bulunmadı.
  "GitHub" bulguları programdaki örnek metin. Düzenleyici açıkken değerler bellekte (beklenen).

- Sayfa içi "Carl Vault" düğmesi (eklenti 1.2.0) eklendi; Helium'da kilitli ve açık kasayla denendi.
  Aynı siteye birden çok kayıt varken açılan liste canlı denenmedi.

## İçe aktarma ve kart (2026-11-04)

- Güvenlik > İçe aktar: Bitwarden JSON/CSV, Chrome/Edge/Brave, Firefox, 1Password, LastPass, KeePass,
  Proton Pass, Dashlane, NordPass CSV; 2FA için Aegis, 2FAS, andOTP (şifresiz), otpauth listesi.
  Google Authenticator aktarım QR'ı ekran görüntüsü aracından okunur. Önce önizleme, yinelenen eklenmez.
- Şifreli dışa aktarımlar (Bitwarden, Aegis, 2FAS) tanınıp "şifresiz dışa aktar" diye uyarılıyor; çözülmüyor.
- Kart alanları yalnız rakam alır, numarayı 4'erli (Amex 4-6-5), son kullanmayı AA/YY yazar.
  Düzenleyicide kart türü, numara hatası (Luhn), süresi geçmiş uyarısı ve "Hızlı kopyala" (ay, yıl, AA/YYYY…).
- Birim ve uçtan uca testler geçti. Pencere canlı (ekran görüntüsüyle) denenmedi: kasa görüntüde gizli.

## Bekleyenler

- **Firefox:** eklenti 1.2.0 zip'inin AMO'da yeniden imzalanması gerekiyor (kullanıcı yapacak).
- **Helium:** Ctrl+Shift+L kısayolu atanmış ama yapay tuşla açılmadı; elle denenecek.
- **Tarayıcıda doldurma:** kayıtta 2FA alanı varsa giriş sonrası kod alanını da doldurmak (isteğe bağlı).
- **Kayıtta yakalama:** tarayıcıda kayıt/giriş formu gönderilince kullanıcı adı ve parolayı yakalayıp
  "kaydedilsin mi?" diye sormak (sıradaki iş).
- **Passkey:** Windows 11 üçüncü taraf passkey sağlayıcı (eklenti kimlik doğrulayıcı) olarak kaydolmak.
  MSIX paketi + COM sunucu gerekir; büyük iş, kullanıcıyla konuşuldu.
- **Ayrı işler:** Win menüsü ya da kurucu (bkz. hafıza notu "Modül 5-8 durumu").

## Deneme notları

- Gerçek kasaya dokunma. `CARLUI_VAULT_DIR` ile scratch kasası aç.
- QR aktarımı kasayı kendi başlatır. Önce deneme kasasını aç, yoksa gerçek kasa açılır.
- Kasa ekran görüntüsünde gizli. Görüntü almak için kabuğun sanal `config.toml` kopyasına geçici olarak
  `[vault] hide_from_capture = false` ekle, iş bitince sil.
