# Sürüm notları

## v1.1.6 — Win menüsü ve adada sahne efekti (2026-11-12)

**CarlUI 1.1.6: Win menüsü ve adada sahne efekti.**

**Yeni.** Ayarlar → Ada'da "Adada sahne efekti" (Kapalı / Bildirimde / Her zaman):
dock'taki sahne adada da görünür; "Her zaman" ile saatin ve medya çubuğunun arkasında da.

**Düzeltmeler.** Adadaki sahne efekti artık belirgin. Yönetici penceresi öndeyken Win
tuşunun Windows Başlat'ını açması düzeltildi (oyunlarda karışılmaz). Win menüsünün ilk
kurulumda geç açılması düzeltildi (simgeler arka planda yükleniyor).

**SHA-256 (CarlUI-Setup.exe):**
`94debf570b0542bcb0d6d66f6b86c29ae6138ab51d7413849c4f2f1343d646ea`

## v1.1.5 — Oyun listesi ve donmuş programı zorla kapatma (2026-11-12)

**CarlUI 1.1.5: oyun listesi ve donmuş programı zorla kapatma.**

**Yeni.** Ayarlar → Oyun'da "Oyun sayılan programlar" listesi: listedeki programın
büyütülmüş ya da kenarlıksız penceresi oyun sayılır, kısayollar oyuna bırakılır
(Roblox baştan listede). Dock'ta donmuş programın menüsünde en üstte "Yanıt vermiyor:
zorla kapat" çıkar.

**SHA-256 (CarlUI-Setup.exe):**
`ed4e26136901d871102f7cbe294157d16e094482016a99bb82690cc07dddbfad`

## v1.1.4 — Kenarlıksız oyunda dock tıklaması düzeltmesi (2026-11-12)

**CarlUI 1.1.4: kenarlıksız oyunda dock tıklaması düzeltmesi.**

**Düzeltmeler.** Kenarlıksız tam ekran oyunda (ör. Roblox) Win menüsünü açıp dock'a
sol ya da sağ tıklayınca dock hemen gizleniyor, tıklama ve sağ tık menüsü çalışmıyordu.
Artık dock tıklama ya da menü bitene kadar yerinde kalıyor; oyun dock'tan kapatılabiliyor.

**SHA-256 (CarlUI-Setup.exe):**
`f93fd0e76de21e59e1513da54c0919cd0a6855188749890cf00d83330370198b`

## v1.1.3 — Oyunda kısayol, Win menüsü ve dock düzeltmeleri (2026-11-12)

**CarlUI 1.1.3: oyunda kısayol, Win menüsü ve dock düzeltmeleri.**

**Düzeltmeler.** Özel tam ekran oyunda şarkı değişince oyundan atılma düzeltildi (ada
göstergeden sonra oyunun monitörüne dönmüyor). Kenarlıksız/büyütülmüş pencerede oynanan
oyunlarda (ör. Minecraft) Ctrl+Space gibi kısayollar çalışmıyor. Win tuşunun bazen
menüyü açmaması ve menü açılınca dock'un gelmemesi düzeltildi. Çözünürlük değişince
dock ekran durulunca bir kez kuruluyor, oyun sürerken kurulmuyor.

**SHA-256 (CarlUI-Setup.exe):**
`307e75c26c97db0b4af26c869840cd011d49eba73c6458b827b40ed9b08e1e53`

## v1.1.2 — Oyun sırasında indirme ve odak düzeltmeleri (2026-11-12)

**CarlUI 1.1.2: oyun sırasında indirme ve odak düzeltmeleri.**

**Düzeltmeler.** Oyun oynarken Steam ya da Discord adada sürekli indirme gibi
görünüyordu; Steam artık yalnızca kendi kaydında süren bir indirme varken gösterilir,
oyun öndeyken yalnızca bilinen indirme programları sayılır. Oyun öndeyken CarlUI
pencereleri oyunu arkaya atacak şekilde zorla öne gelmiyor; Win tuşu tam ekranın kısa
düşüşlerinde menüyü oyunun üstünde açmıyor.

**Teşhis.** Odak oyundan başka bir pencereye geçerse günlüğe hangi programın aldığı
yazılır ("odak oyundan gitti").

**SHA-256 (CarlUI-Setup.exe):**
`444313ee4b67d33100106eb29ce4329aaf9bdfe22b69740bc660ff0d23008191`

