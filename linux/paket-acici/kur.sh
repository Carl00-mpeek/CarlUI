#!/usr/bin/env bash
# Carl Paket Açıcı kurulumu. sudo ile ÇALIŞTIRMAYIN; yalnız kendi kullanıcınız için kurulur.
set -euo pipefail

if [ "$(id -u)" -eq 0 ]; then
    echo "Lütfen sudo olmadan çalıştırın: ./kur.sh"
    exit 1
fi

KAYNAK="$(cd "$(dirname "$0")" && pwd)"
VERI="${XDG_DATA_HOME:-$HOME/.local/share}"
BIN="$HOME/.local/bin"

echo "== Gerekli programlar denetleniyor"
eksik=()
for p in kdialog konsole bsdtar curl; do
    command -v "$p" >/dev/null 2>&1 || eksik+=("$p")
done
if [ ${#eksik[@]} -gt 0 ]; then
    paketler=()
    for p in "${eksik[@]}"; do
        case "$p" in bsdtar) paketler+=(libarchive) ;; *) paketler+=("$p") ;; esac
    done
    echo "Eksik: ${eksik[*]} -> sudo pacman -S --needed ${paketler[*]}"
    sudo pacman -S --needed "${paketler[@]}"
fi

echo "== Dosyalar kopyalanıyor"
install -Dm755 "$KAYNAK/carl-paket-ac"            "$BIN/carl-paket-ac"
install -Dm644 "$KAYNAK/carl-paket-acici.desktop" "$VERI/applications/carl-paket-acici.desktop"
# Oturum PATH'inde ~/.local/bin olmayabilir; tam yolu yaz.
sed -i "s|^Exec=carl-paket-ac |Exec=$BIN/carl-paket-ac |" "$VERI/applications/carl-paket-acici.desktop"
install -Dm644 "$KAYNAK/carl-paket.xml"           "$VERI/mime/packages/carl-paket.xml"

echo "== Dosya türleri kaydediliyor"
update-mime-database "$VERI/mime" >/dev/null 2>&1 || true
update-desktop-database "$VERI/applications" >/dev/null 2>&1 || true

turler=(
    application/vnd.debian.binary-package application/x-deb application/x-debian-package
    application/x-rpm application/x-redhat-package-manager
    application/x-alpm-package
    application/vnd.appimage application/x-iso9660-appimage application/x-appimage
    application/vnd.flatpak.ref application/vnd.flatpak.repo application/vnd.flatpak
)
for t in "${turler[@]}"; do
    xdg-mime default carl-paket-acici.desktop "$t"
done
command -v kbuildsycoca6 >/dev/null 2>&1 && kbuildsycoca6 >/dev/null 2>&1 || true

case ":$PATH:" in
    *":$BIN:"*) ;;
    *) echo "Not: $BIN PATH içinde değil. Çift tıklama yine de çalışır; terminalden kullanmak için oturumu kapatıp açın." ;;
esac

echo
echo "✔ Kuruldu. Artık Dolphin'de .deb, .rpm, .pkg.tar.zst, .AppImage ve .flatpakref dosyalarına çift tıklayabilirsiniz."
if ! command -v paru >/dev/null 2>&1 && ! command -v yay >/dev/null 2>&1; then
    echo "İpucu: AUR'dan kurabilmek için paru önerilir: sudo pacman -S paru"
fi
