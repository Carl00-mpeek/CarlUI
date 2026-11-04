# CarlUI tarayıcı eklentisi (isteğe bağlı)

Tek eklenti iki iş yapar: tarayıcı indirmelerini CarlUI adasına gönderir ve
giriş sayfalarını Carl Vault kasasından doldurur.

Eklenti kurulmazsa CarlUI yine çalışır. Bu durumda tarayıcı indirmeleri
İndirilenler klasörü izlenerek adada görünür; yüzde gösterilmez, yalnızca
boyut görünür.

## Carl Vault ile doldurma
- Giriş sayfasında eklentinin düğmesine basın (ya da **Ctrl+Shift+L**; tuşu
  tarayıcının eklenti kısayolları sayfasından değiştirebilirsiniz).
- Açılan listede bu siteye ait kayıtlar görünür (yalnızca ad ve kullanıcı
  adı). Birini seçince kullanıcı adı ve parola sayfaya yazılır.
- Bir kaydın listede görünmesi için kasadaki kayıtta bu sitenin adresi
  (ör. `https://github.com`) bir **Adres** alanında olmalıdır.
- Eşleme sıkıdır: `github.com` kaydı `gist.github.com`'a da uyar, ama
  `githuh.com` ya da `github.com.baska-site.com` gibi benzer adreslere asla
  uymaz. https kaydı http sayfaya verilmez.
- Kasa yalnızca açıkken ve tarayıcı öndeyken yanıt verir; kasa kilitliyse
  eklenti "Carl Vault'u aç" düğmesi gösterir. Eklenti parolayı saklamaz.
- **Sayfa içi düğme:** Giriş kutusunun sağında mor bir **Carl Vault** düğmesi
  çıkar. Kasa açıkken bu düğme yalnızca kasada kaydı olan sitelerde görünür.
  Kasa kilitliyken hangi sitede kayıt olduğu bilinemez; düğme her giriş
  sayfasında çıkar.
  - Kasa açıksa: basınca bilgiler yazılır. Siteye ait birden çok kayıt varsa
    önce bir liste açılır.
  - Kasa kilitli ya da kapalıysa: Carl Vault penceresi öne gelir. Ana parolayı
    orada yazın; sayfaya yazmayın, çünkü sayfa kendi kutularına basılan tuşları
    okuyabilir. Kilit açılınca tarayıcıya dönülür ve bilgiler kendiliğinden
    yazılır (2 dk bekler).
- **Yeni girişi kaydetme (1.3.0):** Bir siteye kayıt olunca ya da kasada
  olmayan bir hesapla giriş yapınca sağ üstte "Carl Vault'a kaydedilsin mi?"
  çıkar. **Kaydet**'e basmadan hiçbir şey kaydedilmez. Parola değişmişse
  "Güncelle" sorulur. Kasada aynısı varsa hiçbir şey sorulmaz. Kasa kilitliyse
  yalnızca kayıt formlarında sorulur, "Aç ve kaydet" ile kilit açılınca yazılır.
  Şeritte kaydın kasada hangi adla görüneceği sorulur (önerilen: site adı, ör.
  "Roblox"); istediğin adı yazabilirsin. Kullanıcı adı ve parola beklerken yalnızca eklentinin belleğinde durur (en
  çok 3 dk), sayfaya geri verilmez.
- **Güçlü parola önerisi:** Kayıt ya da parola değiştirme formunda yeni parola
  kutusuna tıklayınca altında bir öneri çıkar (20 karakter; site sınır koyduysa
  ona uyar). ↻ başka öneri verir, "Sembol" kapatılabilir, **Kullan** parola ve
  "tekrar" kutularına yazar. Parola bu bilgisayarda üretilir.
- Ayarlar → Carl Vault → "Tarayıcıda doldurma" ile tamamen kapatılabilir
  (sayfa içi düğme ve kaydetme sorusu da kaybolur).
- Eklenti güncellenince Chromium tabanlı tarayıcılarda (Chrome, Edge, Helium)
  uzantılar sayfasında CarlUI kartındaki ↻ (yeniden yükle) düğmesine basın.
- Not: Bilgisayarda kötü amaçlı bir program çalışıyorsa, kasa açıkken hiçbir
  parola yöneticisi tam koruma sağlayamaz.

## İndirmeler: ne yapar?
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
3. `extension\carlui-downloads-1.1.0.zip` dosyasını yükleyin. Otomatik
   denetimden geçince imzalı `.xpi` birkaç dakika içinde indirilebilir.
4. Firefox'ta `about:addons` → dişli → **Dosyadan eklenti yükle…** ile `.xpi`
   dosyasını seçin. Bu kurulum kalıcıdır.
