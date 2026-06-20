#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEMO="$ROOT/examples/demo"
REPO="$DEMO/repo"
PKG="$DEMO/pkg"
DEB="$DEMO/hello-raptor_0.1.0_all.deb"

export RAPTOR_ROOT="$DEMO/root"
export RAPTOR_STATE="$DEMO/state.json"
export RAPTOR_CACHE="$DEMO/cache"
export RAPTOR_SOURCES="$DEMO/sources.list"

rm -rf "$DEMO"
mkdir -p "$PKG/DEBIAN" "$PKG/usr/local/bin" "$RAPTOR_ROOT" "$RAPTOR_CACHE"

cat > "$PKG/DEBIAN/control" <<'EOF'
Package: hello-raptor
Version: 0.1.0
Architecture: all
Maintainer: Demo <demo@example.com>
Description: Hello from Raptor
EOF

cat > "$PKG/usr/local/bin/hello-raptor" <<'EOF'
#!/bin/sh
echo "Hello from Raptor!"
EOF
chmod +x "$PKG/usr/local/bin/hello-raptor"

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
BIN="$CARGO_TARGET_DIR/debug"

cargo build --quiet --manifest-path "$ROOT/Cargo.toml"

"$BIN/raptor" pkg build --root "$PKG" --output "$DEB"
"$BIN/raptor" repo create --kind private --root "$REPO" --suite stable --component main
"$BIN/raptor" pkg publish "$DEB" --repo "$REPO" --suite stable --arch all

echo "deb file:$REPO stable main" > "$RAPTOR_SOURCES"

"$BIN/raptor" -y repo update
"$BIN/raptor" -y pkg get hello-raptor

test -x "$RAPTOR_ROOT/usr/local/bin/hello-raptor"
"$RAPTOR_ROOT/usr/local/bin/hello-raptor"

echo "Demo OK"
