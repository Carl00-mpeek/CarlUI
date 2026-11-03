<p align="center">
  <a href="README.md"><img src="https://img.shields.io/badge/English-1f2128?style=for-the-badge" alt="English"></a>
  <a href="README.tr.md"><img src="https://img.shields.io/badge/T%C3%BCrk%C3%A7e-3E8BFF?style=for-the-badge" alt="Türkçe"></a>
</p>

<h1 align="center">CarlUI</h1>

<p align="center">
  Windows 11 için Rust ile yazılmış, hafif ve hızlı bir masaüstü kabuğu.<br>
  Yüzen dock, üstte saat/bildirim/medya adası, arama, pano geçmişi ve kontrol merkezi.
</p>

<p align="center">
  <a href="https://github.com/Carl00-mpeek/CarlUI/releases/latest"><img src="https://img.shields.io/github/v/release/Carl00-mpeek/CarlUI?style=flat-square&label=s%C3%BCr%C3%BCm&color=3E8BFF" alt="Sürüm"></a>
  <img src="https://img.shields.io/badge/Windows-11-3E8BFF?style=flat-square" alt="Windows 11">
  <img src="https://img.shields.io/badge/Rust-orange?style=flat-square" alt="Rust">
  <a href="LICENSE"><img src="https://img.shields.io/badge/lisans-MIT-green?style=flat-square" alt="MIT"></a>
</p>

<p align="center">
  <a href="https://github.com/Carl00-mpeek/CarlUI/releases/latest"><b>⬇ CarlUI-Setup.exe indir</b></a>
  &nbsp;·&nbsp; yönetici izni gerekmez &nbsp;·&nbsp; ücretsiz ve açık kaynak
</p>

![CarlUI masaüstü](docs/screenshots/desktop-tr.png)

<p align="center">
  <img src="docs/screenshots/island-tr.png" alt="Ada: indirme ilerlemesi ve bildirimler"><br>
  <sub><b>Ada</b> · canlı indirme ilerlemesi (ad, boyut, hız, kalan süre), Steam oyunları dahil, ve bildirimler</sub>
</p>

<table>
  <tr>
    <td width="50%"><img src="docs/screenshots/calc-tr.png" alt="Hesap makinesi"><br><sub><b>Arama</b> · yazdıkça hesaplar</sub></td>
    <td width="50%"><img src="docs/screenshots/currency-tr.png" alt="Döviz"><br><sub><b>Döviz ve birim çevirisi</b> · günlük kur</sub></td>
  </tr>
  <tr>
    <td><img src="docs/screenshots/clip-tr.png" alt="Pano geçmişi"><br><sub><b>Pano geçmişi (Win+V)</b> · metin ve resim, arama, düzenleme</sub></td>
    <td><img src="docs/screenshots/control-tr.png" alt="Kontrol merkezi"><br><sub><b>Kontrol merkezi</b> · ses, parlaklık, Wi-Fi, tepsi ikonları</sub></td>
  </tr>
  <tr>
    <td><img src="docs/screenshots/settings-tr.png" alt="Ayarlar"><br><sub><b>Ayarlar</b> · her şey tek pencerede, anında uygulanır</sub></td>
    <td><img src="docs/screenshots/taskmgr-tr.png" alt="Görev yöneticisi"><br><sub><b>Görev yöneticisi</b> · imzasız programlar öne</sub></td>
  </tr>
  <tr>
    <td><img src="docs/screenshots/setup-tr.png" alt="Kurucu"><br><sub><b>Kurucu</b> · tek dosya, iki dil, yönetici izni gerekmez</sub></td>
    <td></td>
  </tr>
</table>

## Neler var?

- **Dock:** Sabitlenmiş ve açık uygulamalar, canlı pencere önizlemeleri,
  sürükle-bırak, otomatik gizleme, Win+1…9, Geri Dönüşüm Kutusu.
