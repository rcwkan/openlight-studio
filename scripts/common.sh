#!/usr/bin/env bash
# Common helper functions for OpenLight Studio local scripts

# Resolve absolute paths
SCRIPTS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPTS_DIR}/.." && pwd)"

# Terminal colors
if [ -t 1 ]; then
  RED='\033[0;31m'
  GREEN='\033[0;32m'
  YELLOW='\033[1;33m'
  BLUE='\033[0;34m'
  CYAN='\033[0;36m'
  BOLD='\033[1m'
  RESET='\033[0m'
else
  RED=''
  GREEN=''
  YELLOW=''
  BLUE=''
  CYAN=''
  BOLD=''
  RESET=''
fi

log_info() {
  echo -e "${BLUE}ℹ${RESET} $1"
}

log_success() {
  echo -e "${GREEN}✔${RESET} $1"
}

log_warn() {
  echo -e "${YELLOW}⚠${RESET} $1"
}

log_error() {
  echo -e "${RED}✖${RESET} $1" >&2
}

log_step() {
  echo -e "\n${BOLD}${CYAN}==>${RESET} ${BOLD}$1${RESET}"
}

# Check Node.js and npm existence
check_node() {
  if ! command -v node >/dev/null 2>&1; then
    log_error "Node.js is not installed or not in PATH."
    exit 1
  fi

  if ! command -v npm >/dev/null 2>&1; then
    log_error "npm is not installed or not in PATH."
    exit 1
  fi
}
