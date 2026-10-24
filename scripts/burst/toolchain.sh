#!/usr/bin/env bash
# The toolchain rust-toolchain.toml names, then its components one at a time. rustup 1.29.2
# (2026-10-21), installing clippy and rustfmt together from the file, stopped on "detected
# conflict: 'bin/cargo-fmt'" and left both listed without their binaries.
set -euo pipefail
cd "$(dirname "$0")/../.."
R="$HOME/.cargo/bin/rustup"
channel=$(sed -n 's/^channel = "\(.*\)"$/\1/p' rust-toolchain.toml)
"$R" -q toolchain install "$channel" --profile minimal
for c in $(sed -n 's/^components = \[\(.*\)\]$/\1/p' rust-toolchain.toml | tr -d '",'); do
  "$R" -q component add --toolchain "$channel" "$c"
done
