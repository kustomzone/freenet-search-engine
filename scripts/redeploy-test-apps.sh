#!/bin/bash
set -euo pipefail

# Redeploy test web apps to a running local network gateway.
# Repackages and publishes each test app in tests/integration/test-app-*.
#
# Prerequisites:
#   - Gateway must be running (./scripts/network-start.sh)
#
# Usage:
#   ./scripts/redeploy-test-apps.sh

PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WEB_CONTAINER_TOOL="$PROJECT_ROOT/target/release/web-container-tool"
WEB_CONTAINER_WASM="$PROJECT_ROOT/target/wasm32-unknown-unknown/release/web_container_contract.wasm"
TEST_DIR="$PROJECT_ROOT/target/test-apps"
GW_PORT=3001

cd "$PROJECT_ROOT"

# Check prerequisites
for cmd in fdev tar xz; do
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

mkdir -p "$TEST_DIR"
> "$TEST_DIR/published-ids.txt"

echo "=== Redeploy Test Apps ==="
echo "  Gateway: port $GW_PORT"
echo ""

count=0
for app_dir in "$PROJECT_ROOT"/tests/integration/test-app-*; do
    [ -d "$app_dir" ] || continue
    app_name=$(basename "$app_dir")
    out="$TEST_DIR/$app_name"
    mkdir -p "$out"

    echo "--- $app_name ---"

    # Package
    (cd "$app_dir" && tar -cJf "$out/webapp.tar.xz" *)

    # Generate keys if needed
    if [ ! -f "$out/keys.toml" ]; then
        "$WEB_CONTAINER_TOOL" generate --output "$out/keys.toml"
        echo "  Generated signing keys"
    fi

    # Sign with minute-based version for monotonic increase
    version=$(( $(date +%s) / 60 ))
    "$WEB_CONTAINER_TOOL" sign \
        --input "$out/webapp.tar.xz" \
        --output "$out/webapp.metadata" \
        --parameters "$out/webapp.parameters" \
        --key-file "$out/keys.toml" \
        --version "$version" 2>/dev/null

    # Get contract ID
    contract_id=$(fdev get-contract-id \
        --code "$WEB_CONTAINER_WASM" \
        --parameters "$out/webapp.parameters")

    # Publish to gateway
    fdev network -p "$GW_PORT" publish \
        --code "$WEB_CONTAINER_WASM" \
        --parameters "$out/webapp.parameters" \
        contract \
        --webapp-archive "$out/webapp.tar.xz" \
        --webapp-metadata "$out/webapp.metadata" 2>&1 \
        | grep -E "published|updated|error" || true

    url="http://127.0.0.1:$GW_PORT/v1/contract/web/$contract_id/"
    status=$(curl -s -o /dev/null -w "%{http_code}" "$url" 2>/dev/null || echo "000")
    echo "  $contract_id  HTTP $status"
    echo "$contract_id" >> "$TEST_DIR/published-ids.txt"
    echo ""

    count=$((count + 1))
done

echo "=== Done: $count test apps published ==="
echo ""
echo "Published IDs:"
cat "$TEST_DIR/published-ids.txt"
echo ""
echo "Apps should appear in the search engine within ~10s (diagnostics poll)."
