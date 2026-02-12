#!/bin/bash
set -euo pipefail

# Quick redeploy of the search engine webapp to a running local network.
# Rebuilds UI, repackages, and publishes to gateway. Skips contract builds.
#
# Prerequisites:
#   - Gateway must be running (./scripts/network-start.sh)
#   - First full deploy must have been done (./scripts/deploy-network.sh)
#     so that webapp-keys.toml, webapp.parameters, etc. exist
#
# Usage:
#   ./scripts/redeploy-webapp.sh           # rebuild + deploy
#   ./scripts/redeploy-webapp.sh --skip-build  # repackage + deploy only (no dx build)

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WEB_CONTAINER_TOOL="$PROJECT_ROOT/target/release/web-container-tool"
WEB_CONTAINER_WASM="$PROJECT_ROOT/target/wasm32-unknown-unknown/release/web_container_contract.wasm"
DEPLOY_DIR="$PROJECT_ROOT/target/deploy"
WEBAPP_DIR="$PROJECT_ROOT/target/webapp"
WEBAPP_KEYS="$DEPLOY_DIR/webapp-keys.toml"
DIOXUS_TOML="$PROJECT_ROOT/Dioxus.toml"
DX_OUTPUT="$PROJECT_ROOT/target/dx/freenet-search-engine/release/web/public"
GW_PORT=3001

SKIP_BUILD=false
if [[ "${1:-}" == "--skip-build" ]]; then
    SKIP_BUILD=true
fi

cd "$PROJECT_ROOT"

# Check that first deploy was done
for f in "$WEBAPP_KEYS" "$WEBAPP_DIR/webapp.parameters"; do
    if [ ! -f "$f" ]; then
        echo "ERROR: $f not found. Run ./scripts/deploy-network.sh first."
        exit 1
    fi
done

WEBAPP_ID=$(fdev get-contract-id \
    --code "$WEB_CONTAINER_WASM" \
    --parameters "$WEBAPP_DIR/webapp.parameters")

echo "=== Redeploy Webapp ==="
echo "  ID: $WEBAPP_ID"
echo ""

# --- Build ---
if [ "$SKIP_BUILD" = false ]; then
    echo "[1/3] Building UI..."
    sed -i '/^base_path/d' "$DIOXUS_TOML"
    sed -i "/\[web\.app\]/a base_path = \"v1/contract/web/$WEBAPP_ID\"" "$DIOXUS_TOML"

    (cd ui && dx build --release 2>&1)

    sed -i '/^base_path/d' "$DIOXUS_TOML"
    echo "  Done."
else
    echo "[1/3] Skipping build (--skip-build)."
fi

# --- Package ---
echo ""
echo "[2/3] Packaging..."

if [ ! -d "$DX_OUTPUT" ]; then
    echo "ERROR: dx output not found at $DX_OUTPUT"
    echo "  Run without --skip-build, or run dx build manually."
    exit 1
fi

(cd "$DX_OUTPUT" && tar -cJf "$WEBAPP_DIR/webapp.tar.xz" *)

version=$(( $(date +%s) / 60 ))
"$WEB_CONTAINER_TOOL" sign \
    --input "$WEBAPP_DIR/webapp.tar.xz" \
    --output "$WEBAPP_DIR/webapp.metadata" \
    --parameters "$WEBAPP_DIR/webapp.parameters" \
    --key-file "$WEBAPP_KEYS" \
    --version "$version"
echo "  Packaged (version=$version)."

# --- Publish ---
echo ""
echo "[3/3] Publishing to gateway (port $GW_PORT)..."

fdev network -p "$GW_PORT" publish \
    --code "$WEB_CONTAINER_WASM" \
    --parameters "$WEBAPP_DIR/webapp.parameters" \
    contract \
    --webapp-archive "$WEBAPP_DIR/webapp.tar.xz" \
    --webapp-metadata "$WEBAPP_DIR/webapp.metadata" 2>&1 \
    | grep -v "^$" | tail -5

echo ""
echo "Done! http://127.0.0.1:$GW_PORT/v1/contract/web/$WEBAPP_ID/"
