#!/bin/bash
set -euo pipefail

# Deploy Freenet Search Engine to a local Freenet node.
#
# Prerequisites:
#   - freenet, fdev, dx must be in PATH
#   - A local Freenet node must be running: freenet local
#   - web-container-tool must be built (for webapp signing)
#
# Usage:
#   ./scripts/deploy-local.sh

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WEB_CONTAINER_TOOL="$PROJECT_ROOT/target/release/web-container-tool"
WEB_CONTAINER_WASM="$PROJECT_ROOT/target/wasm32-unknown-unknown/release/web_container_contract.wasm"
DEPLOY_DIR="$PROJECT_ROOT/target/deploy"
WEBAPP_DIR="$PROJECT_ROOT/target/webapp"

cd "$PROJECT_ROOT"

echo "=== Freenet Search Engine — Local Deployment ==="
echo ""

# --- Step 0: Check prerequisites ---
echo "[0/8] Checking prerequisites..."

for cmd in fdev dx cargo tar xz; do
    if ! command -v "$cmd" &>/dev/null; then
        echo "ERROR: $cmd not found in PATH"
        exit 1
    fi
done

if [ ! -f "$WEB_CONTAINER_WASM" ]; then
    echo "ERROR: web_container_contract.wasm not found at $WEB_CONTAINER_WASM"
    echo "Build it: cargo build --release -p web-container-contract --target wasm32-unknown-unknown"
    exit 1
fi

if [ ! -f "$WEB_CONTAINER_TOOL" ]; then
    echo "ERROR: web-container-tool not found at $WEB_CONTAINER_TOOL"
    echo "Build it: cargo build --release -p web-container-tool"
    exit 1
fi

echo "  All prerequisites found."

# --- Step 1: Build contract WASMs ---
echo ""
echo "[1/8] Building contract WASMs..."
cargo build --release -p contract-catalog --target wasm32-unknown-unknown
cargo build --release -p contract-fulltext-shard --target wasm32-unknown-unknown
echo "  catalog:  $(du -h target/wasm32-unknown-unknown/release/contract_catalog.wasm | cut -f1)"
echo "  shard:    $(du -h target/wasm32-unknown-unknown/release/contract_fulltext_shard.wasm | cut -f1)"

# --- Step 2: Generate initial states and parameters ---
echo ""
echo "[2/8] Generating initial CBOR state/parameter files..."
mkdir -p "$DEPLOY_DIR"
cargo run -p deploy-helper -- "$DEPLOY_DIR" 2>/dev/null

# --- Step 3: Deploy catalog contract ---
echo ""
echo "[3/8] Deploying catalog contract..."
CATALOG_ID=$(fdev get-contract-id \
    --code target/wasm32-unknown-unknown/release/contract_catalog.wasm \
    --parameters "$DEPLOY_DIR/catalog-params.cbor")
echo "  Catalog contract ID: $CATALOG_ID"

fdev publish \
    --code target/wasm32-unknown-unknown/release/contract_catalog.wasm \
    --parameters "$DEPLOY_DIR/catalog-params.cbor" \
    contract \
    --state "$DEPLOY_DIR/catalog-state.cbor" || echo "  (may already exist)"

# --- Step 4: Deploy 16 shard contracts ---
echo ""
echo "[4/8] Deploying 16 shard contracts..."
SHARD_IDS=()
for i in $(seq 0 15); do
    SHARD_ID=$(fdev get-contract-id \
        --code target/wasm32-unknown-unknown/release/contract_fulltext_shard.wasm \
        --parameters "$DEPLOY_DIR/shard-${i}-params.cbor")
    SHARD_IDS+=("$SHARD_ID")
    echo "  Shard $i: $SHARD_ID"

    fdev publish \
        --code target/wasm32-unknown-unknown/release/contract_fulltext_shard.wasm \
        --parameters "$DEPLOY_DIR/shard-${i}-params.cbor" \
        contract \
        --state "$DEPLOY_DIR/shard-${i}-state.cbor" || echo "  (may already exist)"