## v1.1.1 — Kurucu kapanış düzeltmesi (2026-11-12)

**CarlUI 1.1.1: kurucu kapanış düzeltmesi.**

**Düzeltmeler.** Kurulum bittikten sonra `CarlUI-Setup.exe` açık kalıp
silinemeyebiliyordu; kurucu artık düzgün kapanıyor. Kurucunun başlattığı CarlUI artık
kurucunun bulunduğu klasörü (ör. İndirilenler) kilitli tutmuyor. Kaldırma sırasında
geçici kopya kurulum klasörünü kilitleyebiliyordu; düzeltildi.

**SHA-256 (CarlUI-Setup.exe):**
`075a6db708c9cbc1014517619ddefe8a78714f7d1c1a8e4eeccf6fd00543a146`

## v1.1.0 — Güncelleme sayfası, genişletilmiş ilk kurulum turu (2026-11-12)

**CarlUI 1.1.0: Güncelleme sayfası ve yenilenen ilk kurulum turu.**

**Yeni: Güncelleme sayfası.** Ayarlar'a yeni bir "Güncelleme" sayfası eklendi:
CarlUI yazısı, sürüm numarası ve durum bir arada görünür; alttaki "Güncellemeleri
denetle" düğmesiyle yeni sürüm olup olmadığına bakılır. Güncelse "Yazılımınız güncel"
yazar; yeni sürüm varsa "Güncelle" düğmesi ve kısa bir "Yenilikler" kartı çıkar.
"Yeni sürümleri bildir" seçeneği Genel'den bu sayfaya taşındı.

**Yeni: Genişletilmiş ilk kurulum turu.** İlk açılışta tur artık karşılama, görünüm
ve vurgu rengi, dock hayvanı ve sahnesi, dock, ada, modüller ve "Neler yapabilirsin"
özetini kapsar. Her adım atlanabilir; tur Ayarlar → Genel'den yeniden açılabilir.

**SHA-256 (CarlUI-Setup.exe):**
`cc611e4a1a1a5e860de1f0663a3f7e80c5d02ff50a39e39cd58adc7620ce4380`

## v1.0.0 — Hayvan tepkileri, güvenli iç haberleşme, genişletilmiş yedek (2026-11-12)

**CarlUI 1.0.0: ilk kararlı sürüm.**

**Yeni: Hayvan tepkileri.** Dock'taki hayvan müzik çalarken müzik notalarıyla dans
eder, bildirim gelince kulak kabartır, 3 dakika boş kalınca uyur, döndüğünde gerinir,
üzerine tıklayınca (sevince) kalpler çıkarır. Ayarlar → Dock → "Hayvan tepkileri";
varsayılan olarak açık.

**Güvenlik.** CarlUI modülleri arasındaki iç haberleşme kısıtlandı: borular yalnızca
sahibine açık; iki taraf da karşısındakinin aynı kurulum klasöründen, aynı kullanıcı
ve oturumdaki bir CarlUI programı olduğunu doğrular. Aşırı uzun mesajlar atılır.

**Genişletilmiş yedek.** Ayarlar → Genel → Yedek artık `CarlUI-yedek-YYYY-MM-DD`
klasörü dışa aktarır: ayarlar, kayıtlı görünümler, pencere düzenleri ve özel
hayvanlar. İçe aktarma bunları geri yükler, başarısız olan bölüm varsa bildirir. Kasa
ve pano geçmişi bilerek dahil değildir. Eski tek `.toml` yedekleri hâlâ içe aktarılır.

**Düzeltmeler.** Pencere düzenleri monitör değişince bir pencereyi ekran dışına geri
koymuyor. Bozuk düzen dosyası kaybolmak yerine `layouts.json.bad` olarak saklanır. Tam
Kaldırıcı, Mağaza uygulaması kaldırması gerçekten başarısız olduğunda bunu bildirir.
Kaldırıcı listesindeki uzun adlar güvenle kısaltılır.

**SHA-256 (CarlUI-Setup.exe):**
`f378e6bd457a69b0766bd74c6475aaaff45c7e21b990232d1260451c637ddaa4`

## v0.11.0 — Canlı hava, yeni hayvanlar ve renkler, bildirim efektleri (2026-11-12)

