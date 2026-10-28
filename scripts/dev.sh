#!/usr/bin/env bash
set -euo pipefail

# Navigate to project root
cd "$(dirname "$0")/.."

echo "==> Starting OpenLight Studio in development mode..."

# Ensure dependencies are installed
if [ ! -d "node_modules" ]; then
  echo "Installing npm dependencies..."
  npm install
fi

# Run Tauri dev (launches Vite dev server + native desktop window)
npm run tauri dev
