#!/bin/sh
# Carl Vault'u yalnızca bu kullanıcı için kurar (sudo gerekmez).
#
#   sh install.sh               kur ya da güncelle
#   sh install.sh --autostart   ayrıca oturum açılınca simge durumunda başlat
#   sh install.sh --uninstall   kaldır (kasa ve ayarlar silinmez)
set -eu

here=$(cd "$(dirname "$0")" && pwd)
bin="$HOME/.local/bin/carl-vault"
apps="${XDG_DATA_HOME:-$HOME/.local/share}/applications/carl-vault.desktop"
auto="${XDG_CONFIG_HOME:-$HOME/.config}/autostart/carl-vault.desktop"

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$bin" "$apps" "$auto"
    echo "Carl Vault kaldırıldı. Kasa ~/.local/share/CarlUI içinde duruyor."
    exit 0
fi

install -Dm755 "$here/carl-vault" "$bin"
# Exec= tam yolu gösterir; ~/.local/bin PATH'te olmasa da menüden açılır.
mkdir -p "$(dirname "$apps")"
sed "s|^Exec=carl-vault|Exec=$bin|" "$here/carl-vault.desktop" > "$apps"
if [ "${1:-}" = "--autostart" ]; then
    mkdir -p "$(dirname "$auto")"
    sed "s|^Exec=carl-vault|Exec=$bin|" "$here/carl-vault-autostart.desktop" > "$auto"
fi
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q "$(dirname "$apps")" || true

echo "Carl Vault kuruldu: $bin"
echo "Uygulama menüsünde \"Carl Vault\" olarak görünür."