**Yeni: Canlı hava.** Dock sahnesi seçtiğin şehrin gerçek havasını izleyebilir:
yağmur, kar, bulut ve gece (ay ve yıldızlar). Bağlantı yoksa seçtiğin sahne
gösterilir. Yoğunluk Hafif / Normal; canlı dock tam ekran oyunda ve pilde durur.
Yalnızca seçtiğin yerin koordinatları (iki ondalık basamağa yuvarlanmış)
Open-Meteo'ya gider; hesap ya da anahtar yoktur. Şehir ararken yazdığın ad da
Open-Meteo'nun arama servisine gönderilir. Veri en çok 20 dakikada bir yenilenir.
Ayarlar → Dock → Canlı hava. Hava verisi: Open-Meteo.com (https://open-meteo.com),
CC BY 4.0.

**Yeni: Hayvanlar ve renkler.** Köpek (altın rengi), fare ve sincap eklendi. Kedi
(siyah, gri, beyaz) ve köpek (siyah, kahverengi, beyaz) için "Renk" seçeneği var.
Ayarlar'da her hayvanın ve rengin yanında küçük bir önizleme resmi görünür.

**Akvaryum.** Balıklar küçüldü, iki yeni balık türü eklendi; yunus biraz büyüdü.

**Bildirim efektleri.** Dock'ta seçili sahne (yağmur, kar, gece, bulut) bildirim
kartının arkasında da görünür; yazı her zaman okunur kalır. Gerçek hava açıksa kart
da o havayı gösterir. Ayarlar → Ada → "Bildirimlerde sahne efekti".

**SHA-256 (CarlUI-Setup.exe):**
`5b44789f0f584106c3231e10db0d2f0e2ec72abf6e1da27d0b9685725ec06502`

## v0.10.0 — Canlı dock, fotoğraftan hayvan, hazır görünümler (2026-11-12)

**Yeni: Canlı dock.** Dock'ta küçük piksel hayvanlar yaşar: kedi, kertenkele,
örümcek, yılan, yunus. Yılan dock'un üstünde dolaşır, kenarına sarılır, kıvrılıp
başını kaldırır ve imleci izler. Yunus dock'un içinde yüzer, üst kenardan sıçrayıp
geri dalar. Sahneler: yağmur, kar, gece (ay ve yıldızlar), bulutlar, saat, akvaryum.
Çok az kaynak kullanır (ölçülen: %0,1–0,8 işlemci, birkaç MB bellek); tam ekran
oyunda ve pilde durur. Ayarlar → Dock → Canlı dock.

**Yeni: Fotoğraftan hayvan.** Bir fotoğraf seç, CarlUI bilgisayarındaki yapay zekâ
ile özneyi kesip dock'ta yaşayan bir piksel hayvana çevirsin. Fotoğraf bilgisayarından
çıkmaz. Bir kez 214 MB'lık BiRefNet modeli (MIT lisansı) iner. Net, tek özneli
fotoğraflar en iyi sonucu verir.

**Yeni: Hazır görünümler.** Tek tıkla tema; kendi görünümünü `.carllook` dosyası
olarak dışa/içe aktar.

**Yeni: Serbest vurgu rengi.** İstediğin rengi seç ya da duvar kâğıdından otomatik
al. "Hareketi azalt" seçeneği eklendi.

**Yeni: İlk açılış karşılaması.** İlk kurulumda kısa bir tanıtım ve temel seçimler.

**Yeni: Daha ferah ayarlar.** Uzun açıklamalar küçük "i" düğmelerinin arkasındaki
bilgi balonlarına taşındı.

**Düzeltmeler:**
- GitHub'dan indirilen kurucuda güncelleme sonrası "Yenilikler" ekranı boş geliyordu,
  artık dolu.
- Hayvan paketi adlarında Windows'un ayrılmış adları (CON, NUL, COM1…) reddediliyor.

**SHA-256 (CarlUI-Setup.exe):**
`d4bcc641784736739fd588d8ddfdec0a390c81b7ecdf98a87dc45f97150590b7`

## v0.9.9 — Görev çubuğu panelleri, Hızlı Erişim yığını ve hata düzeltmeleri (2026-11-11)

**Yeni: Görev çubuğunda ayrı paneller.** Compact ve Full görünümde ses simgesi yalnızca
ses panelini (kaydırıcı, çıkış aygıtları, uygulama sesleri), ağ simgesi ağ panelini
(Ethernet durumu, görünen kablosuz ağlar; kayıtlı ağa tek tıkla bağlanır) açar. Ok
yalnızca parlaklık ve açık uygulamaları gösterir.

**Yeni: Dock'ta Hızlı Erişim yığını.** Simgesi içindeki ilk dört öğeyi üst üste
gösterir; ızgara dock tarafından kayarak açılır, solarak kapanır. Kapalı uygulamada
çizgi yok, arkadaki uygulamada sönük gri çizgi var.

**Güvenlik: Tarayıcı eklentisi eşleştirmesi.** Kasa artık yalnızca onayladığın
eklentiye parola verir: eklenti ilk kez kasayı kullanmak isteyince CarlUI bir kez onay
sorar. Eklenti 1.7.0'a güncellendi; Chrome/Edge'de eklentiler sayfasından **Yeniden
yükle**'ye bas.

**Düzeltmeler:**
- Ayarlar, dosya o an okunamazsa varsayılana sıfırlanmıyor; bir modülün kaydı
  ötekinin değişikliğini ezmiyor.
- Adında "İ" geçen dosya indirirken, panodan resim alırken ve görev yöneticisinde
  çökme olasılığı giderildi.
- Donan bir modül artık bütün CarlUI iletişimini kilitleyemiyor.
- İndirme yönlendirmelerinde çerezler başka siteye taşınmıyor; `site.com@baska.com`
  gibi adreslerde gerçek sunucu gösteriliyor.
- Explorer geç açılınca görev çubuğu gizleme ayarın yanlış kaydedilmiyor.
- Dock'ta sürükleme başarısız olursa simgenin grubu korunuyor; kasa dosyası diske tam
  yazılmadan yerine konmuyor.

**SHA-256 (CarlUI-Setup.exe):**
`3d15504d079b34e58f8df1599a2ecb73fa7b147a876f54ad6ad14d1c90df60e2`

## v0.9.8 — Dock tasarımları, Ada görünümleri ve görev yöneticisi düzeltmeleri (2026-11-11)

**Yeni: Dock konumu ve görünümü.** Dock artık altta, solda ya da sağda durabilir.
Altta "görev çubuğu" görünümü seçilebilir: Minimal (yüzen dock), Compact (ortada
simgeler, yanında ağ/ses/pil ve saat) ya da Full (kenardan kenara, saat sağ uçta).
Saat ayarlardaki 12/24 saat seçimine uyar ve ayar değişince hemen güncellenir.

**Yeni: Dock'a ek özellikler.** Klasör yığınları (İndirilenler gibi klasörler dock'ta
ızgara olarak açılır), açık uygulamalar için çizgi gösterge, ayırıcılar, medya
düğmeleri ve diğer monitörlerde de dock. Ayarlardan açılan çark dock: kısayola
(varsayılan Mouse5) basınca imlecin çevresinde uygulama halkası açılır.

