# CarlUI

Windows 11 için Rust ile yazılmış masaüstü kabuğu: yüzen dock, tray paneli,
üst bildirim alanı, başlatıcı. Tasarım dili: OneUI + Liquid Glass.

## Mimari (hibrit)

| Parça | Teknoloji | Neden |
|---|---|---|
| Sürekli ekranda olanlar (dock, tray paneli, bildirimler) | Rust + Windows.UI.Composition | Animasyonlar Windows'un compositor'ünde çalışır, boşta CPU ≈ 0 |
| Ayarlar penceresi | Tauri + React | Zengin form ekranı; sadece açıkken çalışır |

```
crates/
  carlui-core   ortak çekirdek: ayar servisi, tema, log, olay yolu
  carlui-ui     ortak görsel parçalar: cam yüzey, spring animasyon, ikonlar
  carlui        ana süreç: görev çubuğu, koruyucu, modül denetleyici, tam ekran algılama
  carlui-dock   dock modülü
  carlui-tray   tepsi ikonları + kontrol merkezi paneli
  carlui-island üstteki ada: saat, bildirimler, medya kontrolleri, arama
  carlui-launcher arama mantığı (kütüphane): uygulama listesi, hesap, döviz, dosya, eylemler
```

Her modül ayrı bir süreçtir. Biri çökerse diğerleri çalışmaya devam eder,
ana süreç çökeni yeniden başlatır. Modüller birbirleriyle olay yolu (named pipe)
üzerinden konuşur.