- **Ada:** Üst ortada saat; bildirimler, çalan şarkı (kapak, ilerleme,
  uygulamaya özel ses), indirme ilerlemesi: dosya adı, boyut, hız ve kalan
  süre (her biri Ayarlar'dan kapatılabilir). Başka programların (Steam, Epic,
  torrent…) indirmeleri de görünür: internet ve disk kullanımından tahmin
  edilir, Steam'de oyunun adı ve ilerlemesi de gelir.
- **Arama:** Windows tuşu ya da Ctrl+Space. Uygulamalar, dosyalar, hesap
  makinesi, birim/döviz çevirisi, web ve yapay zekâ kısayolları.
- **Win menüsü:** Sabitlenmiş uygulamalar, klasörler, güç düğmeleri.
- **Pano geçmişi (Win+V):** Metin ve resimler, arama, sabitleme, yerinde
  düzenleme, şifreleri gizleme ve kendiliğinden silme. Tam ekran oyunda da
  oyunu küçültmeden açılır.
- **Kontrol merkezi:** Tepsi ikonları, ses (çıkış aygıtı ve uygulama başına
  ses), Wi-Fi, Bluetooth, parlaklık.
- **Oyun modu:** Tam ekranda arayüz çekilir, modüller uyku moduna geçer,
  bildirimler oyundan sonra gösterilir.
- **Görev yöneticisi, dosya dönüştürücü, ekran görüntüsü aracı** ve hepsini
  yöneten **Ayarlar** penceresi. Yetki isteyen süreçler ve başlangıç öğeleri
  için görev yöneticisinde **Yönetici olarak çalıştır** düğmesi var.
- **Türkçe ve İngilizce arayüz:** Windows'un görüntü diline uyar; **Ayarlar →
  Genel → Dil** bölümünden değiştirilebilir.

## Kurulum

