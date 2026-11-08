# Sürüm notları

## v0.9.4 — Akıllı asistan modeli düzeltmesi (2026-11-08)

**Düzeltme: Akıllı asistan modeli kurulu görünmüyordu.** Qwen3 4B indirilip "hazır"
bildirimi çıktığı halde Ayarlar yine "İndir" gösteriyor, asistan modeli kullanmıyordu.
İndirilen llama.cpp paketi açıldıktan sonra silinir; kurulum denetimi ise paketin
klasörde durmasını bekliyordu. Artık açılan paketin yerine SHA-256 damgası kalır ve
denetim ona bakar. 0.9.3 ile indirmiş olanlar yalnızca 37 MB'lık paketi bir kez daha
indirir; 2,5 GB'lık model yeniden inmez.

## v0.9.3 — Akıllı asistan, anlık tekrar kaldırıldı (2026-11-08)

**Kaldırıldı: Anlık tekrar ve sesli komutla klip.** Açıkken ekranı ve sesi durmadan
kaydediyor, sesli komut için mikrofonu sürekli dinliyordu. Hiçbir şey kaydedilmezken
bile yaklaşık %26 işlemci ve 1,7 GB bellek tuttu. Bu, hafif kalma sözüne aykırı.
`carlui-replay` artık arka planda çalışmaz. Eski geçici video parçaları ve Vosk modeli
ilk açılışta silinir. Kaydedilmiş klipler `Videolar\CarlUI`'da kalır. Alan kaydı
(Ctrl+R, Ctrl+Alt+R) yerinde.

**Yeni: Asistan uzun cümleleri anlar.** Bilgisayarında çalışan küçük bir dil modeli
(Qwen3 4B) söylediğini komuta çevirir:
- "Discord ve Steam'i 1. ekranda bölerek ayarla": iki pencere o ekranda yan yana dizilir.
- "Spotify'ın sesini 20 yap": yalnızca o uygulamanın sesi değişir.
- "Spotify'da lofi aç": şarkı Spotify'da çalar.
- Tek cümlede birden fazla komut da olur: "Not Defteri'ni aç ve sesi 30 yap".

Model isteğe bağlıdır (2,5 GB), **Ayarlar → Yapay zekâ → Akıllı asistan** bölümünden
indirilip açılır. Arka planda hiçbir model beklemez: Whisper yalnızca kısayol basılıyken
çalışır, yazıya dökünce kapanır; dil modeli de yalnızca kuralların anlamadığı cümlede
kullanılır, iş bitince kapanır. İşlemci ya da ekran kartı (yanıt ~0,1 sn) seçilebilir;
tam ekran oyunda, izin verilmedikçe model açılmaz (Ayarlar → Oyundayken de kullan). Kapalıyken yalnızca Whisper ve hazır komutlar çalışır.
Kısa komutlar ("YouTube'u aç", "sesi 30 yap") eskisi gibi modele gitmeden anında çalışır.

**Yeni: Fare tuşu kısayolları.** Her kısayol orta, geri ya da ileri fare tuşu olabilir.
Sol tık kısayol yapılamaz.

**Yeni: Dock önizlemesinde müzik düğmeleri.** Şarkı çalan uygulamanın önizlemesinde
önceki / oynat / sonraki düğmeleri çıkar.

**Yeni: Hızlı panelde Ethernet.** Kabloyla bağlıyken Wi-Fi kutucuğu "Ethernet" olarak
yanar (Wi-Fi kartı olmayan bilgisayarda boş kalmaz). Tıklayınca Wi-Fi kartı varsa Wi-Fi
açılır ve kablosuz ağ listesi gelir; yoksa ağ ayarları açılır.

**Düzeltmeler**
- **Model indirme:** Sunucu yanıt vermeyi kesince indirme %0'da takılı kalıyordu; artık
  kesilip yeniden denenir. Önceden inmiş dosyalar yeniden indirilmez.
- **Görev çubuğu:** CarlUI görev yöneticisinden sonlandırılınca ya da çökünce Windows'un
  görev çubuğu geri gelmiyordu.
- **Asistan:** "Parlaklığı yüzde elli yap" anlaşılmıyordu.

**SHA-256 (CarlUI-Setup.exe):**
`0b235f231a8ed19d57aa900077cb33733624feface87b24e50199180e93a7a02`

## v0.9.2 — Tam ekranda şarkı ve ses göstergesi (2026-11-07)

**Yeni: Ada tam ekranda da görünür.** Şarkı değişince ya da ses açılıp kısılınca ada
kısa süre belirir. Odak almaz, tıklamalar oyuna gider. **Ayarlar → Oyun modu** bölümündeki
**Tam ekranda şarkı ve sesi göster** ile kapatılabilir. Özel tam ekran oyunlarda
görünmeyebilir.

