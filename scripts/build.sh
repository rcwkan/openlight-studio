#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

# Ensure Rust/Cargo, Homebrew and Node are in PATH
if [ -f "$HOME/.cargo/env" ]; then
  # shellcheck source=/dev/null
  . "$HOME/.cargo/env"
fi
for extra_dir in "$HOME/.cargo/bin" "/opt/homebrew/bin" "/usr/local/bin"; do
  if [ -d "$extra_dir" ] && [[ ":$PATH:" != *":$extra_dir:"* ]]; then
    export PATH="$extra_dir:$PATH"
  fi
done

if ! command -v cargo >/dev/null 2>&1; then
  echo "Error: 'cargo' not found in PATH."
  echo "Please install Rust from https://rustup.rs"
  exit 1
fi

echo "==> Building OpenLight Studio for production..."

if [ ! -d "node_modules" ]; then
  echo "Installing npm dependencies..."
  npm install
fi

# Detect OS
OS_NAME="$(uname -s)"

if [ "$OS_NAME" = "Darwin" ]; then
  echo "==> Building macOS Application Bundle (.app)..."
  npx tauri build -b app

  APP_PATH="src-tauri/target/release/bundle/macos/OpenLight Studio.app"
  DMG_DIR="src-tauri/target/release/bundle/dmg"
  ARCH="$(uname -m)"
  DMG_PATH="${DMG_DIR}/OpenLight Studio_0.1.0_${ARCH}.dmg"

  mkdir -p "$DMG_DIR"
  echo "==> Packaging macOS DMG installer with hdiutil..."
  rm -f "$DMG_PATH"
  hdiutil create -volname "OpenLight Studio" -srcfolder "$APP_PATH" -ov -format UDZO "$DMG_PATH"

  mkdir -p "dist-packages/macos"
  cp -R "$APP_PATH" "dist-packages/macos/"
  cp "$DMG_PATH" "dist-packages/macos/"

  echo "==> macOS build complete! Artifacts:"
  echo "    - .app: dist-packages/macos/OpenLight Studio.app"
  echo "    - .dmg: dist-packages/macos/OpenLight Studio_0.1.0_${ARCH}.dmg"
else
  # Linux or other OS
  npx tauri build
fi

echo "==> Build finished successfully."
