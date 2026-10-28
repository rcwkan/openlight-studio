#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> Building OpenLight Studio for production..."

if [ ! -d "node_modules" ]; then
  echo "Installing npm dependencies..."
  npm install
fi

# Build native desktop application release bundle
npm run tauri build

echo "==> Build complete. Artifacts are located in src-tauri/target/release/bundle/"
