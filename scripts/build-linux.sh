#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> Building Linux packages via Docker container..."

if ! docker image inspect openlight-builder >/dev/null 2>&1; then
  echo "Building openlight-builder docker image..."
  docker build -t openlight-builder -f scripts/Dockerfile.linux .
fi

docker run --rm \
  -v "$(pwd)":/app \
  -v /app/node_modules \
  -w /app \
  openlight-builder \
  ./scripts/build-linux-container.sh
