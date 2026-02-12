#!/bin/bash
set -euo pipefail

# Publish test web apps to a local Freenet node for integration testing.
#
# Prerequisites:
#   - fdev must be in PATH
#   - web-container-tool must be built (cargo build --release -p web-container-tool)
#   - web-container-contract WASM must be built (cargo build --release -p web-container-contract --target wasm32-unknown-unknown)
#   - A local Freenet node must be running (freenet local)
#
# Usage:
#   ./tests/integration/publish-test-apps.sh

PROJECT_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WEB_CONTAINER_TOOL="$PROJECT_ROOT/target/release/web-container-tool"
WEB_CONTAINER_WASM="$PROJECT_ROOT/target/wasm32-unknown-unknown/release/web_container_contract.wasm"
BUILD_DIR="$PROJECT_ROOT/target/test-apps"

echo "=== Publishing Test Web Apps ==="
echo ""

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

mkdir -p "$BUILD_DIR"

# Publish each test app
for app_dir in "$SCRIPT_DIR"/test-app-*; do
    app_name=$(basename "$app_dir")
    echo "--- Publishing $app_name ---"

    out="$BUILD_DIR/$app_name"
    mkdir -p "$out"

    # 1. Create tar.xz from app directory
    (cd "$app_dir" && tar -cJf "$out/webapp.tar.xz" *)
    echo "  Packaged: $(du -h "$out/webapp.tar.xz" | cut -f1)"

    # 2. Generate a unique signing keypair for this test app
    keys_file="$out/keys.toml"
    if [ ! -f "$keys_file" ]; then
        "$WEB_CONTAINER_TOOL" generate --output "$keys_file"
        echo "  Generated signing keys"
    fi

    # 3. Sign the webapp (version based on timestamp for uniqueness)
    seconds=$(date +%s)
    version=$(( seconds / 60 ))
    "$WEB_CONTAINER_TOOL" sign \
        --input "$out/webapp.tar.xz" \
        --output "$out/webapp.metadata" \
        --parameters "$out/webapp.parameters" \
        --key-file "$keys_file" \
        --version "$version"
    echo "  Signed (version=$version)"

    # 4. Get the contract ID
    contract_id=$(fdev get-contract-id \
        --code "$WEB_CONTAINER_WASM" \
        --parameters "$out/webapp.parameters")
    echo "  Contract ID: $contract_id"

    # 5. Publish to local node
    fdev publish \
        --code "$WEB_CONTAINER_WASM" \
        --parameters "$out/webapp.parameters" \
        contract \
        --webapp-archive "$out/webapp.tar.xz" \
        --webapp-metadata "$out/webapp.metadata" \
    && echo "  Published successfully!" \
    || echo "  (may already exist)"

    # 6. Verify it's accessible
    url="http://127.0.0.1:7509/v1/contract/web/$contract_id/"
    status=$(curl -s -o /dev/null -w "%{http_code}" "$url" 2>/dev/null || echo "000")
    if [ "$status" = "200" ]; then
        echo "  Verified: $url (HTTP 200)"
    else
        echo "  WARNING: $url returned HTTP $status"
    fi

    echo "  $contract_id" >> "$BUILD_DIR/published-ids.txt"
    echo ""
done

echo "=== Done ==="
echo ""
echo "Published contract IDs:"
cat "$BUILD_DIR/published-ids.txt" 2>/dev/null || true
echo ""
echo "The search engine should discover these within ~10 seconds"
echo "(diagnostics polling interval)."
echo ""
echo "To verify:"
echo "  1. Open the search engine in a browser"
echo "  2. Wait for discovery to complete"
echo "  3. Look for 'Freenet Weather Dashboard' and 'CryptoNotes' in the app directory"
echo "  4. Enable contribution in Settings to index them"
echo "  5. Search for 'weather', 'encrypted', 'notes', etc."
