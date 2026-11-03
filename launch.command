#!/usr/bin/env bash
# One-click launcher for macOS: double-click in Finder (close the Terminal window to quit).
cd "$(dirname "$0")" || exit 1
# Finder starts a bare shell: load nvm / Homebrew so `node` and `npm` are found
export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
[ -s "$NVM_DIR/nvm.sh" ] && . "$NVM_DIR/nvm.sh"
[ -x /opt/homebrew/bin/brew ] && eval "$(/opt/homebrew/bin/brew shellenv)"
command -v npm >/dev/null || { echo "Node.js not found - install it from https://nodejs.org"; read -r -p "Press Enter to close"; exit 1; }
./scripts/dev.sh
read -r -p "Press Enter to close"