1. [Releases](https://github.com/Carl00-mpeek/CarlUI/releases/latest) sayfasından `CarlUI-Setup.exe` dosyasını indir.
2. Çalıştır, **Kur**'a bas. Yönetici izni gerekmez; CarlUI
   `%LOCALAPPDATA%\Programs\CarlUI` klasörüne kurulur, Başlat menüsüne eklenir
   ve oturum açılınca kendiliğinden başlar.

**"Windows bilgisayarınızı korudu" uyarısı:** Kurucu imzasız olduğu için
SmartScreen bu uyarıyı gösterir. **Ek bilgi → Yine de çalıştır** ile devam
edebilirsin. Bazı antivirüsler de yanlış alarm verebilir: CarlUI görev
çubuğunu gizler, klavye kısayollarını yakalamak için klavye kancası kullanır ve
isteğe bağlı sıcaklık okuyucu yönetici olarak çalışır. Bunların hepsi yalnızca
bu bilgisayarda olur. CarlUI veri toplamaz; yalnızca senin istediğin işlerde
(döviz kuru, çeviri, web araması, indirmeler, günlük sürüm denetimi) internete
bağlanır. Kaynak kodun tamamı bu depoda.

Kurucunun test sürümünün [VirusTotal taraması](https://www.virustotal.com/gui/file/2e72ef0b882a7e761bbcce33dc5afdc82c5c74d329a3ad7bb52aabe735b28d78)
sonucunda 71 antivirüsten 69'u temiz buldu, Microsoft Defender da temiz
buldu. Yalnızca ESET'in genel "Kryptik" tahmini ve Bkav işaretledi; ikisi de
imzasız kurucularda sık görülen yanlış alarm türleri. İndirdiğin dosyanın
SHA-256 değerini Releases sayfasındakiyle karşılaştırabilirsin.

### İsteğe bağlı parçalar

- **CPU sıcaklığı:** Intel işlemcilerde sıcaklık için
  [PawnIO](https://pawnio.eu) sürücüsü gerekir. Kuruluysa kurucu "CPU
  sıcaklığı" seçeneğini açar; bir kez yönetici izni ister.
- **Tarayıcı eklentisi:** Tarayıcıda başlayan indirmeleri adaya taşır.
  `%LOCALAPPDATA%\Programs\CarlUI\extension\carlui-downloads` klasöründe gelir;
  tarayıcının eklentiler sayfasında geliştirici modunu açıp **Paketlenmemiş öğe
  yükle** ile bu klasörü seç. Ayrıntı: **Ayarlar → Arama → Tarayıcı eklentisi**.
- **Duvar kâğıdı:** Kurucuda "CarlUI duvar kâğıdı" seçeneği. Dosyalar
  [assets/wallpaper](assets/wallpaper) klasöründe (1920×1080, 2560×1440,
  3840×2160); `tools\make-wallpaper.ps1` ile yeniden üretilebilir.

### Güncelleme

CarlUI günde bir kez GitHub'da yeni sürüm olup olmadığına bakar; varsa adada
bildirim çıkar. Sen tıklamadıkça hiçbir şey olmaz. Tıklayınca CarlUI yeni
kurucuyu indirir, SHA-256 değerini sürümdekiyle karşılaştırır, sessizce kurar
ve yeniden başlar. Ayarların korunur. Bir sorun çıkarsa bildirim sürüm
sayfasına yönlendirir, oradan elle güncelleyebilirsin. Denetim **Ayarlar →
Genel → Güncellemeler** bölümünden kapatılabilir.

### Kaldırma

**Windows Ayarları → Uygulamalar → Yüklü uygulamalar** listesinden ya da Başlat menüsündeki **CarlUI'yi
kaldır** kısayolundan. Görev çubuğu geri gelir, kurulan dosyalar Geri Dönüşüm
Kutusu'na taşınır. Ayarların kalmasını ya da silinmesini sen seçersin.

**Acil durum:** Görev çubuğu bir şekilde geri gelmezse:

```powershell
& "$env:LOCALAPPDATA\Programs\CarlUI\carlui.exe" --restore-taskbar
```

## Ayarlar ve veriler

| Ne | Nerede |
|---|---|
| Program | `%LOCALAPPDATA%\Programs\CarlUI\` |
| Ayarlar | `%APPDATA%\CarlUI\config.toml` (Ayarlar penceresinden değiştirilir, anında uygulanır) |
| Pano geçmişi, önbellek | `%LOCALAPPDATA%\CarlUI\` |
| Loglar | `%LOCALAPPDATA%\CarlUI\logs\` (7 gün saklanır) |

## Kaynaktan derleme

Gerekenler: [Rust](https://rustup.rs) ve Visual Studio Build Tools (C++).

```powershell
cargo build --release
.\target\release\carlui.exe          # çalıştır
.\target\release\carlui.exe --quit   # kapat
```

Tek dosyalık kurucuyu üretmek için:

```powershell
powershell -ExecutionPolicy Bypass -File tools\build-setup.ps1   # → dist\CarlUI-Setup.exe
```

Mimari: her modül (dock, tepsi, ada, Win menüsü…) ayrı bir süreçtir; biri
çökerse ana süreç onu yeniden başlatır. Modüller adlandırılmış kanal üzerinden
konuşur. Arayüz metinleri kodda iki dilde yan yana durur:
`t!("Kapat", "Close")`. Ayrıntılı geliştirme notları ve test adımları:
[docs/GELISTIRME-NOTLARI.md](docs/GELISTIRME-NOTLARI.md).

## Lisans

MIT © 2026 Demhat Dayan. Ayrıntı: [LICENSE](LICENSE). Üçüncü taraf parçalar
(PawnIO IntelMSR modülü — LGPL-2.1, Rust kütüphaneleri) için:
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

Hata ve öneriler için [Issues](https://github.com/Carl00-mpeek/CarlUI/issues)
sayfasını kullanabilirsin. İletişim: maildemhat@gmail.com
