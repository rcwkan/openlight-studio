#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> Running Rust backend tests..."
cargo test --manifest-path src-tauri/Cargo.toml

echo "==> Verifying frontend TypeScript and Vite build..."
npm run build

echo "==> All tests and builds passed successfully!"
