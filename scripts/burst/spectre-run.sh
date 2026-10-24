#!/usr/bin/env bash
# Runs on Spectre. Keeps one warm aws-burst runner for the copy of this repository
# it sits in, syncs the tree to it, runs remote-build.sh there and copies out/ back.
# Each copy (Projects/walkman-manager, Projects/walkman-manager-<x>) has its own runner,
# so two branches built at once do not swap trees on one.
#
#   spectre-run.sh [check|app|all|bundle]     (default: all)
#   spectre-run.sh down                       terminate the runner
# UWM_REPO names another copy of the tree on Spectre, such as a worktree synced there.
set -uo pipefail
export PATH="$HOME/.local/bin:$PATH"
REPO="${UWM_REPO:-$(cd "$(dirname "$0")/../.." && pwd)}"
COPY="$(basename "$REPO")"
STATE="$HOME/.cache/uwm-burst/runner${COPY#walkman-manager}"
MODE="${1:-all}"
TYPE="${UWM_BURST_TYPE:-m7i-flex.large}"
mkdir -p "$(dirname "$STATE")"

if [[ $MODE == down ]]; then
  [[ -s $STATE ]] && aws-burst down "$(cut -d' ' -f1 "$STATE")"
  : > "$STATE"
  exit 0
fi

alive() {
  [[ -s $STATE ]] || return 1
  local ip
  ip=$(cut -d' ' -f2 "$STATE")
  # shellcheck disable=SC2046
  ssh $(aws-burst sshopts) "ec2-user@$ip" true >/dev/null 2>&1
}

if ! alive; then
  json=$(aws-burst up --type "$TYPE" --ttl 300 --idle 45 --name "$COPY") || exit 3
  python3 -c 'import json,sys; d=json.loads(sys.argv[1]); print(d["id"], d["ip"])' "$json" > "$STATE"
fi
read -r ID IP < "$STATE"
SSHOPTS="$(aws-burst sshopts)"

rsync -az --delete --exclude target --exclude node_modules --exclude dist --exclude out --exclude .git --exclude app/src-tauri/gen \
  -e "ssh $SSHOPTS" "$REPO/" "ec2-user@$IP:uwm/" || { echo "spectre-run: sync to $ID failed" >&2; exit 6; }
# shellcheck disable=SC2086
ssh $SSHOPTS "ec2-user@$IP" "bash uwm/scripts/burst/remote-build.sh $MODE"
rc=$?
mkdir -p "$REPO/out"
# shellcheck disable=SC2086
rsync -az -e "ssh $SSHOPTS" "ec2-user@$IP:uwm/out/" "$REPO/out/" || true
echo "spectre-run: $MODE finished with status $rc on $ID" >&2
exit $rc