**Düzeltmeler**
- **Ses göstergesi:** Ayarlardaki seçenek hiçbir şey yapmıyordu, artık masaüstünde de
  çalışır.

**SHA-256 (CarlUI-Setup.exe):**
`17a67044bea401ab0ad6277911e1e1fd1d9a80d8c28a25a67a43d18a5c8be277`

## v0.9.1 — Arama, sıcaklık ve kayıt düzeltmeleri (2026-11-06)

**Yeni: CPU sıcaklığı Ayarlar'dan kurulur.** PawnIO'yu CarlUI'den sonra kurduysan
**Ayarlar → Ada → Sistem göstergesi** bölümündeki **Kur** düğmesi yeter. Bir kez
yönetici izni ister. Şimdilik yalnızca Intel işlemcilerde çalışır.

**Düzeltmeler**
- **Arama:** HiBit Uninstaller gibi kaldırma programları aramada çıkmıyordu.
- **Office:** "office" yazınca Word, Excel ve diğer Office uygulamaları da çıkar.
- **İndirme göstergesi:** Discord'da yayın izlemek indirme sanılıyordu. Teams, Zoom,
  Slack ve Spotify için de aynı düzeltme yapıldı.
- **Anlık tekrar:** Oyundan çıkınca çözünürlük değişirse kayıt sıfırlanıyor, klip
  1-2 saniye kalıyordu. Artık oyun kaydı korunur. Farklı çözünürlükteki bölümler
  kenarlarında siyah boşlukla kaydedilir.

**SHA-256 (CarlUI-Setup.exe):**
`41a33616666a12a6348eee42fec670f6d285695b764751a22e39d155ccecfed1`

## v0.9.0 — Anında yanıt veren asistan (2026-11-06)

**Asistan artık bekletmiyor.** Model bellekte hazır durur, tuşu bırakınca yazı yarım
saniye içinde gelir (önceden 5-6 saniyeydi). Canlı yazı da ilk saniyeden çalışır.
- **Bellek:** Whisper small ile yaklaşık 1,6 GB RAM ve 1,9 GB ekran kartı belleği
  tutar. Daha küçük model daha az yer kaplar.
- **Bırakma:** Oyun öne gelince ya da asistan kapatılınca model bellekten çıkar.
  Asistan kapanınca bellek bir saniye içinde boşalır.

**Yeni: Sitede sesli arama.** "YouTube'da X'i aç" ve "YouTube'u aç ve X'i arat"
artık YouTube'da arar.

**Düzeltmeler**
- **Sessizlik:** Konuşma duyulmazsa kart uyarı göstermeden kapanır.
- **Onay ekranı:** Konuşmayı kısayolla durdurunca onay ekranı hemen kapanıyordu.
- **Google Chrome:** "Google Chrome'u aç" Google'da arama yapıyordu, artık Chrome'u
  açar. "Google'ı aç" da google.com'u açar.

**SHA-256 (CarlUI-Setup.exe):**
`230d289a55d5ea2d43047a796a1f2a4f4e98066e5840a51a4e7ab14548930828`

## v0.8.0 — Sesli asistan aramada (2026-11-06)

**Asistan artık adanın aramasında açılır.** Tetikleyince arama açılır, altta öneriler
yerine sesinle büyüyen animasyon çıkar. Konuştukların üstteki arama kutusuna canlı
yazılır.
- **Yedek:** Oyundayken, ada kapalıyken ya da model inerken eski kart gösterilir.

**Yeni: Uygulamadan önce onayla.** Ayarlar'ın AI sayfasında seçilir.
- **Kapalıysa:** Tuşu bırakınca metin doğrudan uygulanır.
- **Açıksa:** Metin kutuda bekler. Düzenle ile değiştirebilir, ✓ ya da Enter ile
  gönderebilir, ✕ ya da Esc ile iptal edebilirsin.

**Düzeltme: Model silme.** Asistan listesindeki Whisper modellerinde Sil düğmesi
çalışmıyordu. Bu dosyalar dikteyle ortak olduğu için model ikisinden de kalkar.

**SHA-256 (CarlUI-Setup.exe):**
`dfaadef75234c0a7d1de7895bc8304d87413ceb23056de981e4001ba6f09b931`

## v0.7.0 — İndirme yöneticisi (2026-11-06)

