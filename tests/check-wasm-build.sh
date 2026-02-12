#!/usr/bin/env bash
set -euo pipefail
echo "Checking WASM build targets..."
cargo check -p freenet-search-engine --target wasm32-unknown-unknown
cargo check -p contract-catalog --target wasm32-unknown-unknown
cargo check -p contract-fulltext-shard --target wasm32-unknown-unknown
echo "All WASM targets OK"
