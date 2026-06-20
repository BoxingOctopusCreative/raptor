#!/usr/bin/env bash
# Host-side helper: ensure the Multipass VM is running and ready for Raptor testing.
set -euo pipefail

VM_NAME="${RAPTOR_VM_NAME:-test-vm}"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

if ! command -v multipass >/dev/null 2>&1; then
  echo "multipass not found; install from https://multipass.run" >&2
  exit 1
fi

state="$(multipass info "$VM_NAME" 2>/dev/null | awk -F': ' '/State/ {print $2}' || true)"
if [[ -z "$state" ]]; then
  echo "Launching $VM_NAME (Ubuntu 24.04, 20G disk)..."
  multipass launch 24.04 --name "$VM_NAME" --cpus 2 --memory 4G --disk 20G
elif [[ "$state" != "Running" ]]; then
  echo "Starting $VM_NAME..."
  multipass start "$VM_NAME"
fi

if ! multipass info "$VM_NAME" | grep -q 'Mount.*raptor'; then
  echo "Mounting $REPO_ROOT -> /home/ubuntu/raptor"
  multipass mount "$REPO_ROOT" "$VM_NAME:/home/ubuntu/raptor"
fi

ip="$(multipass info "$VM_NAME" | awk -F': ' '/IPv4/ {print $2; exit}')"
echo ""
echo "VM: $VM_NAME ($ip)"
echo "Shell:  multipass shell $VM_NAME"
echo "Setup:  multipass exec $VM_NAME -- bash /home/ubuntu/raptor/scripts/vm-setup.sh"
echo "Demo:   multipass exec $VM_NAME -- bash -lc 'source ~/.cargo/env && export CARGO_TARGET_DIR=\$HOME/raptor-build && bash /home/ubuntu/raptor/examples/demo.sh'"
