#!/bin/sh
# Удаление SkyUrok, установленного через install.sh. Ваши проекты не удаляются.
rm -f "$HOME/.local/bin/skyurok-app" "$HOME/.local/bin/skyurok" \
      "$HOME/.local/share/applications/skyurok.desktop" \
      "$HOME/.local/share/icons/hicolor/256x256/apps/skyurok.png"
echo "SkyUrok удалён. Настройки и автосохранения остались в ~/.local/share/skyurok"
