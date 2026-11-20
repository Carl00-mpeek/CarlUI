#!/usr/bin/env bash
# Carl Paket Açıcı'yı kaldırır (kurduğunuz programlara dokunmaz).
set -u
VERI="${XDG_DATA_HOME:-$HOME/.local/share}"

rm -f "$HOME/.local/bin/carl-paket-ac" \
      "$VERI/applications/carl-paket-acici.desktop" \
      "$VERI/mime/packages/carl-paket.xml"

# mimeapps.list içindeki varsayılan atamaları temizle
for f in "${XDG_CONFIG_HOME:-$HOME/.config}/mimeapps.list" "$VERI/applications/mimeapps.list"; do
    [ -f "$f" ] && sed -i '/carl-paket-acici\.desktop/d' "$f"
done

update-mime-database "$VERI/mime" >/dev/null 2>&1 || true
update-desktop-database "$VERI/applications" >/dev/null 2>&1 || true
command -v kbuildsycoca6 >/dev/null 2>&1 && kbuildsycoca6 >/dev/null 2>&1 || true
rm -rf "${XDG_CACHE_HOME:-$HOME/.cache}/carl-paket"

echo "✔ Carl Paket Açıcı kaldırıldı."
