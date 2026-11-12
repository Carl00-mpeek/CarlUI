## Türkçe

**CarlUI 1.0.0: ilk kararlı sürüm.**

**Yeni: Hayvan tepkileri**
- Dock'taki hayvan müzik çalarken müzik notalarıyla dans eder, bildirim gelince kulak kabartır, 3 dakika boş kalınca uyur, döndüğünde gerinir, üzerine tıklayınca (sevince) kalpler çıkarır.
- Ayarlar → Dock → "Hayvan tepkileri". Varsayılan olarak açıktır.

**Güvenlik**
- CarlUI modülleri arasındaki iç haberleşme kısıtlandı: borular yalnızca sahibine açık; iki taraf da karşısındakinin aynı kurulum klasöründen, aynı kullanıcı ve oturumdaki bir CarlUI programı olduğunu doğrular. Aşırı uzun mesajlar atılır.

**Genişletilmiş yedek**
- Ayarlar → Genel → Yedek artık `CarlUI-yedek-YYYY-MM-DD` klasörü dışa aktarır: ayarlar, kayıtlı görünümler, pencere düzenleri ve özel hayvanlar. İçe aktarma bunları geri yükler, başarısız olan bölüm varsa bildirir.
- Kasa ve pano geçmişi bilerek yedeğe dahil değildir. Eski tek `.toml` yedekleri hâlâ içe aktarılır.

**Düzeltmeler**
- Pencere düzenleri, monitör değişince bir pencereyi ekran dışına geri koymuyor.
- Bozuk düzen dosyası kaybolmak yerine `layouts.json.bad` olarak saklanır.
- Tam Kaldırıcı, Mağaza uygulaması kaldırması gerçekten başarısız olduğunda bunu bildirir.
- Kaldırıcı listesindeki uzun adlar güvenle kısaltılır.

---

## English

**CarlUI 1.0.0: the first stable release.**

**New: Pet reactions**
- The dock pet dances with music notes while music plays, perks up when a notification arrives, falls asleep after 3 minutes idle and stretches when you return, and shows hearts when you click (pet) it.
- Settings → Dock → "Pet reactions" (Hayvan tepkileri). On by default.

**Security**
- The internal messaging between CarlUI modules is now restricted: pipes are owner-only, and both sides verify that the peer is a CarlUI program from the same install folder, same user and session. Over-long messages are dropped.

**Extended backup**
- Settings → General → Backup now exports a `CarlUI-yedek-YYYY-MM-DD` folder with settings, saved looks, window layouts and custom pets. Import restores them and reports any part that failed.
- The vault and clipboard history are deliberately not included. Old single `.toml` backups still import.

**Fixes**
- Window layouts no longer restore a window off-screen after a monitor change.
- A corrupt layouts file is kept as `layouts.json.bad` instead of being lost.
- The Full Uninstaller now reports when a Store app removal actually failed.
- Long names in the uninstaller list are shortened safely.
