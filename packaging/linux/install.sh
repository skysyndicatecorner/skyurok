#!/bin/sh
# Установка SkyUrok для текущего пользователя (без прав администратора).
set -e
DIR="$(cd "$(dirname "$0")" && pwd)"
BIN="$HOME/.local/bin"
APPS="$HOME/.local/share/applications"
ICONS="$HOME/.local/share/icons/hicolor/256x256/apps"
DATA="$HOME/.local/share/skyurok"
mkdir -p "$BIN" "$APPS" "$ICONS" "$DATA"
install -m 755 "$DIR/skyurok-app" "$DIR/skyurok" "$BIN/"
install -m 644 "$DIR/icon-256.png" "$ICONS/skyurok.png"
[ -f "$DIR/demo-school.json" ] && install -m 644 "$DIR/demo-school.json" "$DATA/"
sed "s|^Exec=skyurok-app|Exec=$BIN/skyurok-app|" "$DIR/skyurok.desktop" > "$APPS/skyurok.desktop"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APPS" >/dev/null 2>&1 || true
echo "Готово: SkyUrok появится в меню приложений (раздел «Образование»)."
echo "Запуск из терминала: $BIN/skyurok-app"