| Dosya | Konum |
|---|---|
| Ayarlar | `%APPDATA%\CarlUI\config.toml` (kaydedince anında uygulanır) |
| Loglar | `%LOCALAPPDATA%\CarlUI\logs\` (7 gün saklanır) |
| Görev çubuğu durumu | `%LOCALAPPDATA%\CarlUI\taskbar-state.json` (yalnızca CarlUI çalışırken var) |

## Derleme ve çalıştırma (adım adım)

**Bir kez yapılır:** Rust kurulu değilse https://rustup.rs adresinden kur
(Visual Studio Build Tools da gerekir; kurulum sihirbazı sorar).

1. Başlat menüsünde "PowerShell" yaz ve aç.
2. Proje klasörüne geç:
   ```powershell
   cd "$env:USERPROFILE\Desktop\CarlUI"
   ```
3. Derle (ilk derleme birkaç dakika sürer, sonrakiler saniyeler):
   ```powershell
   cargo build --release
   ```
4. Çalıştır:
   ```powershell
   .\target\release\carlui.exe
   ```
5. Kapat (görev çubuğu geri gelir):
   ```powershell
   .\target\release\carlui.exe --quit
   ```
   Ya da dock'a sağ tıkla → **CarlUI'den çık**.

**Acil durum:** Görev çubuğu bir şekilde geri gelmezse:
```powershell
.\target\release\carlui.exe --restore-taskbar
```

## Modül 1 — Çekirdek ve görev çubuğu yönetimi: test adımları

| # | Ne yap | Beklenen |
|---|---|---|
| 1 | `carlui.exe` çalıştır | Görev çubuğu kaybolur, dock alttan biraz yukarıda belirir |
| 2 | Bir pencereyi maksimize et | Pencere ekranın en altına kadar uzanır, boşluk kalmaz |
| 3 | `Win + D` bas | Dock görünür kalır |
| 4 | Görev Yöneticisi → `carlui-dock.exe` → Görevi sonlandır | Dock bir saniye içinde geri gelir |
| 5 | Görev Yöneticisi → `carlui.exe` (ikisi de) → Görevi sonlandır | Görev çubuğu geri gelir, dock kapanır |
| 6 | Tekrar çalıştır, sonra `--quit` | Görev çubuğu geri gelir; "otomatik gizle" ayarın açılmadan önceki haliyle aynıdır |
| 7 | Görev Yöneticisi → "Windows Gezgini" → Yeniden başlat | Görev çubuğu bir an görünüp tekrar gizlenir, dock kalır |
| 8 | Bir videoyu tam ekran yap ya da tam ekran bir oyun aç | Dock tamamen kaybolur; çıkınca animasyonla geri gelir |
| 9 | `config.toml` içinde `icon_size = 36` yapıp kaydet | Dock yeniden başlatma olmadan büyür |
| 10 | `config.toml` içinde `hide_taskbar = false` yapıp kaydet | Görev çubuğu anında geri gelir |

Tablodaki 10 maddenin hepsi otomatik test edildi ve geçti. Ek olarak "ana süreç
ve koruyucu aynı anda öldürülürse bir sonraki açılış sistemi düzeltir"
senaryosu da doğrulandı. Notlar:
- 7: Görev çubuğu yeniden başlarken yaklaşık 1 sn görünüp tekrar gizleniyor.
- 8: Çerçevesiz tam ekran pencereyle test edildi. Gerçek bir "exclusive
  fullscreen" oyunla ayrıca denenmeli.

Ölçüm (release, boşta, 10 sn): ana süreç 1,7 MB, koruyucu 1,1 MB, dock 62 MB
(çoğu Direct3D/compositor kütüphaneleri); üçünde de CPU 0,000 sn.

## Modül 2 — Dock: test adımları

| # | Ne yap | Beklenen |
|---|---|---|
| 1 | İlk açılış | Görev çubuğundaki sabitlenmişler aynı sırayla dock'a gelir (Store uygulamaları dahil) |
| 2 | Bir uygulama aç | Dock yumuşakça genişler, ikon büyüyerek belirir, altında nokta çıkar |
| 3 | Aynı uygulamadan 2. pencere aç | İkonun altında 2 nokta; öndeki uygulamanın noktası vurgu renginde haptır |
| 4 | Çok pencereli ikona tıkla | Pencereler arasında sırayla döner |
| 5 | Tek pencereli, öndeki uygulamanın ikonuna tıkla | Pencere küçülür; tekrar tıkla → geri gelir |
| 6 | Bir ikonun üstünde bekle | Adı cam bir etikette görünür |
| 7 | Açık uygulamanın ikonunda yarım saniye bekle | Canlı pencere önizlemeleri açılır; karta tıkla → o pencere öne gelir; × → pencere kapanır |
| 8 | İkona sağ tıkla | Koyu menü dock'un üstünde açılır: yeni pencere, sabitle/kaldır, kapat |
| 9 | Bir ikonu sürükle | Diğerleri yer açar; bırakınca sıra kaydedilir. Açık bir uygulamayı sabitlenmişlerin arasına bırakmak onu sabitler |
| 10 | Uygulamayı kapat | İkon küçülerek kaybolur, dock daralır |
| 11 | `behavior = "auto-hide"` | Fare uzaklaşınca dock aşağı kayar; ekranın alt kenarına (dock hizasında) gelince geri çıkar |
| 12 | `behavior = "intellihide"` | Bir pencere dock'un alanına girince (ör. maksimize) dock gizlenir; çekilince geri gelir |
| 13 | `Win + 1…9` | Dock'taki N. uygulamayı açar/öne getirir/küçültür; Başlat menüsü açılmaz |
| 14 | Orta tık | Uygulamanın yeni bir penceresini açar |

Hepsi bu oturumda gerçek masaüstünde otomatik test edildi ve geçti. Ek olarak:
- **Fare takibi bekçisi:** Fare kancası bilerek 1,5 sn takıltıldı; Windows kancayı
  kaldırdı, bekçi ~2 sn içinde fark edip yeniden kurdu, sonra Win+5 çalıştı.
- **Tam ekran ve Win+D** dock'un yeni haliyle tekrar doğrulandı.

Ölçüm (release, boşta 15 sn, fare hareketsiz): dock 68 MB, 0,016 sn CPU (diğer
uygulamaların pencere başlığı değişiklikleri dock'u kısa süreliğine uyandırıyor).

Notlar:
- Başlat ve Ara düğmeleri, kendi başlatıcımız (5. adım) gelene kadar Windows'un
  menüsünü açıyor; bunun için Win tuşu gönderiliyor. Pencere öne getirme ve
  küçültmede hiçbir sahte tuş kullanılmıyor.
- Win+N yakalanınca Başlat menüsünün açılmaması için atanmamış bir tuş (0xE8)
  gönderiliyor; Windows araçlarında yaygın, zararsız bir yöntem.
- Fare kancası Win+1…9 açıkken de kurulur (bekçinin nabzı için). Fare hareket
  etmezken hiç uyanmaz.
- Tanı için: `CARLUI_TRACE=1` ortam değişkeniyle başlatılırsa otomatik gizleme
  kararları log'a yazılır.

## Modül 3 — Tray + kontrol merkezi: test adımları

| # | Ne yap | Beklenen |
|---|---|---|
| 1 | CarlUI açıkken tepsi ikonu olan bir uygulama çalıştır | İkon dock'un sağındaki ok düğmesinin panelinde görünür |
| 2 | Dock'taki yukarı oka tıkla | Balon düğmeden çıkar, yukarı süzülür, panel genişler, içerik sırayla belirir; ok aşağı döner |
| 3 | Oka tekrar tıkla | Ters animasyon: panel küçülür, balon düğmeye iner; düğmede ikinci ikon kalmaz |
| 4 | Paneldeki bir ikona sol tıkla | Uygulama tıklamayı alır (ör. penceresi açılır), panel kapanır |
| 5 | Bir ikona sağ tıkla | Uygulamanın kendi menüsü açılır |
| 6 | Panel açıkken boş bir yere tıkla | Panel kapanır |
| 7 | Ses kaydırıcısını sürükle / ikonuna tıkla | Sistem sesi değişir / sessize alınır |
| 8 | Wi-Fi / Bluetooth kutucuğu | Radyoyu açar/kapatır (cihazda yoksa kutucuk soluk görünür) |
| 9 | Parlaklık kaydırıcısı | DDC/CI destekleyen monitörde parlaklık değişir; desteklemiyorsa kaydırıcı gizlenir |
| 10 | Panel açıkken tam ekrana geç | Panel kendiliğinden kapanır |
| 11 | Görev Yöneticisi → `carlui-tray.exe` → sonlandır | Bir saniye içinde geri gelir; uygulamalar ikonlarını birkaç saniye içinde (her biri kendi hızında) yeniden gönderir |

1–7, 10 ve 11 gerçek masaüstünde otomatik test edildi ve geçti (sol tık, sağ tık
menüsü, ses %42 → %25 → %42 dahil). 9: monitör DDC/CI parlaklığını okuyabiliyor,
kaydırıcı görünüyor (değiştirme denenmedi). 8 (Wi-Fi/Bluetooth aç-kapa) henüz
denenmedi: bağlantıyı koparabileceği için uzaktan otomatik test edilmedi.
11 dört kez art arda denendi: Claude, CarlHub, AnyDesk ve Windows'un ses ikonu
her seferinde ~6 sn içinde geri geldi. Bir istisna: PowerShell ile yazılmış test
ikonu kendini yeniden kaydetmedi (o uygulamanın kendi davranışı; ikon explorer'da
kalmaya devam ediyor, kaybolmuyor).

Ölçüm (release, boşta 15 sn): tray 60 MB, 0,016 sn CPU.

Nasıl çalışıyor: CarlUI, Windows'un görev çubuğu penceresiyle aynı sınıf adına
sahip gizli bir pencere açar. Uygulamalar tepsi ikonlarını bu pencereye gönderir;
CarlUI kopyasını alıp mesajı aynen gerçek görev çubuğuna da iletir. Böylece
CarlUI kapanınca hiçbir ikon kaybolmaz.

## Modül 4 — Ada (saat, bildirim, medya): test adımları

| # | Ne yap | Beklenen |
|---|---|---|
| 1 | CarlUI'yi aç | Ekranın üst ortasında küçük cam bir hap içinde saat görünür |
| 2 | Saate tıkla | Hap genişler, tarih + saat 4 sn görünür, sonra geri küçülür |
| 3 | Bir uygulamadan bildirim gelsin | Ada aşağı doğru genişler: ikon, uygulama adı, başlık, metin. 2,5 sn sonra kendiliğinden küçülür (fare üstündeyse bekler) |
| 4 | Bildirime tıkla | Bildirimi gönderen uygulama öne gelir (kapalıysa açılır) |
| 5 | Bildirime sağ tıkla | Bildirim hemen kapanır |
| 6 | Spotify / YouTube'da şarkı çal | Ada'da kapak, saat ve oynayan çubuklar görünür |
| 7 | Medya adasına tıkla | Büyür: kapak, şarkı adı, sanatçı, ilerleme çubuğu, önceki/oynat/sonraki, ses kaydırıcısı |
| 8 | İlerleme çubuğuna tıkla veya sürükle | Şarkı o noktaya atlar |
| 9 | Önceki / sonraki / oynat-duraklat | Şarkıyı çalan uygulamada çalışır |
| 10 | Ses kaydırıcısı | Yalnızca o uygulamanın sesi değişir (Spotify'da çalıyorsa Spotify, YouTube'daysa tarayıcı); sistem sesi ve diğer uygulamalar değişmez |
| 11 | Fareyi adadan uzaklaştır | Ada kendiliğinden küçülür |
| 12 | Şarkı çalarken bildirim gelsin | Bildirim gösterilir, sonra medya adasına geri dönülür |
| 13 | Tam ekrana geç (video/oyun) | Ada kaybolur, bildirimler araya girmez |
| 14 | Şarkıyı kapat | Ada saate döner (duraklatılmışsa 60 sn bekler) |
| 15 | `[island] hide_over_maximized = true` | Maksimize pencere varken ada yukarı kayar; fare ekranın üst kenarına gelince görünür, bildirimler yine aşağı iner |

1–15 gerçek masaüstünde otomatik test edildi ve geçti. Medya testleri için
Windows'un kendi oynatıcısını kullanan küçük bir test çalar yazıldı
(`examples/test_player.rs`). Spotify ile aynı sistem medya kontrollerini kullanır.
Ses kaydırıcısı testinde çalar %40'a indi; sistem sesi %42'de, diğer uygulamalar
%100'de kaldı. 4: tıklanınca kaynak pencere öne geldi. 3: PowerShell ile gönderilen
gerçek bir Windows bildirimi yakalandı. Gerçek Spotify ve YouTube ile ayrıca
denenmeli.

Ölçüm (release, 20 sn): boşta 57 MB, 0,063 sn CPU; şarkı çalarken 63 MB,
0,031 sn CPU (ilerleme çubuğu ve çubuk animasyonları ekran kartında döner).

Nasıl çalışıyor:
- **Medya:** Windows'un sistem medya kontrolleri (klavyedeki medya tuşlarının
  konuştuğu yer). Spotify, tarayıcılar ve çoğu oynatıcı buraya bilgi verir. Komutlar
  da aynı yoldan gider; uygulamalara hiçbir şey enjekte edilmez.
- **Uygulamaya özel ses:** Windows ses karıştırıcısındaki o uygulamanın kaydırıcısı
  değiştirilir (Ses karıştırıcısı'nda da görünür).
- **Bildirimler:** Windows'un bildirim dinleyicisi. Yeni bildirim geldiğinde
  Windows'un kayıt defterine yazdığı zaman damgası izlenir, liste yalnızca o an
  okunur. Boştayken hiçbir şey çalışmaz.

Notlar:
- İlk açılışta Windows bildirimlere erişim izni isteyebilir: Ayarlar → Gizlilik ve
  güvenlik → Bildirimler.
- Windows'un kendi bildirim balonu (sağ altta) ada çalışırken kapatılır
  (`[notify] hide_windows_banners = true`, Ayarlar → Ada → "Windows bildirim
  balonlarını gizle"): her uygulamanın `ShowBanner` değeri 0 yapılır, bildirimler
  Bildirim Merkezi'ne yine düşer. Ada kapanınca yalnızca CarlUI'nin kapattıkları
  geri açılır. Hiç bildirim göndermemiş bir uygulamanın ilk balonu görünebilir.
- Ayarlar: `[clock] mode = "always" | "when-app-active" | "never"`,
  `[island] top_margin = 6`, `[island] hide_over_maximized = false`,
  `[island] media_tint = true` (şarkı/video çalarken ada kapağın rengini alır).

## Modül 5 — Arama (Spotlight adaya taşındı): test adımları

Eski CarlSpotlight artık gerekmiyor: arama üstteki adanın parçası. Windows
tuşuna (ya da Ctrl+Space'e) basınca ada aşağı doğru büyür; en üstte saat ve
çalan şarkı kalır, altında arama kutusu, onun altında sonuç sayısına göre
büyüyüp küçülen liste (en fazla 8 satır, fazlası kaydırılır).

| # | Ne yap | Ne görmelisin |
|---|---|---|
| 1 | Windows tuşuna bas, bırak | Başlat menüsü yerine ada aşağı açılır, imleç kutuda yanıp söner |
| 2 | Hiçbir şey yazma | En çok açtığın 6 uygulama + Tüm uygulamalar / Ayarlar / Dosya Gezgini + kilit, yeniden başlat, kapat |
| 3 | `not` yaz, Enter | Not Defteri açılır, arama kapanır |
| 4 | `2+2*3` yaz | `= 8`; Enter sonucu panoya kopyalar |
| 5 | `100 usd` yaz | Güncel kurla TL karşılığı (internet gerekir) |
| 6 | `10 km mil` yaz | Birim çevirisi |
| 7 | `chatgpt merhaba` yaz, Enter | ChatGPT soruyla açılır (claude, gemini, perplexity, codex da var) |
| 8 | Bir dosya adı yaz | Masaüstü/Belgeler/İndirilenler… içindeki dosyalar listelenir |
| 9 | Bir uygulamada Tab'a bas | Eylemler: aç / yeni pencere / kapat / yeniden başlat / konumu aç / yolu kopyala |
| 10 | Ctrl+Enter / Shift+Enter | Dosya konumunu açar / yönetici olarak çalıştırır |
| 11 | `/` yaz | Sistem komutları; yeniden başlat ve kapat için iki kez Enter gerekir |
| 12 | Esc | Önce yazıyı siler, ikinci Esc kapatır ve önceki pencereye döner |
| 13 | Arama açıkken başka yere tıkla | Arama kapanır |
| 14 | Fare tekerleği / üstüne gelme / sağ tık | Kaydırır / seçer / eylemleri açar |
| 15 | Dock'taki Ara düğmesi | Aynı arama açılır (Başlat düğmesi Win menüsünü açar) |
| 16 | Şarkı çalarken aramayı aç | Sağ üstte şarkı, ada kapağın renginde |
| 17 | Bilgisayarı yeniden başlat | CarlUI oturum açılır açılmaz gelir (Görev Zamanlayıcı → "CarlUI" görevi) |

Kısayollar (`config.toml` → `[shortcuts]`, kaydedince anında uygulanır):

```toml
[shortcuts]
search = "Ctrl+Space"          # boş bırakılırsa kapalı
win_key = "menu"               # "menu": CarlUI Win menüsü, "search": arama, "windows": Başlat menüsü
app_numbers = "Win"            # "Ctrl+Alt" → Ctrl+Alt+1…9, boş → kapalı
```

Kısayollar bozulmasın diye doğrulanır: fare tuşları (sol/sağ/orta tık),
değiştiricisiz tek tuş (F13–F24 hariç), yalnızca Shift, Windows'un temel
kısayolları (Ctrl+Alt+Del, Alt+Tab, Alt+F4, Win+L, Win+D…) ve Ctrl+C/V/X/Z/A/S
kabul edilmez. Geçersiz değer girilirse varsayılan kullanılır ve loga yazılır.

Diğer ayarlar: `[launcher] enabled`, `file_search`, `currency`,
`web_search = "https://www.google.com/search?q={q}"`,
`[general] start_with_windows = true`.

Ölçüm (ada süreci, arama dahil): boşta 85 MB, aramayı açıp kapatınca 89 MB;
arama kapalıyken yoklama yapılmaz (uygulama listesi, dosya dizini ve kur yalnızca
arama açılınca, gerekiyorsa arka planda tazelenir).

Nasıl çalışıyor:
- Windows tuşu: dock'un klavye kancası tuşun tek başına basılıp bırakıldığını
  görür, bırakmayı yutup adaya "aramayı aç" der; araya zararsız bir maskeleme
  tuşu eklenir, Başlat menüsü açılmaz, Win takılı kalmaz. Win+E gibi
  kısayollar etkilenmez. Tam ekran oyunda yapılmaz.
- Uygulama listesi `shell:AppsFolder`'dan (Başlat menüsünün kendi listesi)
  okunur, önbelleğe yazılır; Başlat menüsü klasörü değişince tazelenir.
- Eski CarlSpotlight'ın kullanım sayıları ilk açılışta içeri aktarılır; başlangıç
  kısayolları `%LOCALAPPDATA%\CarlUI\backup\startup\` klasörüne taşındı (silinmedi).
- Hızlı başlangıç: Run anahtarı ve Başlangıç klasörü Explorer'ın gecikmesine
  takılır; CarlUI bunun yerine oturum açılınca hemen çalışan bir zamanlanmış
  görev kurar (yönetici izni gerekmez). `start_with_windows = false` kaldırır.

## Modül 6 — Pano: test adımları

Eski Pano uygulaması artık gerekmiyor. Win+V'ye basınca farenin altında küçük
bir yuvarlak doğar, sağa kayar ve orada panele büyür (~0,25 sn); içerik
buzlu bir camın ardından belirir, panel büyüdükçe cam açılır (gerçek blur
yerine tek renkli bir katman, ek yük yok). Panel ince ve kısadır (440 px, 5 satır). Solda kayıt
listesi (resimler küçük resimle), sağda önizleme. Eski Pano'nun geçmişi ilk
açılışta içeri aktarıldı.

| # | Ne yap | Ne görmelisin |
|---|---|---|
| 1 | Bir yazı kopyala, Win+V | Panel farenin sağında açılır, en üstte kopyaladığın yazı |
| 2 | WhatsApp'ta bir resme sağ tık → Kopyala, Win+V | Resim listede küçük resmiyle, önizlemede sığdırılmış görünür |
| 3 | Resim önizlemesinin üstünde yarım saniye bekle (ya da göz düğmesine bas) | Resim önizlemeden kopup programın dışına taşarak büyür, ekranın ortasına gelir |
| 4 | Büyük resimde tekerlek ya da +/-, sürükle, oklar | Yumuşakça yakınlaşır, kayar; Esc, X ya da dışarı tıklamak resmi önizlemeye geri küçültür |
| 5 | Yazı seçiliyken önizlemedeki yazıya tıkla | Önizleme kutusu yerinden biraz taşarak (panelin içinde) büyür, yazıyı orada düzenlersin |
| 6 | Kartta **Kaydet** / **X** | Yeni hâli kaydın üstüne yazar / eski hâline döner |
| 7 | Düzenlerken Ctrl+Enter / Ctrl+S / Esc | Kaydet ve yapıştır / kaydet / vazgeç |
| 8 | `istanbul` yaz | Türkçe harfler doğru eşleşir (İstanbul bulunur) |
| 9 | Enter / Shift+Enter | Seçileni yapıştırır / yalnızca panoya kopyalar |
| 10 | Tab | Eylemler: sabitle, sil, BÜYÜK/küçük harf, boşlukları temizle… |
| 11 | Ctrl+P / Delete | Sabitle (listede üstte kalır, silinmez) / sil |
| 11a | Sil (çöp kutusu / Delete) | Hemen silmez: altında "Silinsin mi?" balonu açılır; ✓ (ya da Enter) siler, ✗ (Esc, başka yere tıklama, çöp kutusuna tekrar basma) vazgeçer |
| 11b | Silindikten sonra **Geri al** (ya da Ctrl+Z) | Alt satırda 6 sn "Kayıt silindi · Geri al" durur; basınca kayıt eski yerine döner |
| 11c | Bir şifre kopyala (ör. `Kedi#Mavi2026`) | Listede `••••••••` ve kilit simgesiyle görünür, önizlemede de gizli; Enter ile yine yapıştırılır, 60 sn sonra kendiliğinden silinir (sabitlenirse kalır) |
| 12 | Tam ekran oyunda Win+V | Panel oyunun üstünde odak almadan açılır, oyun küçülmez; tuşlar panele gider, Enter seçileni oyuna (ör. CS2 sohbetine) yapıştırır. Ctrl+Space oyuna gider |

