#!/usr/bin/env bash
# ==============================================================================
# deploy-cloudflare.sh — Deploy OpenLight Studio HTML to Cloudflare Pages & Worker Router
# Production URL: https://openlight.makecade.com/ or https://openlight-studio.pages.dev/
# ==============================================================================

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# shellcheck source=scripts/common.sh
source "${REPO_ROOT}/scripts/common.sh"

TARGET="${1:-prod}"
WRANGLER="npx --yes wrangler"
PROJECT_NAME="${CF_PAGES_PROJECT:-openlight-studio}"
OUT_DIR="${REPO_ROOT}/html"
CUSTOM_DOMAIN="${CUSTOM_DOMAIN:-https://openlight.makecade.com}"

extract_pages_url() {
  local raw="$1"
  local url
  url="$(printf '%s\n' "$raw" | grep -oE 'https://[^[:space:]]+\.pages\.dev' | tail -n 1 || true)"
  if [[ -n "$url" ]]; then
    printf '%s\n' "$url"
    return 0
  fi
  echo "https://${PROJECT_NAME}.pages.dev"
}

probe_http_code() {
  local url="$1"
  local retries=3
  local code="000"

  for ((i=1; i<=retries; i++)); do
    code="$(curl -s -o /dev/null -w "%{http_code}" -m 10 -L "$url" || true)"
    if [[ "$code" == "200" || "$code" == "301" || "$code" == "302" ]]; then
      break
    fi
    if [[ $i -lt $retries ]]; then
      sleep 2
    fi
  done
  echo "$code"
}

verify_endpoints() {
  local base_url="${1%/}"
  local pages_url="${2:-}"

  log_info "Verifying deployment on ${BOLD}${base_url}${RESET}..."

  local test_code
  test_code="$(probe_http_code "$base_url/")"

  local active_url="$base_url"
  if [[ "$test_code" != "200" && -n "$pages_url" ]]; then
    log_warn "Primary URL $base_url returned HTTP $test_code (DNS / Worker Route may still be propagating)."
    log_info "Testing direct Cloudflare Pages URL: ${BOLD}${pages_url}${RESET}..."
    active_url="$pages_url"
  fi

  local test_paths=(
    ""
    "icon.png"
  )

  for p in "${test_paths[@]}"; do
    local probe_url="$active_url"
    if [[ -n "$p" ]]; then
      probe_url="${active_url%/}/$p"
    fi
    local code
    code="$(probe_http_code "$probe_url")"
    if [[ "$code" == "200" || "$code" == "301" ]]; then
      echo -e "  ${GREEN}✓${RESET} [HTTP $code] $probe_url"
    else
      echo -e "  ${YELLOW}!${RESET} [HTTP $code] $probe_url"
    fi
  done
}

ensure_pages_project() {
  local cf_cfg="${HOME}/Library/Preferences/.wrangler/config/default.toml"
  if [[ -f "$cf_cfg" ]]; then
    local token
    token="$(grep -E '^oauth_token\s*=' "$cf_cfg" | head -n 1 | cut -d '"' -f 2 || true)"
    local account_id
    account_id="$(grep -E '^account_id\s*=' "$cf_cfg" 2>/dev/null | head -n 1 | cut -d '"' -f 2 || echo "6f39fdd62805c7bf2573c10e8156a9c1")"
    if [[ -n "$token" && -n "$account_id" ]]; then
      curl -s -X POST "https://api.cloudflare.com/client/v4/accounts/${account_id}/pages/projects" \
        -H "Authorization: Bearer $token" \
        -H "Content-Type: application/json" \
        -d "{\"name\":\"${PROJECT_NAME}\",\"production_branch\":\"main\"}" >/dev/null 2>&1 || true
    fi
  fi
}

echo -e "${BOLD}${CYAN}========================================================================${RESET}"
echo -e "${BOLD}${CYAN}  Deploying OpenLight Studio HTML to Cloudflare Pages & Router          ${RESET}"
echo -e "${BOLD}${CYAN}  Target: ${CUSTOM_DOMAIN}/                                             ${RESET}"
echo -e "${BOLD}${CYAN}========================================================================${RESET}"
echo ""

# Preflight checks
check_node

# 1. Configure Cloudflare Pages SPA Routing and Headers
log_info "Configuring Cloudflare Pages headers & routing fallback in ${OUT_DIR}/..."

cat << 'EOF' > "${OUT_DIR}/_headers"
/assets/*
  Cache-Control: public, max-age=31536000, immutable
/icon.png
  Cache-Control: public, max-age=31536000, immutable
/*
  X-Content-Type-Options: nosniff
  X-Frame-Options: SAMEORIGIN
  Referrer-Policy: strict-origin-when-cross-origin
EOF

cat << 'EOF' > "${OUT_DIR}/_redirects"
/*        /index.html        200
EOF

# 2. Deploy HTML folder to Cloudflare Pages
log_info "Deploying static output (${OUT_DIR}/) to Cloudflare Pages project '${PROJECT_NAME}'..."
ensure_pages_project

BRANCH_ARG="main"
case "$TARGET" in
  prod|production|--prod|main|"")
    BRANCH_ARG="main"
    ;;
  preview|--preview)
    BRANCH_ARG="preview"
    ;;
  *)
    log_error "Unknown target '$TARGET' (use 'preview' or 'prod')."
    exit 1
    ;;
esac

DEPLOY_OUTPUT="$($WRANGLER pages deploy "$OUT_DIR" --project-name="$PROJECT_NAME" --branch="$BRANCH_ARG" --commit-dirty=true 2>&1)"
echo "$DEPLOY_OUTPUT"
PAGES_DEPLOY_URL="$(extract_pages_url "$DEPLOY_OUTPUT")"

# 3. Deploy Cloudflare Worker Router
log_info "Deploying Cloudflare Worker Router [makecade-openlight-router]..."
if [[ -f "${REPO_ROOT}/cloudflare-worker/wrangler.json" ]]; then
  $WRANGLER deploy -c "${REPO_ROOT}/cloudflare-worker/wrangler.json" || {
    log_warn "Worker deployment reported an issue (zone routes might require active DNS for openlight.makecade.com)."
    log_info "You can configure or verify worker routes in the Cloudflare dashboard."
  }
fi

echo ""
echo -e "${GREEN}========================================================================${RESET}"
echo -e "${BOLD}${GREEN} ✓ OPENLIGHT STUDIO HTML DEPLOYED TO CLOUDFLARE!${RESET}"
echo -e "${GREEN}========================================================================${RESET}"
echo -e "Production Custom URL:       ${CYAN}${BOLD}${CUSTOM_DOMAIN}/${RESET}"
echo -e "Direct Cloudflare Pages URL: ${CYAN}${BOLD}${PAGES_DEPLOY_URL}/${RESET}"
echo -e "Cloudflare Worker Router:    ${CYAN}${BOLD}[makecade-openlight-router] (openlight.makecade.com/*)${RESET}"
echo -e "Architecture:                ${CYAN}${BOLD}Pure Static Landing + Edge Proxy Router${RESET}"
echo -e "${GREEN}========================================================================${RESET}"
echo ""

verify_endpoints "$CUSTOM_DOMAIN" "$PAGES_DEPLOY_URL"
log_success "Cloudflare deployment completed."
