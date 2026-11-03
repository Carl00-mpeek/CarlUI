# CarlUI İndirmeler eklentisi (isteğe bağlı)

Eklenti kurulmazsa CarlUI yine çalışır. Bu durumda tarayıcı indirmeleri
İndirilenler klasörü izlenerek adada görünür; yüzde gösterilmez, yalnızca
boyut görünür.

## Ne yapar?
- **İndirme yöneticisi açıkken:** Tarayıcıda bir indirme başlayınca indirme
  duraklatılır. Ada aşağı doğru "İndirme bilgisi" kartıyla açılır ve dosya adı,
  boyutu, kaynağı ve kayıt yeri görünür. Üç seçenek vardır:
  - **İndir:** CarlUI indirir; yüzde ve hız adada görünür.
  - **Farklı kaydet…:** Önce kayıt yeri seçilir.
  - **Tarayıcıya bırak:** Tarayıcı kaldığı yerden devam eder.
- **İndirme yöneticisi kapalıyken** (Ayarlar → Arama): Eklenti hiçbir indirmeye
  karışmaz. Yalnızca tarayıcının kendi indirmelerinin yüzdesini ve hızını adaya
  bildirir.
- **CarlUI çalışmıyorken:** Hiçbir şey yapmaz.

Eklenti yalnızca bu bilgisayardaki `127.0.0.1:47321` adresiyle konuşur.
Oturum isteyen indirmeler (ör. Google Drive) CarlUI'dan da inebilsin diye o
sitenin çerezleri yalnızca bu yerel adrese, yalnızca o indirme için gönderilir.

## Kurulum
**Chrome / Edge / Brave / Opera**
1. Adres çubuğuna `chrome://extensions` yazın (Edge'de `edge://extensions`).
2. **Geliştirici modu**nu açın.
3. **Paketlenmemiş öğe yükle** düğmesine basın ve bu klasörü seçin:
   `extension\carlui-downloads`

**Firefox**
1. `about:debugging#/runtime/this-firefox` adresini açın.
2. **Geçici Eklenti Yükle** düğmesine basın ve bu klasördeki `manifest.json`
   dosyasını seçin.
3. Firefox kapanınca bu yükleme silinir. Kalıcı kurulum için eklentinin
   imzalanması gerekir (aşağıda).

**Firefox — kalıcı kurulum (imzalı)**
1. https://addons.mozilla.org/developers/ adresinde Mozilla hesabıyla oturum açın.
2. **Yeni Eklenti Gönder** → dağıtım olarak **Kendim (On your own)** seçin.
   Eklenti mağazada listelenmez; yalnızca imzalanır.
3. `extension\carlui-downloads-1.0.0.zip` dosyasını yükleyin. Otomatik
   denetimden geçince imzalı `.xpi` birkaç dakika içinde indirilebilir.
4. Firefox'ta `about:addons` → dişli → **Dosyadan eklenti yükle…** ile `.xpi`
   dosyasını seçin. Bu kurulum kalıcıdır.
