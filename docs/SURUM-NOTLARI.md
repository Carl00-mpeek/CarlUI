# Sürüm notları

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
