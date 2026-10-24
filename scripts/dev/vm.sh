#!/usr/bin/env bash
# A throwaway VM from a distribution's cloud image, for trying the packages on a clean system.
#
#   vm.sh up NAME [USB_VID:PID]   boot a fresh copy (NAME is a folder under $UWM_VM_DIR holding
#                                 the verified cloud image), optionally with a USB device
#   vm.sh ssh NAME [COMMAND...]   run a command in it as `tester` (passwordless sudo)
#   vm.sh put NAME FILE...        copy files to ~tester
#   vm.sh get NAME PATH DEST      copy a file back
#   vm.sh down NAME               power it off
#
# Each VM's ssh port is derived from its name; UWM_VM_PORT overrides it (for every command)
# when two names land on the same one.
#
# Each `up` starts from the untouched image: the disk is an overlay on it, made again (over the
# last one) every time. The overlay stays in NAME/run after `down`.
set -euo pipefail
DIR="${UWM_VM_DIR:-$HOME/.cache/uwm-vm}"
CMD="${1:?usage: vm.sh up|ssh|put|get|down NAME ...}"
NAME="${2:?name}"
shift 2
VM="$DIR/$NAME"
RUN="$VM/run"
KEY="$DIR/id_ed25519"
PORT=${UWM_VM_PORT:-$((2250 + $(printf '%s' "$NAME" | cksum | cut -d' ' -f1) % 40))}
SSH=(ssh -q -i "$KEY" -p "$PORT" -o IdentitiesOnly=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR)

case "$CMD" in
  up)
    base=$(find "$VM" -maxdepth 1 \( -name '*.img' -o -name '*.qcow2' \) | head -1)
    [[ -n $base ]] || { echo "vm.sh: no cloud image in $VM" >&2; exit 2; }
    [[ -f $KEY ]] || ssh-keygen -q -t ed25519 -N '' -C uwm-vm -f "$KEY"
    if [[ -s $RUN/pid ]] && kill -0 "$(cat "$RUN/pid")" 2>/dev/null; then
      echo "vm.sh: $NAME is already up" >&2
      exit 2
    fi
    if ss -Hltn "sport = :$PORT" | grep -q .; then
      echo "vm.sh: port $PORT is taken; set UWM_VM_PORT" >&2
      exit 2
    fi
    mkdir -p "$RUN"
    qemu-img create -q -f qcow2 -F qcow2 -b "$base" "$RUN/disk.qcow2" 24G
    cat > "$RUN/user-data" <<EOF
#cloud-config
users:
  - name: tester
    sudo: ALL=(ALL) NOPASSWD:ALL
    shell: /bin/bash
    ssh_authorized_keys: [$(cat "$KEY.pub")]
EOF
    printf 'instance-id: %s-%s\nlocal-hostname: %s\n' "$NAME" "$(date +%s)" "$NAME" > "$RUN/meta-data"
    genisoimage -quiet -output "$RUN/seed.iso" -volid cidata -joliet -rock "$RUN/user-data" "$RUN/meta-data"
    usb=()
    if [[ -n ${1:-} ]]; then
      usb=(-device qemu-xhci -device "usb-host,vendorid=0x${1%%:*},productid=0x${1##*:}")
    fi
    qemu-system-x86_64 -enable-kvm -cpu host -smp 4 -m "${UWM_VM_MEM:-3072}" \
      -drive "file=$RUN/disk.qcow2,if=virtio" -drive "file=$RUN/seed.iso,if=virtio,media=cdrom" \
      -nic "user,model=virtio-net-pci,hostfwd=tcp:127.0.0.1:$PORT-:22" "${usb[@]}" \
      -display none -serial "file:$RUN/serial.log" -pidfile "$RUN/pid" -daemonize
    for _ in $(seq 1 90); do
      if "${SSH[@]}" tester@127.0.0.1 'cloud-init status --wait >/dev/null 2>&1; true' 2>/dev/null; then
        echo "vm.sh: $NAME up on port $PORT"
        exit 0
      fi
      sleep 4
    done
    echo "vm.sh: $NAME did not answer on ssh; see $RUN/serial.log" >&2
    exit 1
    ;;
  ssh) exec "${SSH[@]}" tester@127.0.0.1 "$@" ;;
  put) exec scp -q -i "$KEY" -P "$PORT" -o IdentitiesOnly=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null "$@" tester@127.0.0.1: ;;
  get) exec scp -q -i "$KEY" -P "$PORT" -o IdentitiesOnly=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null "tester@127.0.0.1:$1" "$2" ;;
  down) [[ ! -s $RUN/pid ]] || kill "$(cat "$RUN/pid")" 2>/dev/null || true ;;
  *) echo "vm.sh: unknown command $CMD" >&2; exit 2 ;;
esac
