#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if [ $# -lt 1 ]; then
  echo "Usage: $0 <path-to-photo-folder>"
  exit 1
fi

cargo run --manifest-path src-tauri/Cargo.toml -- --import "$1"
