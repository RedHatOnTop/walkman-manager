#!/usr/bin/env bash
# The build itself, inside the Fedora 44 container (the Ubuntu 22.04 one for bundle).
#   check   rustfmt, clippy, tests
#   app     release binary in out/walkman-manager
#   all     both
#   bundle  deb, rpm and AppImage in out/bundle
set -euo pipefail
MODE="${1:-all}"
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TERM_COLOR=never
cd /src
mkdir -p out
# A container set up before rust-toolchain.toml named one gets it here; otherwise a no-op.
rustup -q toolchain install
echo "build: $(rustc --version), toolchain $(rustup show active-toolchain | cut -d' ' -f1)"
LOCKED=--locked
# Cargo has no local toolchain to keep the lockfile current, so it is settled here against
# the manifests (a minimal update) and copied back with the binaries.
cargo metadata --format-version 1 >/dev/null
cp Cargo.lock out/Cargo.lock

# The Windows helper that runs Music Center's analysis engine; uwm-core takes it in.
host() {
  command -v i686-w64-mingw32-gcc >/dev/null || dnf install -y -q mingw32-gcc >/dev/null
  i686-w64-mingw32-gcc -O2 -Wall -Werror -municode -static -s -o out/mmhost.exe \
    crates/uwm-core/mmhost/mmhost.c -lole32 -loleaut32
  export UWM_MMHOST_EXE=/src/out/mmhost.exe
}

frontend() {
  (cd app && npm ci --no-audit --no-fund --loglevel=error && npm run build)
}

check() {
  cargo fmt --all --check
  cargo clippy --workspace --all-targets $LOCKED -- -D warnings
  cargo test --workspace $LOCKED
}

app() {
  (cd app && npx tauri build --no-bundle --ci)
  cp target/release/walkman-manager out/walkman-manager
  cargo build --release $LOCKED -p uwm-cli
  cp target/release/uwm out/uwm
  sha256sum out/walkman-manager out/uwm out/mmhost.exe
}

# In the Ubuntu container: the deb, rpm and AppImage, with their own target directory so
# nothing built against Fedora's libraries is reused.
bundle() {
  export CARGO_TARGET_DIR=/root/target-bundle APPIMAGE_EXTRACT_AND_RUN=1
  (cd app && npx tauri build --ci)
  rm -rf out/bundle
  mkdir -p out/bundle
  cp "$CARGO_TARGET_DIR"/release/bundle/deb/*.deb "$CARGO_TARGET_DIR"/release/bundle/rpm/*.rpm \
    "$CARGO_TARGET_DIR"/release/bundle/appimage/*.AppImage out/bundle/
  (cd out/bundle && sha256sum -- *)
}

case "$MODE" in
  check) host; frontend; check ;;
  app) host; frontend; app ;;
  all) host; frontend; check; app ;;
  bundle) host; frontend; bundle ;;
  *) echo "unknown mode $MODE" >&2; exit 2 ;;
esac
