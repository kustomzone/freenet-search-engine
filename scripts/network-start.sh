#!/bin/bash
set -euo pipefail

# Start a local Freenet network (1 gateway + 1 node) for integration testing.
# The gateway runs on WS port 3001, the node on WS port 3002.
#
# To revert to single-node mode:
#   ./scripts/network-stop.sh
#   freenet local
#
# Usage:
#   ./scripts/network-start.sh

FREENET_CORE="$HOME/Dev/Freenet/freenet-core"
MAKEFILE="$FREENET_CORE/scripts/local-network.mk"

if [ ! -f "$MAKEFILE" ]; then
    echo "ERROR: local-network.mk not found at $MAKEFILE"
    exit 1
fi

# Stop any existing freenet processes
echo "Stopping any existing freenet processes..."
pkill -9 -f "freenet local" 2>/dev/null || true
pkill -9 -f "freenet network" 2>/dev/null || true
sleep 2

echo "Setting up local network (1 gateway + 1 node)..."
make -f "$MAKEFILE" -C "$FREENET_CORE/scripts" setup N_GATEWAYS=1 N_NODES=1 2>&1

echo ""
echo "Starting local network..."
make -f "$MAKEFILE" -C "$FREENET_CORE/scripts" start N_GATEWAYS=1 N_NODES=1 2>&1

echo ""
echo "Network is running:"
echo "  Gateway: ws://127.0.0.1:3001 (for search engine)"
echo "  Node:    ws://127.0.0.1:3002 (for test app publishing)"
echo ""
echo "To stop: ./scripts/network-stop.sh"
