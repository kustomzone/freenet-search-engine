#!/bin/bash
set -euo pipefail

# Deploy Freenet Search Engine to a local network (started by network-start.sh).
# Deploys contracts + webapp + test apps to the gateway node (port 3001).
#
# Prerequisites:
#   - ./scripts/network-start.sh must have been run
#   - fdev, dx must be in PATH
#   - River's web-container-tool must be built
#
# To revert to single-node mode:
#   ./scripts/network-stop.sh
#   freenet local
#   ./scripts/deploy-local.sh
#
# Usage:
#   ./scripts/deploy-network.sh

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RIVER_ROOT="$HOME/Dev/Freenet/river"
WEB_CONTAINER_TOOL="$RIVER_ROOT/target/native/x86_64-unknown-linux-gnu/release/web-container-tool"
WEB_CONTAINER_WASM="$RIVER_ROOT/published-contract/web_container_contract.wasm"
DEPLOY_DIR="$PROJECT_ROOT/target/deploy"
WEBAPP_DIR="$PROJECT_ROOT/target/webapp"
DIOXUS_TOML="$PROJECT_ROOT/Dioxus.toml"

# Port for the gateway node (everything deployed here)
GW_PORT=3001

cd "$PROJECT_ROOT"

echo "=== Freenet Search Engine — Network Deployment ==="
echo "  Gateway: port $GW_PORT"
echo ""

# --- Check prerequisites ---
echo "[0/10] Checking prerequisites..."
for cmd in fdev dx cargo tar xz; do
    if ! command -v "$cmd" &>/dev/null; then
        echo "ERROR: $cmd not found in PATH"
        exit 1
    fi
done

if [ ! -f "$WEB_CONTAINER_WASM" ]; then
    echo "ERROR: web_container_contract.wasm not found at $WEB_CONTAINER_WASM"
    exit 1
fi
if [ ! -f "$WEB_CONTAINER_TOOL" ]; then
    echo "ERROR: web-container-tool not found at $WEB_CONTAINER_TOOL"
    exit 1
fi
echo "  All prerequisites found."

# --- Step 1: Build contract WASMs ---
echo ""
echo "[1/10] Building contract WASMs..."
cargo build --release -p contract-catalog --target wasm32-unknown-unknown
cargo build --release -p contract-fulltext-shard --target wasm32-unknown-unknown

# --- Step 2: Generate initial states ---
echo ""
echo "[2/10] Generating CBOR state/parameter files..."
mkdir -p "$DEPLOY_DIR"
cargo run -p deploy-helper -- "$DEPLOY_DIR" 2>/dev/null

# --- Step 3: Deploy catalog to gateway ---
echo ""
echo "[3/10] Deploying catalog contract to gateway (port $GW_PORT)..."
CATALOG_WASM="target/wasm32-unknown-unknown/release/contract_catalog.wasm"
fdev network -p "$GW_PORT" publish \
    --code "$CATALOG_WASM" \
    --parameters "$DEPLOY_DIR/catalog-params.cbor" \
    contract \
    --state "$DEPLOY_DIR/catalog-state.cbor" 2>&1 | grep -E "Publishing|published|updated" || true

# --- Step 4: Deploy 16 shards to gateway ---
echo ""
echo "[4/10] Deploying 16 shard contracts to gateway..."
SHARD_WASM="target/wasm32-unknown-unknown/release/contract_fulltext_shard.wasm"
for i in $(seq 0 15); do
    fdev network -p "$GW_PORT" publish \
        --code "$SHARD_WASM" \
        --parameters "$DEPLOY_DIR/shard-${i}-params.cbor" \
        contract \
        --state "$DEPLOY_DIR/shard-${i}-state.cbor" 2>&1 | grep -E "Publishing|published|updated" || true
done
echo "  All 16 shards deployed."

# --- Step 5: Compute webapp ID (before building UI, so we can set base_path) ---
echo ""
echo "[5/10] Computing webapp contract ID..."
mkdir -p "$WEBAPP_DIR"
WEBAPP_KEYS="$DEPLOY_DIR/webapp-keys.toml"
if [ ! -f "$WEBAPP_KEYS" ]; then
    "$WEB_CONTAINER_TOOL" generate --output "$WEBAPP_KEYS"
fi

# Pre-sign a dummy file to generate the parameters file.
# Parameters depend only on the signing key, not the content.
if [ ! -f "$WEBAPP_DIR/webapp.parameters" ]; then
    echo "bootstrap" > /tmp/webapp-bootstrap.tar.xz
    "$WEB_CONTAINER_TOOL" sign \
        --input /tmp/webapp-bootstrap.tar.xz \
        --output /tmp/webapp-bootstrap.metadata \
        --parameters "$WEBAPP_DIR/webapp.parameters" \
        --key-file "$WEBAPP_KEYS" \
        --version 1 2>/dev/null
    rm -f /tmp/webapp-bootstrap.tar.xz /tmp/webapp-bootstrap.metadata
