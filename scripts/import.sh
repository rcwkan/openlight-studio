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

if [ $# -lt 1 ]; then
  echo "Usage: $0 <path-to-photo-folder>"
  exit 1
fi

cargo run --manifest-path src-tauri/Cargo.toml -- --import "$1"
