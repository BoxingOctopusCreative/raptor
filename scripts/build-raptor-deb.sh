#!/usr/bin/env bash
# Build a .deb of the raptor binary using raptor pkg build.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RAPTOR_BIN="${RAPTOR_BIN:-$ROOT/target/release/raptor}"
RAPTOR_CLI="${RAPTOR_CLI:-$RAPTOR_BIN}"
VERSION="${VERSION:-0.1.0}"
WORKDIR="${WORKDIR:-$ROOT/packaging/staging}"
OUTPUT="${OUTPUT:-}"

if [[ ! -x "$RAPTOR_BIN" ]]; then
  echo "raptor binary not found at $RAPTOR_BIN (run: cargo build --release)" >&2
  exit 1
fi

if [[ -z "$OUTPUT" ]]; then
  ARCH="${ARCH:-$(dpkg --print-architecture 2>/dev/null || true)}"
  if [[ -z "$ARCH" ]]; then
    case "$(uname -m)" in
      aarch64|arm64) ARCH=arm64 ;;
      x86_64|amd64) ARCH=amd64 ;;
      *) ARCH="$(uname -m)" ;;
    esac
  fi
  OUTPUT="$ROOT/packaging/target/raptor_${VERSION}_${ARCH}.deb"
fi

ARCH="${ARCH:-$(basename "$OUTPUT" | sed -n 's/.*_\([^_]*\)\.deb$/\1/p')}"

rm -rf "$WORKDIR"
mkdir -p "$WORKDIR/data/bin" "$(dirname "$OUTPUT")"
cp "$RAPTOR_BIN" "$WORKDIR/data/bin/raptor"
chmod +x "$WORKDIR/data/bin/raptor"

sed -e "s/^  version: .*/  version: ${VERSION}/" \
    -e "s/^  architecture: .*/  architecture: ${ARCH}/" \
  "$ROOT/packaging/raptor.yaml" > "$WORKDIR/raptor.yaml"

(
  cd "$WORKDIR"
  "$RAPTOR_CLI" pkg build --manifest raptor.yaml --output "$OUTPUT"
)

echo "$OUTPUT"
