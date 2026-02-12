#!/bin/bash
set -euo pipefail

# Stop the local Freenet network started by network-start.sh.
#
# After stopping, you can go back to single-node mode:
#   freenet local
#
# Usage:
#   ./scripts/network-stop.sh

FREENET_CORE="$HOME/Dev/Freenet/freenet-core"
MAKEFILE="$FREENET_CORE/scripts/local-network.mk"

if [ -f "$MAKEFILE" ]; then
    make -f "$MAKEFILE" -C "$FREENET_CORE/scripts" stop N_GATEWAYS=1 N_NODES=1 2>&1
else
    echo "Makefile not found, killing processes directly..."
    pkill -9 -f "freenet network" 2>/dev/null || true
fi

echo ""
echo "Network stopped. To revert to single-node mode:"
echo "  freenet local"
