# Carl Vault — Linux (CachyOS, KDE Plasma, Wayland)

Linux'ta Carl Vault tek bir uygulamadır: kasa penceresi ile tarayıcı eklentisinin konuştuğu yerel köprü (`127.0.0.1:47321`) aynı süreçte çalışır. Windows'taki CarlUI adası burada yoktur. Kasa biçimi Windows'takiyle aynı olduğu için kasa iki sistem arasında taşınabilir.

Kod: `crates/carlui-vault-linux` (çalıştırılabilir dosyanın adı `carl-vault`).

## Neler var

- Kasa oluşturma, ana parola, kurtarma anahtarı, yanlış denemede bekletme, boşta otomatik kilit (uyku/uyanmada da kilitlenir).
- Kayıtlar: giriş, kart, not vb. kategoriler; alan ekleme ve silme; parola üretici ve güç göstergesi; canlı 2FA (TOTP) kodu; ek dosya (en çok 25 MB); kayda bağlı passkey'leri görme ve silme.
- Pano: kopyalanan değer KDE pano geçmişine (Klipper) yazılmaz ve ayarlanan süre sonra silinir (varsayılan 30 sn).
- Yedek: Windows'tan alınan şifreli `.carlvault` yedeğini geri yükleme ve yeni yedek alma.
- Tarayıcı: eklentiyle doldurma, yeni parolayı kaydetme ve passkey oluşturma ya da kullanma. İstekler kasa penceresinde onay kartı olarak görünür.
- İndirme yakalama Linux'ta yoktur; indirmeler tarayıcıda kalır.

## Hazır paket ile kurulum

