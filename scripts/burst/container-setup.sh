#!/usr/bin/env bash
# One-time setup of the Fedora 44 build container.
set -euo pipefail
dnf install -y -q \
  webkit2gtk4.1-devel gtk3-devel libsoup3-devel javascriptcoregtk4.1-devel librsvg2-devel \
  openssl-devel gcc gcc-c++ make pkgconf-pkg-config nodejs npm curl file rsync git >/dev/null
curl -fsSLo /tmp/rustup-init.sh https://sh.rustup.rs
sh /tmp/rustup-init.sh -y -q --profile minimal --default-toolchain none
bash "$(dirname "$0")/toolchain.sh"
echo "setup: node $(node --version), $("$HOME/.cargo/bin/rustc" --version)"
