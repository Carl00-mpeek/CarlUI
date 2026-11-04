# Yapılacaklar (Carl Vault ve çevresi)

Son durum: 2026-11-04. Aşama 1–5 bitti ve main'e push edildi.

## Aşama 5 sonucu

- Dosya kurcalama: kayıt kimliği, KDF ayarları (ör. 4 GiB bellek), sürüm ve kesik dosya reddediliyor; çökme yok.
- Hız sınırı: 3. hatadan sonra 5, 10, 20 … sn (en çok 5 dk). Sayaç `unlock.json`'da, yeniden başlatınca sürüyor.
- Bellek dökümü: kilitliyken ve düzenleyici kapalıyken ana parola, parola, e-posta ve not bulunmadı.
  "GitHub" bulguları programdaki örnek metin. Düzenleyici açıkken değerler bellekte (beklenen).

## Bekleyenler

- **Firefox:** eklenti 1.1.0 zip'inin AMO'da yeniden imzalanması gerekiyor (kullanıcı yapacak).
- **Helium:** Ctrl+Shift+L kısayolu atanmış ama yapay tuşla açılmadı; elle denenecek.
- **Tarayıcıda doldurma:** kayıtta 2FA alanı varsa giriş sonrası kod alanını da doldurmak (isteğe bağlı).
- **Ayrı işler:** Win menüsü ya da kurucu (bkz. hafıza notu "Modül 5-8 durumu").

## Deneme notları

- Gerçek kasaya dokunma. `CARLUI_VAULT_DIR` ile scratch kasası aç.
- QR aktarımı kasayı kendi başlatır. Önce deneme kasasını aç, yoksa gerçek kasa açılır.
- Kasa ekran görüntüsünde gizli. Görüntü almak için kabuğun sanal `config.toml` kopyasına geçici olarak
  `[vault] hide_from_capture = false` ekle, iş bitince sil.