**Yeni: Ada görünümleri.** Hap, Çentik (üst kenara yapışık) ve Kart görünümleri;
12/24 saat seçimi, daha çok tarih biçimi ve açılan kısım için ayrı tarih biçimi.

**Yeni: Win menüsü** kayarak ve belirerek açılır.

**Görev yöneticisi:** Başlangıç listesine RunOnce, ilke (policy) ve Mağaza
uygulamalarının başlangıç girdileri eklendi; ikinci kez açılınca var olan pencere öne
gelir; üstüne gelince yalnızca değişen satır yeniden çizilir.

**Düzeltme:** Çark dock açıkken bir uygulama kapanırsa seçim başka uygulamaya
kaymıyor.

**SHA-256 (CarlUI-Setup.exe):**
`dd80f79114dbc2f342408e0b9f2b6883032cccadb68a2a9fe0485b6321110d9d`

## v0.9.7 — Dock, Bluetooth ve oyunda gösterge düzeltmeleri (2026-11-08)

**Düzeltme: Dock'tan "Programı kapat" masaüstünü siyah bırakıyordu.** Dosya Gezgini
penceresinde bu komut explorer.exe'yi kapatıyordu; masaüstü ve görev çubuğu da aynı
süreç olduğu için ekran kararıyordu. Artık Gezgin hiçbir yoldan durdurulmaz ya da
kapatılmaz; klasör pencereleri "Pencereyi kapat" ile kapanır.

