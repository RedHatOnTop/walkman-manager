#!/usr/bin/env bash
# One-time setup of the Ubuntu 22.04 packaging container. The packages are built on the
# oldest base they support (glibc 2.35, WebKitGTK 4.1), so one binary runs on newer
# Ubuntu, Debian and Fedora alike.
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq \
  libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev librsvg2-dev \
  libssl-dev build-essential pkg-config gcc-mingw-w64-i686 curl ca-certificates file rsync git \
  xz-utils >/dev/null
# Ubuntu 22.04's own Node is 12; Vite needs a current one.
curl -fsSL https://deb.nodesource.com/setup_22.x | bash - >/dev/null
apt-get install -y -qq nodejs >/dev/null
curl -fsSLo /tmp/rustup-init.sh https://sh.rustup.rs
sh /tmp/rustup-init.sh -y -q --profile minimal --default-toolchain none
bash "$(dirname "$0")/toolchain.sh"
echo "setup: node $(node --version), $("$HOME/.cargo/bin/rustc" --version), $(ldd --version | head -1)"