fi

WEBAPP_ID=$(fdev get-contract-id \
    --code "$WEB_CONTAINER_WASM" \
    --parameters "$WEBAPP_DIR/webapp.parameters")
echo "  Webapp ID: $WEBAPP_ID"

# --- Step 6: Build UI with correct base_path ---
echo ""
echo "[6/10] Building UI with dx (base_path set for web container)..."

# Set base_path in Dioxus.toml so asset paths resolve correctly
# under /v1/contract/web/{key}/ (River does the same thing).
sed -i '/^base_path/d' "$DIOXUS_TOML"
sed -i "/\[web\.app\]/a base_path = \"v1/contract/web/$WEBAPP_ID\"" "$DIOXUS_TOML"

(cd ui && dx build --release 2>&1)

# Clean up: remove base_path so dx serve still works for local dev
sed -i '/^base_path/d' "$DIOXUS_TOML"

echo "  UI built."

# --- Step 7: Package webapp ---
echo ""
echo "[7/10] Packaging webapp..."

DX_OUTPUT="target/dx/freenet-search-engine/release/web/public"
if [ ! -d "$DX_OUTPUT" ]; then
    echo "ERROR: dx output not found at $DX_OUTPUT"
    exit 1
fi

(cd "$DX_OUTPUT" && tar -cJf "$WEBAPP_DIR/webapp.tar.xz" *)

seconds=$(date +%s)
version=$(( seconds / 60 ))
"$WEB_CONTAINER_TOOL" sign \
    --input "$WEBAPP_DIR/webapp.tar.xz" \
    --output "$WEBAPP_DIR/webapp.metadata" \
    --parameters "$WEBAPP_DIR/webapp.parameters" \
    --key-file "$WEBAPP_KEYS" \
    --version "$version"
echo "  Webapp packaged (version=$version)."

# --- Step 8: Deploy webapp to gateway ---
echo ""
echo "[8/10] Deploying webapp to gateway (port $GW_PORT)..."
fdev network -p "$GW_PORT" publish \
    --code "$WEB_CONTAINER_WASM" \
    --parameters "$WEBAPP_DIR/webapp.parameters" \
    contract \
    --webapp-archive "$WEBAPP_DIR/webapp.tar.xz" \
    --webapp-metadata "$WEBAPP_DIR/webapp.metadata" 2>&1 | grep -E "Publishing|published|updated" || true

# --- Step 9: Publish test web apps to gateway ---
echo ""
echo "[9/10] Publishing test web apps to gateway (port $GW_PORT)..."
TEST_DIR="$PROJECT_ROOT/target/test-apps"
mkdir -p "$TEST_DIR"

for app_dir in "$PROJECT_ROOT"/tests/integration/test-app-*; do
    [ -d "$app_dir" ] || continue
    app_name=$(basename "$app_dir")
    out="$TEST_DIR/$app_name"
    mkdir -p "$out"

    # Package
    (cd "$app_dir" && tar -cJf "$out/webapp.tar.xz" *)

    # Generate keys if needed
    if [ ! -f "$out/keys.toml" ]; then
        "$WEB_CONTAINER_TOOL" generate --output "$out/keys.toml"
    fi

    # Sign
    test_version=$(( $(date +%s) / 60 ))
    "$WEB_CONTAINER_TOOL" sign \
        --input "$out/webapp.tar.xz" \
        --output "$out/webapp.metadata" \
        --parameters "$out/webapp.parameters" \
        --key-file "$out/keys.toml" \
        --version "$test_version" 2>/dev/null

    test_id=$(fdev get-contract-id \
        --code "$WEB_CONTAINER_WASM" \
        --parameters "$out/webapp.parameters")

    # Publish to gateway (same node as search engine, for immediate discovery)
    fdev network -p "$GW_PORT" publish \
        --code "$WEB_CONTAINER_WASM" \
        --parameters "$out/webapp.parameters" \
        contract \
        --webapp-archive "$out/webapp.tar.xz" \
        --webapp-metadata "$out/webapp.metadata" 2>&1 | grep -E "Publishing|published|updated" || true

    echo "  $app_name: $test_id"
done

# --- Step 10: Summary ---
echo ""
echo "[10/10] Verifying..."

# Check webapp is accessible on gateway
gw_status=$(curl -s -o /dev/null -w "%{http_code}" "http://127.0.0.1:$GW_PORT/v1/contract/web/$WEBAPP_ID/" 2>/dev/null || echo "000")

echo ""
echo "=========================================="
echo "Deployment complete!"
echo ""
echo "Search engine: http://127.0.0.1:$GW_PORT/v1/contract/web/$WEBAPP_ID/"
echo "  Gateway status: HTTP $gw_status"
echo ""
echo "Test apps published to gateway for immediate discovery."
echo ""
echo "To stop:  ./scripts/network-stop.sh"
echo "To revert to single-node: ./scripts/network-stop.sh && freenet local"
echo "=========================================="
