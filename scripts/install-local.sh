#!/usr/bin/env bash
# Installs a built binary for the current user: ~/.local/bin/walkman-manager, an icon and
# a desktop entry, so the app starts from the launcher like any other.
#
#   scripts/install-local.sh [path/to/walkman-manager]   (default: out/remote/walkman-manager)
set -euo pipefail
cd "$(dirname "$0")/.."
BIN="${1:-out/remote/walkman-manager}"
ID=dev.personnya.walkman-manager
[[ -x $BIN ]] || { echo "install-local: no executable at $BIN" >&2; exit 1; }

mkdir -p ~/.local/bin ~/.local/share/applications ~/.local/share/icons/hicolor/256x256/apps ~/.local/share/icons/hicolor/scalable/apps
install -m 0755 "$BIN" ~/.local/bin/walkman-manager.new
mv -f ~/.local/bin/walkman-manager.new ~/.local/bin/walkman-manager
install -m 0644 app/src-tauri/icons/128x128@2x.png ~/.local/share/icons/hicolor/256x256/apps/$ID.png
install -m 0644 app/src-tauri/icons/icon.svg ~/.local/share/icons/hicolor/scalable/apps/$ID.svg
cat > ~/.local/share/applications/$ID.desktop <<DESKTOP
[Desktop Entry]
Type=Application
Name=Ultimate Walkman Manager
Name[ko]=Ultimate Walkman Manager
Comment=Manage Sony Walkman players: devices, music library and sync
Comment[ko]=소니 워크맨 관리: 기기, 음악 라이브러리, 동기화
Exec=$HOME/.local/bin/walkman-manager
Icon=$ID
Terminal=false
Categories=AudioVideo;Audio;
StartupWMClass=walkman-manager
DESKTOP
update-desktop-database ~/.local/share/applications >/dev/null 2>&1 || true
gtk-update-icon-cache -q ~/.local/share/icons/hicolor >/dev/null 2>&1 || true
echo "installed $(sha256sum ~/.local/bin/walkman-manager | cut -c1-16) to ~/.local/bin/walkman-manager"
