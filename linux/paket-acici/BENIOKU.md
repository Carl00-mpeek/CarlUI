# Carl Paket Açıcı

CachyOS (KDE Plasma, Wayland) için: Dolphin'de bir paket dosyasına **çift tıklayın**, gerisini o halletsin.

| Dosya | Ne yapar |
|---|---|
| `.deb`, `.rpm` | Önce aynı programı **resmi depoda** ve **AUR'da** arar, bulursa oradan kurmayı önerir (en sağlıklısı). Bulamazsa `.deb` için **debtap** ile Arch paketine dönüştürüp kurar, ya da içeriği bir klasöre çıkarır. |
| `.pkg.tar.zst` / `.pkg.tar.xz` | Paket bilgisini gösterir, onaylarsanız `pacman -U` ile kurar. |
| `.AppImage` | Bir kez çalıştırır **ya da** `~/Applications` klasörüne koyup simgesiyle Başlat menüsüne ekler. |
| `.flatpakref`, `.flatpakrepo`, `.flatpak` | Flatpak ile (kullanıcıya özel) kurar ya da depoyu ekler. |

Şifre gereken her işlem bir **Konsole** penceresinde açılır; `sudo` şifrenizi orada kendiniz girersiniz ve pacman'ın ne yaptığını görürsünüz.

## Kurulum

Bu klasörü CachyOS'a kopyalayın, içinde bir terminal açın:

```bash
chmod +x kur.sh kaldir.sh carl-paket-ac
./kur.sh
```

`sudo` ile çalıştırmayın. Eksik olan `kdialog`, `konsole`, `libarchive`, `curl` varsa kendisi kurar.

AUR desteği için `paru` önerilir (CachyOS'ta genelde hazır gelir):

```bash
sudo pacman -S paru
```

`.deb` dönüştürme isterseniz debtap gerekir; ilk kullanımda program sizin için kurmayı da önerir:

```bash
paru -S debtap
```

## Kullanım

- Dolphin'de dosyaya çift tıklayın.
- Ya da sağ tık → **Birlikte Aç** → **Carl Paket Açıcı**.
- Ya da terminalden: `carl-paket-ac dosya.deb`
- Argümansız çalıştırırsanız dosya seçme penceresi açılır.

Çift tıklama başka bir programı açıyorsa (ör. Ark ya da Discover): dosyaya sağ tık → **Özellikler** → dosya türü yanındaki ayar düğmesi → **Carl Paket Açıcı**'yı en üste taşıyın.

## Neden önce depo/AUR?

CachyOS Arch tabanlıdır, `.deb` ise Debian/Ubuntu için yapılmıştır. Dönüştürülmüş paketler bağımlılık adları farklı olduğu için bazen eksik ya da bozuk kurulur ve güncelleme almaz. Chrome, VS Code, Discord, Spotify gibi programların neredeyse hepsi depoda ya da AUR'da (`-bin` sonekiyle) bulunur; program bunu sizin yerinize kontrol eder.

AUR araması için yalnızca **paket adı** `aur.archlinux.org` adresine gönderilir.

## Kaldırma

```bash
./kaldir.sh
```

Bununla kurduğunuz programlar silinmez; onları her zamanki gibi kaldırın:

- pacman/AUR: `sudo pacman -R paket-adı`
- Flatpak: `flatpak uninstall uygulama-kimliği`
- AppImage: `~/Applications` içindeki dosyayı ve `~/.local/share/applications/appimage-*.desktop` dosyasını silin.