**Düzeltme: Küçültme animasyonu dock'a gidiyor.** Pencere küçülürken ve geri açılırken
animasyon gizli Windows görev çubuğundaki düğmeye gidiyordu. Artık dock'taki ikonuna
küçülüp oradan büyüyor; Gezgin yeniden başlasa da öyle kalıyor.

**Düzeltme: Bluetooth döşemesi açık olduğu hâlde kapalı görünüyordu.** Hızlı ayarlar
Bluetooth ve diğer kablosuz durumlarını artık sürekli sormak yerine değişim olduğunda
öğreniyor: hem doğru görünüyor hem daha hafif.

**Yeni: Özel tam ekran oyunda gösterge diğer monitörde.** Ses ya da şarkı değişince
gösterge, oyunun olmadığı monitörde çıkar; oyun tam ekrandan düşmez. Tek monitörde
gösterge yine çıkmaz.

**SHA-256 (CarlUI-Setup.exe):**
`378be91eaed6cc652d6d9115b6364cb48af529395389e5379036b44850dce827`

## v0.9.6 — Oyunda ses/şarkı göstergesi düzeltmesi (2026-11-08)

**Düzeltme: Ses ya da şarkı değişince oyun arkaya düşüyordu.** Özel tam ekran
(exclusive) oyunun üstünde herhangi bir pencere belirince Windows oyunu tam ekrandan
düşürür; ses/şarkı göstergesi de bunu tetikliyor, oyun küçülüp arkaya geçiyordu.
Artık oyun ekranı ele geçirmişse (ekran kartında özel sahiplik ya da masaüstünden
farklı çözünürlük) gösterge çıkmaz. Kenarlıksız tam ekranda ve tam ekran videoda
eskisi gibi odak almadan görünür. Ayrıca oyunun ekran kipi değişikliği "monitör
değişti" sanılıp pencereler geri konmuyor: tam ekrandan sonraki 30 saniye boyunca
pencere yerleri oynatılmaz.

**SHA-256 (CarlUI-Setup.exe):**
`c2c9e5cbee14e492ff5a6dc9a9a0ffab3bae00601305dc3a030348c0e7035fc4`

## v0.9.5 — Asistana döviz kuru (2026-11-08)

**Yeni: "Dolar kaç TL?"** Sesli asistana kuru sorabilirsin: "dolar kaç", "100 euro kaç
TL", "iki yüz elli euro ne kadar", "euro kaç dolar", "how much is a dollar in lira".
Yanıt ekranda görünür ("1 dolar = 50,10 TL · 08.11.2026"); Türkçe ses kuruluysa sesli
de okunur ("1 dolar, 50 lira 10 kuruş"). Kur, arama kutusundaki döviz çevirisiyle aynı
yerden (Avrupa Merkez Bankası verisi, Frankfurter) gelir. Soru kurallarla anlaşılır,
dil modeli beklenmez. "YouTube'da dolar kuru ara" yine arama yapar.

**SHA-256 (CarlUI-Setup.exe):**
`2737243e176665eef011c505c01a698571835dfda7ffe025de03f4696c5ffc40`

## v0.9.4 — Akıllı asistan modeli düzeltmesi (2026-11-08)

**Düzeltme: Akıllı asistan modeli kurulu görünmüyordu.** Qwen3 4B indirilip "hazır"
bildirimi çıktığı halde Ayarlar yine "İndir" gösteriyor, asistan modeli kullanmıyordu.
İndirilen llama.cpp paketi açıldıktan sonra silinir; kurulum denetimi ise paketin
klasörde durmasını bekliyordu. Artık açılan paketin yerine SHA-256 damgası kalır ve
denetim ona bakar. 0.9.3 ile indirmiş olanlar yalnızca 37 MB'lık paketi bir kez daha
indirir; 2,5 GB'lık model yeniden inmez.

**SHA-256 (CarlUI-Setup.exe):**
`52ab27a0260ee50ed43f7c1b969cc59cf26faa7a9b6dfc8c2893755dc9aba661`

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
`26f0c259a761c192ab0a966a57942f47ce39466dadea72c5813050f0f6a6660e`

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