**İndirmeler artık onaysız başlamaz.** Tarayıcıdaki indirme hemen kesilir ve adada
bir kart açılır. Kart sen karar verene dek bekler.
- **Seçenekler:** İndir, Farklı kaydet…, Tarayıcıya bırak ve İptal.
- **İptal:** Dosya inmez, tarayıcıda da yarım dosya kalmaz.
- **Tam ekran:** Oyun ya da tam ekran varken indirme kendiliğinden başlamaz, kart
  sonra açılır.
- **Eklenti:** Güncellemeden sonra tarayıcının eklentiler sayfasında CarlUI
  eklentisinin ↻ düğmesine bas. Tarayıcıya bırakılan indirme adresten yeniden
  başlar; tek kullanımlık bağlantılarda sayfadan yeniden başlatman gerekebilir.

**Yeni: Tam Kaldırıcı.** Uygulamayı kaldırır, geride kalan klasör ve kayıt
anahtarlarını tarar. Dosyalar Geri Dönüşüm Kutusu'na gider, anahtarlar önce
yedeklenir. Yeni kurulanlar listenin üstünde görünür. Win menüsünde varsayılan
kısayolu var.

**Yeni: Dosya kilidi açıcı.** Bir dosyayı tutan uygulamayı gösterir ve bıraktırır.
Tam Kaldırıcı'nın silemediği öğelerde de sunulur.

**Yeni: Alan kaydı.** Ekran görüntüsü aracında seçilen alan sesiyle MP4'e kaydedilir,
sonuç GIF'e çevrilebilir.

**Yeni: Pencere düzenleri.** Pencere yerleri ekran değişince ve uykudan sonra geri
konur. Düzenler adla kaydedilip uygulanır.

**SHA-256 (CarlUI-Setup.exe):**
`bb0b3d152b625def4f44a3b75113987d14031a77fbcedfb03af9953df951cbf5`

## v0.6.2 — Açık tema (2026-11-06)

**Açık tema baştan yapıldı.** Gri yüzeyler yerine beyaz cam geldi; bu görünüm
pencere odağı kaybedince de korunuyor. Koyu tema değişmedi.
- **Yazılar:** Açık temadaki yazılar ve vurgu renkleri artık AA kontrast düzeyinde
  okunuyor.
- **Pencereler:** Ayarlar, Görev Yöneticisi, Kasa, Dönüştürücü, Ofis ve kurucu aynı
  beyaz camı kullanıyor. Başlık çubukları da pencereyle aynı renkte.
- **Belgeler:** Tablo, belge ve PDF sayfaları her iki temada da gerçek renkleriyle
  beyaz kâğıt olarak çiziliyor.
- **Renk seçenekleri:** Renk örnekleri temaya göre değişmiyor. Kaydırıcı topuzları
  beyaz zeminde de seçiliyor.

**Düzeltme:** Tabloya sondaki `=` ile yazılan işlemler de formül sayılıyor. Örneğin
`a1+b5=` iki hücrenin toplamını verir. `Toplam=` gibi etiketler metin olarak kalır.

**SHA-256 (CarlUI-Setup.exe):**
`b6654f33d44b0aa0aa1d18d4d0c3100ee9ecbdffa0cf1692162f941e0a6fafc5`

## v0.6.1 — Performans (2026-11-06)

**Menüler artık donmadan açılıyor.** Özellikle zayıf bilgisayarlarda ve ağ ya da
harici disk takılıyken görülen gecikmeler giderildi.
- **Dock sağ tık menüsü:** Uygulama ve jump list ikonları önbellekte tutulur.
  Jump list her açılışta yeniden okunmaz.
- **Klasör yığınları:** Ağ sürücüsü, USB ve CD'deki dosyaların ikonu çıkarılırken
  menü artık beklemez.
- **Çöp kutusu:** Doluluk arka planda izlenir. Dock artık 3 saniyede bir
  takılmaz.
- **Win menüsü:** İlk açılış da hızlı, çünkü ikonlar gizliyken önceden yüklenir.
  Kaydırırken ikonlar kademeli gelir. Uygulama listesi her karede değil, yalnızca
  değişince yeniden kurulur.

**Düzeltme:** Açık bir uygulamayı dock'a sabitleyince artık başlatıcısı kaydedilir
(Windows görev çubuğunun yaptığı gibi). Örneğin Steam "Steam WebHelper" olarak
kaydediliyor ve sabitlenen ikondan açılmıyordu. Eski hatalı sabitlemeyi bir kez
kaldırıp yeniden sabitleyin.

**Yeni: Sesli asistan.** **Ctrl+Alt+A** basılı tutulur, komut söylenir, tuş
bırakılır. Örneğin "YouTube'u aç", "YouTube'da lofi ara", "sesi 30 yap",
"Spotify'ı kapat". Kısayolun kendisi ayarlanabilir. Kurucuda ve Ayarlar →
Yapay zekâ'da açılır (varsayılan kapalı); modeli ilk kullanımda iner.

