#!/usr/bin/env bash
# Run inside the Multipass Ubuntu VM after mounting the repo at /home/ubuntu/raptor
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive

echo "=== Installing build dependencies ==="
sudo apt-get update -qq
sudo apt-get install -y -qq build-essential pkg-config libssl-dev curl gpg gpgv ca-certificates git xz-utils

if ! command -v cargo >/dev/null 2>&1; then
  echo "=== Installing Rust ==="
  curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
fi
source "$HOME/.cargo/env"

cd /home/ubuntu/raptor
export CARGO_TARGET_DIR="$HOME/raptor-build"

echo "=== Building release binaries ==="
cargo build --release

echo "=== Running core tests (unit + integration that do not need host paths) ==="
cargo test -p raptor-core --lib
cargo test -p raptor-core --test acquire --test gpg_acquire --test secure_update

echo "=== Running local demo ==="
export RAPTOR_ROOT="$HOME/raptor-demo/root"
export RAPTOR_STATE="$HOME/raptor-demo/state.json"
export RAPTOR_CACHE="$HOME/raptor-demo/cache"
export RAPTOR_SOURCES="$HOME/raptor-demo/sources.list"
mkdir -p "$HOME/raptor-demo"
bash examples/demo.sh

echo ""
echo "=== Success ==="
echo "Binaries: $HOME/raptor-build/release/"
ls -la "$HOME/raptor-build/release/" | grep raptor
