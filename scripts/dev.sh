#!/usr/bin/env bash
set -euo pipefail

# Navigate to project root
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

# Verify cargo is installed
if ! command -v cargo >/dev/null 2>&1; then
  echo "Error: 'cargo' not found in PATH."
  echo "Please install Rust from https://rustup.rs or run: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
  exit 1
fi

echo "==> Starting OpenLight Studio in development mode..."

# Ensure dependencies are installed
if [ ! -d "node_modules" ]; then
  echo "Installing npm dependencies..."
  npm install
fi

# Run Tauri dev (launches Vite dev server + native desktop window)
npx tauri dev