**Diğer:** Ayarlar'da sekme değişince anahtarlar kaymıyor, uzun yazılar
kesilmeden alt satıra iniyor. Cam menülerde uzun satırlar da alt satıra iniyor.
Görev yöneticisi fare hareketinde boşuna çizim yapmıyor.

**SHA-256 (CarlUI-Setup.exe):**
`523d2af31cad43005974cce7133fdfe23adf98ab2f7236d9351d370035d4a408`

## v0.2.0 — Carl Vault (2026-11-04)

**Yeni: Carl Vault**, CarlUI'nin şifreli parola ve 2FA kasası.
- Kasa yerelde, ana parolayla Argon2id + XChaCha20-Poly1305 ile şifrelenir;
  hiçbir şey buluta gitmez. Kurcalamaya ve parola denemesine karşı korumalıdır.
- Parola, e-posta, kart, not ve 2FA kayıtları. 2FA kodları (TOTP) kasada üretilir.
- **Tarayıcı eklentisi (1.3.0):** Giriş sayfasını doldurur. Yeni girişte ya da
  parola değişince sorup kaydeder; kayıt adını da sorar. Kayıt formunda güçlü
  parola önerir. Kasa yalnızca öndeki tarayıcıdan gelen isteği kabul eder.
- **İçe aktarma:** Bitwarden, Chrome, Edge, Brave, Firefox, 1Password, LastPass,
  KeePass, Proton Pass, Dashlane, NordPass; 2FA için Aegis, 2FAS, andOTP ve
  otpauth listeleri. Google Authenticator aktarım QR'ı da okunur. Gmail, Outlook gibi hesaplar "E-posta" kategorisine girer.
- **Güçlü parola üretici** ve güç göstergesi.
- **QR okuyucu:** Ekran görüntüsünde seçilen QR'daki 2FA kodu doğrudan kasaya
  gider. Eğik çekilmiş fotoğraflar ve yoğun kodlar da okunur. Pano geçmişindeki
  resimlerde de eylem listesinde "Carl Vault'a aktar" çıkar.
- Pano: kasa belirteci Win+V'de görünür ve hedef pencereye yazdırılır;
  Win+V açılınca en yeni kayıt seçili gelir.

**Diğer:**
- Tek tıkla güncelleme: adadaki bildirime tıklayınca indirir, özeti doğrular, kurar.
  Güncelleme açık Carl Vault'u da yeniler.
- Sıcaklık ve dolu disk uyarıları.
- İndirmelerde duraklat / sürdür / iptal; biten indirmeye bildirim.
- Ekran görüntüsü bildiriminde Aç / Düzenle / Sil.
- Epic oyun adları; yönetici olarak çalıştır.

**SHA-256 (CarlUI-Setup.exe):**
`84e1d6d9a2bd541db6643322dda0184cbc425290e5da5549ad5582378d8f4029`

## v0.1.0 — ilk sürüm (2026-11-03)

CarlUI'nin herkese açık ilk sürümü.

**Neler var:** yüzen dock, üstte saat/bildirim/medya adası, arama ve Win
menüsü, pano geçmişi (Win+V, tam ekran oyunun üstünde de), kontrol merkezi,
oyun modu, görev yöneticisi, dosya dönüştürücü ve Ayarlar penceresi. Kurucuda
isteğe bağlı CarlUI duvar kâğıdı. Arayüz Türkçe ve İngilizce: Windows'un
görüntü diline uyar, **Ayarlar → Genel → Dil** bölümünden değiştirilebilir.
Yeni sürüm çıkınca adada bildirim gelir (günde bir kez GitHub'a bakılır,
kendiliğinden kurulmaz; **Ayarlar → Genel → Güncellemeler**).

**Kurulum:** `CarlUI-Setup.exe` dosyasını indirip çalıştır. Yönetici izni
gerekmez. Kurucu imzasız olduğu için SmartScreen uyarı verebilir:
**Ek bilgi → Yine de çalıştır**.

**Bilinen durumlar:**
- CPU sıcaklığı için [PawnIO](https://pawnio.eu) sürücüsü gerekir (isteğe bağlı).
- Gerçek "özel tam ekran" (exclusive fullscreen) oyunlarda pencereler oyunun
  üstünde görünemez; oyunu "kenarlıksız pencere" moduna al.

**SHA-256 (CarlUI-Setup.exe):**
`3563d3310a40bc053c981eb4d83b9694dc96d62a26b3439e282ff4ace31e3597`
