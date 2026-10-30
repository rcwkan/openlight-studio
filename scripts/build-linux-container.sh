#!/usr/bin/env bash
set -euo pipefail

echo "==> [Linux Container] Setting up target directory..."
export CARGO_TARGET_DIR=/tmp/linux-target
mkdir -p "$CARGO_TARGET_DIR"

echo "==> [Linux Container] Installing Linux npm dependencies..."
npm install

echo "==> [Linux Container] Building frontend..."
npm run build

echo "==> [Linux Container] Building Linux package (.deb)..."
npx tauri build --bundles deb || npx tauri build --no-bundle

echo "==> [Linux Container] Copying output artifacts to dist-packages/linux..."
mkdir -p /app/dist-packages/linux
if [ -d "$CARGO_TARGET_DIR/release/bundle" ]; then
  cp -r "$CARGO_TARGET_DIR"/release/bundle/* /app/dist-packages/linux/ 2>/dev/null || true
fi
if [ -f "$CARGO_TARGET_DIR/release/openlight-studio" ]; then
  cp "$CARGO_TARGET_DIR/release/openlight-studio" /app/dist-packages/linux/openlight-studio-linux
fi

echo "==> [Linux Container] Build complete! Output files in dist-packages/linux:"
ls -lh /app/dist-packages/linux
