#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

# If OpenLight Studio is running, it connects to the running window.
# If closed, it runs headlessly directly against the catalog.
cargo run --manifest-path src-tauri/Cargo.toml -- --mcp-stdio "$@"
