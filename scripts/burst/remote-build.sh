#!/usr/bin/env bash
# Runs on the aws-burst runner. Builds inside a long-lived Fedora 44 container so
# the binary links against the same WebKitGTK as the Fedora desktop it runs on,
# and so cargo and npm caches survive between runs. The packages come from a second
# container on Ubuntu 22.04, the oldest base they support.
set -euo pipefail
MODE="${1:-all}"
cd "$(dirname "$0")/../.."
if ! command -v docker >/dev/null 2>&1; then
  sudo dnf install -y -q docker >/dev/null
fi
sudo systemctl start docker
if [[ $MODE == bundle ]]; then
  NAME=uwm-bundle IMAGE=docker.io/library/ubuntu:22.04 SETUP=container-setup-ubuntu.sh
else
  NAME=uwm-build IMAGE=registry.fedoraproject.org/fedora:44 SETUP=container-setup.sh
fi
if ! sudo docker inspect "$NAME" >/dev/null 2>&1; then
  sudo docker run -d --name "$NAME" -v "$PWD:/src" -w /src "$IMAGE" sleep infinity
  sudo docker exec "$NAME" bash "/src/scripts/burst/$SETUP"
fi
sudo docker exec "$NAME" bash /src/scripts/burst/container-build.sh "$MODE"