Ayarlar (`config.toml`):

```toml
[clipboard]
enabled = true
max_items = 200          # sabitlenenler sayılmaz
paste_on_enter = true    # false: Enter yalnızca panoya kopyalar
secret_seconds = 60      # şifre gibi görünen kopyaların ömrü (0 = silinmez, yalnızca gizlenir)

[shortcuts]
clipboard = "Win+V"      # boş bırakılırsa kapalı
```

Kayıtlar `%LOCALAPPDATA%\CarlUI\clip\` klasöründe (history.json + resimler).

Oyun güvenliği: tam ekran (exclusive dahil) bir uygulama öndeyken arama
kısayolları Windows'tan tamamen geri alınır; Ctrl+Space ve Windows tuşu oyuna
olduğu gibi gider. Win+V ise pano panelini odak almadan açar (bkz. 12). Kısayola basıldığı anda da bir kez daha "önde oyun var
mı" bakılır; varsa tuş oyuna geri gönderilir.

## Modül 7 — Oyun modu: test adımları

Tam ekran (çerçevesiz ya da exclusive) bir uygulama öne gelince CarlUI oyunun
yolundan tamamen çekilir; oyuna hiç dokunmaz (hook, enjeksiyon yok, anti-cheat uyumlu).

| # | Adım | Beklenen |
|---|------|----------|
| 1 | Tam ekran bir oyun/video aç | Dock, ada, tepsi paneli kaybolur; kısayollar oyuna gider |
| 2 | Oyundayken Görev Yöneticisi → Ayrıntılar | `carlui-*` süreçleri "Verimlilik modu" yaprağıyla görünür, bellekleri birkaç MB'a iner |
| 3 | Oyundayken bildirim gelsin | Gösterilmez, saklanır |
| 4 | Oyundan çık | Her şey animasyonla geri gelir; 1 sn sonra kaçan bildirimler gösterilir (3'ten fazlaysa önce "Oyundayken N bildirim geldi" özeti, sonra son üçü) |

Ölçüm (çerçevesiz tam ekran, 5 bildirim): bellek dock 63,7 → 10,2 MB, ada 65,8 → 4,9 MB,
tepsi 40,9 → 3,5 MB, ana süreç 8,5 → 1,6 MB. Uyku durumu = EcoQoS (verimlilik modu) +
arka plan önceliği (işlemci, disk, bellek) + çalışma kümesini boşaltma; çıkışta geri alınır.

Ayarlar (`config.toml`):

```toml
[game]
hide_on_fullscreen = true   # tam ekranda arayüzü gizle
sleep_modules = true        # oyunda modülleri uyku durumuna al
hold_notes = true           # oyunda gelen bildirimleri sakla, çıkınca göster
```

## Yol haritası

1. ✅ Çekirdek + görev çubuğu yönetimi + koruyucu
2. ✅ Dock: açık pencere takibi, noktalar, önizleme, sürükle-bırak, sabitlenmişleri içeri aktarma, otomatik gizleme, Win+1…9
3. ✅ Tray + kontrol merkezi (5 adımlı balon animasyonu)
4. ✅ Ada: saat + bildirimler + medya kontrolleri (uygulamaya özel ses)
5. ✅ Arama (Spotlight adada) + hızlı başlangıç + ayarlanabilir kısayollar + kapak rengi
6. ✅ Pano (Win+V, resim görüntüleyici, yerinde metin düzenleme, silme onayı + geri al, gizli kayıtlar, eski Pano'dan içeri aktarma)
7. ✅ Oyun modu (uyku durumu: verimlilik modu + bellek boşaltma, oyunda gelen bildirimleri sonraya saklama)
8. Ayarlar ekranı (Tauri + React)
9. Yükleyici