done

# --- Step 5: Write contract IDs to config file ---
echo ""
echo "[5/8] Writing contract IDs to $DEPLOY_DIR/contract-ids.json..."
{
    echo "{"
    echo "  \"catalog\": \"$CATALOG_ID\","
    echo "  \"shards\": ["
    for i in $(seq 0 15); do
        comma=","
        [ "$i" -eq 15 ] && comma=""
        echo "    \"${SHARD_IDS[$i]}\"$comma"
    done
    echo "  ]"
    echo "}"
} > "$DEPLOY_DIR/contract-ids.json"
echo "  Contract IDs saved."
echo "  NOTE: Update ui/src/api/contracts.rs with these IDs for production use."

# --- Step 6: Build UI ---
echo ""
echo "[6/8] Building UI with dx..."
(cd ui && dx build --release 2>&1)
echo "  UI built successfully."

# --- Step 7: Package webapp ---
echo ""
echo "[7/8] Packaging webapp..."
mkdir -p "$WEBAPP_DIR"

# Find the dx output directory
DX_OUTPUT=""
for candidate in \
    "target/dx/freenet-search-engine/release/web/public" \
    "target/dx/freenet_search_engine/release/web/public" \
    "ui/target/dx/freenet-search-engine/release/web/public" \
    "ui/target/dx/freenet_search_engine/release/web/public"; do
    if [ -d "$candidate" ]; then
        DX_OUTPUT="$candidate"
        break
    fi
done

if [ -z "$DX_OUTPUT" ]; then
    echo "ERROR: Could not find dx build output directory"
    echo "Searched: target/dx/*/release/web/public"
    exit 1
fi

echo "  dx output: $DX_OUTPUT"
(cd "$DX_OUTPUT" && tar -cJf "$WEBAPP_DIR/webapp.tar.xz" *)

# Generate webapp signing keys if they don't exist
WEBAPP_KEYS="$DEPLOY_DIR/webapp-keys.toml"
if [ ! -f "$WEBAPP_KEYS" ]; then
    "$WEB_CONTAINER_TOOL" generate --output "$WEBAPP_KEYS"
    echo "  Generated webapp signing keys."
fi

# Sign the webapp
seconds=$(date +%s)
version=$(( seconds / 60 ))
"$WEB_CONTAINER_TOOL" sign \
    --input "$WEBAPP_DIR/webapp.tar.xz" \
    --output "$WEBAPP_DIR/webapp.metadata" \
    --parameters "$WEBAPP_DIR/webapp.parameters" \
    --key-file "$WEBAPP_KEYS" \
    --version "$version"
echo "  Webapp packaged and signed (version=$version)."

# --- Step 8: Deploy webapp contract ---
echo ""
echo "[8/8] Deploying webapp contract..."
WEBAPP_ID=$(fdev get-contract-id \
    --code "$WEB_CONTAINER_WASM" \
    --parameters "$WEBAPP_DIR/webapp.parameters")
echo "  Webapp contract ID: $WEBAPP_ID"

fdev publish \
    --code "$WEB_CONTAINER_WASM" \
    --parameters "$WEBAPP_DIR/webapp.parameters" \
    contract \
    --webapp-archive "$WEBAPP_DIR/webapp.tar.xz" \
    --webapp-metadata "$WEBAPP_DIR/webapp.metadata" || echo "  (may already exist)"

# --- Done ---
echo ""
echo "=========================================="
echo "Deployment complete!"
echo ""
echo "Webapp URL: http://127.0.0.1:7509/v1/contract/web/$WEBAPP_ID/"
echo ""
echo "Contract IDs:"
echo "  Catalog:  $CATALOG_ID"
for i in $(seq 0 15); do
    echo "  Shard $i: ${SHARD_IDS[$i]}"
done
echo "  Webapp:   $WEBAPP_ID"
echo "=========================================="