[Releases](https://github.com/Carl00-mpeek/CarlUI/releases/latest) sayfasından `Carl-Vault-<sürüm>-linux-x86_64.tar.gz` dosyasını indir. Dosya x86_64 içindir ve glibc 2.28 ya da daha yenisini ister (CachyOS, Arch, Fedora, Ubuntu 20.04 ve sonrası).

```bash
tar -xzf Carl-Vault-*-linux-x86_64.tar.gz
```

```bash
cd Carl-Vault-*-linux-x86_64 && sh install.sh --autostart
```

`--autostart` uygulamanın oturum açılınca simge durumunda başlamasını sağlar; böylece eklenti köprüsü hep hazır olur. İstemiyorsan `sh install.sh` yeter. Güncellemek için yeni paketle aynı komutu çalıştır. Kaldırmak için `sh install.sh --uninstall` kullan; kasa silinmez.

Paket Windows'ta şu komutla üretilir: `powershell -ExecutionPolicy Bypass -File tools\build-linux.ps1` (gerekenler betiğin başında yazar).

## Derleme

```bash
sudo pacman -S --needed rust base-devel
```

```bash
git clone <depo> CarlUI && cd CarlUI
```

```bash
cargo build --release -p carlui-vault-linux
```

Kurulum (yalnızca kendi kullanıcın için):

```bash
install -Dm755 target/release/carl-vault ~/.local/bin/carl-vault
```

```bash
install -Dm644 crates/carlui-vault-linux/packaging/carl-vault.desktop ~/.local/share/applications/carl-vault.desktop
```

Oturum açılınca simge durumunda başlasın (isteğe bağlı; eklenti köprüsü böylece hep hazır olur):

```bash
install -Dm644 crates/carlui-vault-linux/packaging/carl-vault-autostart.desktop ~/.config/autostart/carl-vault.desktop
```

`~/.local/bin` PATH'te değilse `.desktop` dosyalarındaki `Exec=` satırına tam yolu yaz.

## Kasayı Windows'tan taşımak

İki yol var:

1. **Klasörü kopyala.** Windows'ta Carl Vault'u kapat, `%APPDATA%\CarlUI\vault` klasörünün tamamını Linux'ta `~/.local/share/CarlUI/vault` olarak kopyala. Ana parola ve kurtarma anahtarı aynı kalır.
2. **Yedekle.** Windows'ta Carl Vault → dışa aktar → `.carlvault` dosyası. Linux'ta yeni bir kasa oluştur, sonra Ayarlar → *Yedekten geri yükle…* ile dosyayı seç ve yedeğin parolasını yaz. Aynı kayıt iki kez yüklenmez.

Linux ayarları `~/.local/share/CarlUI/vault-linux.json` dosyasındadır. Eklenti eşleştirme belirteçleri `~/.local/share/CarlUI/bridge-tokens` dosyasındadır; bu dosyayı yalnızca sen okuyabilirsin.

## Tarayıcı eklentisi

Eklenti klasörü: `extension/carlui-downloads`. Windows'takiyle aynı eklentidir ve değişiklik gerekmez.

İlk kullanımda eklenti bağlanmak ister ve kasa penceresinde **Eklenti bağlantısı** kartı çıkar. Yalnızca kendi eklentinse *Onayla*'ya bas.

Kasa isteklerini yalnızca bu kullanıcıya ait bir tarayıcı sürecinden gelen bağlantılar yapabilir. Köprü, bağlantıyı açan süreci `/proc` üzerinden denetler. Tanınan tarayıcılar: Helium, Chromium, Chrome, Brave, Vivaldi, Edge, Firefox, LibreWolf, Floorp, Zen.

### Helium (paketlenmemiş yükleme)

1. `helium://extensions` (ya da `chrome://extensions`) sayfasını aç.
2. Sağ üstte **Geliştirici modu**'nu aç.
3. **Paketlenmemiş öğe yükle** → `extension/carlui-downloads` klasörünü seç.
4. Kısayol: `Ctrl+Shift+L`, ya da araç çubuğundaki CarlUI simgesi.

Helium'un kendi parola kaydetme önerisini kapatmak istersen: Ayarlar → Otomatik doldurma → Şifre yöneticisi.

### Firefox (kendi AMO hesabınla imzalama)

Kararlı Firefox imzasız eklentiyi kalıcı olarak yüklemez. Eklentiyi kendi Mozilla hesabınla, listelenmeyen (unlisted) olarak imzalarsın. Bu işlem eklentiyi mağazada yayımlamaz, yalnızca imzalı bir `.xpi` verir.

1. https://addons.mozilla.org/developers/addon/api/key/ adresinden **kendi** API anahtarını al (JWT issuer ve secret). Bu anahtarları kimseyle paylaşma ve depoya yazma.
2. `web-ext` aracını kur:

   ```bash
   sudo pacman -S --needed nodejs npm
   ```

   ```bash
   npm install --global web-ext
   ```

3. Anahtarları yalnızca o terminal oturumu için ortam değişkeni olarak ver ve imzala:

   ```bash
   cd extension/carlui-downloads
   ```

   ```bash
   read -rp "JWT issuer: " WEB_EXT_API_KEY; read -rsp "JWT secret: " WEB_EXT_API_SECRET; echo; export WEB_EXT_API_KEY WEB_EXT_API_SECRET
   ```

   ```bash
   web-ext sign --channel=unlisted --artifacts-dir ../../dist/firefox
   ```

4. `dist/firefox/*.xpi` dosyasını Firefox'a sürükle-bırak ya da `about:addons` → dişli → *Dosyadan eklenti yükle*.
5. `about:addons` → CarlUI → **İzinler** sekmesinde *Tüm web sitelerindeki verilerinize erişin* iznini aç. Firefox'ta MV3 site izinleri isteğe bağlıdır; bu izin olmadan doldurma çalışmaz.

Her yeni eklenti sürümünde `manifest.json` içindeki `version` artırılmalı; AMO aynı sürümü ikinci kez imzalamaz.

Sadece denemek için (kalıcı değil, Firefox kapanınca gider): `about:debugging` → *Bu Firefox* → *Geçici eklenti yükle* → `manifest.json`.

## Wayland notları

- **Pencereyi öne getirme:** Bir passkey ya da eşleştirme isteği gelince pencere öne gelmeyi ister. KWin odak çalmayı engelleyebilir; o zaman görev çubuğunda Carl Vault yanıp söner, tıkla.
- **Pano:** Wayland'de pano içeriği, onu koyan uygulama açıkken yaşar. Carl Vault'u kapatırsan kopyalanan değer de gider. *Pencere kapatılınca arka planda çalış* ayarı açıkken (varsayılan) kapatma düğmesi pencereyi yalnızca küçültür; tamamen kapatmak için **Çık**'a bas.
- **Sistem tepsisi simgesi yok:** Arka planda çalışırken uygulama görev çubuğunda küçültülmüş durur.
- Kopyalanan parolalar Klipper geçmişine yazılmaz (`x-kde-passwordManagerHint`).

## Sorun giderme

- Eklenti "Carl Vault'a bağlanılamadı" diyor: Carl Vault açık mı? Port dolu mu? `ss -ltnp | grep 47321` komutuyla bak. Terminalden `carl-vault` çalıştırınca köprü hataları terminale yazılır.
- "Bu istek tarayıcı eklentisinden gelmedi": tarayıcının çalıştırılabilir dosyası tanınan listede değildir Yeni adı ekle: `CARLUI_BROWSERS=adı carl-vault` (virgülle birden fazla; ad `ps -o comm= -p <pid>` çıktısıdır). Flatpak tarayıcı kendi ağ ad alanında çalışabilir; yerel paket (pacman/AUR) önerilir.
- Deneme için ayrı kasa ve port: `CARLUI_VAULT_DIR=/tmp/v CARLUI_BRIDGE_PORT=47399 carl-vault`.
