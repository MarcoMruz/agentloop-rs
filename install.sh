#!/usr/bin/env bash
# install.sh — Build and install the AgentLoop ACP bridge binary.
#
# Usage:
#   ./install.sh          # install to ~/.local/bin  (default)
#   INSTALL_DIR=/usr/local/bin ./install.sh
#
# After installing, load the Zed extension:
#   1. Open Zed → Extensions (Cmd+Shift+X)
#   2. Click "Install Dev Extension"
#   3. Select: <this repo>/crates/zed-agentloop

set -euo pipefail

INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
BINARY="agentloop-acp-bridge"

echo "==> Building $BINARY (release)..."
cargo build --release -p agentloop-acp-bridge

mkdir -p "$INSTALL_DIR"
cp "target/release/$BINARY" "$INSTALL_DIR/$BINARY"

echo "==> Installed: $INSTALL_DIR/$BINARY"

# Warn if INSTALL_DIR is not on PATH
if ! echo "$PATH" | tr ':' '\n' | grep -qxF "$INSTALL_DIR"; then
    echo
    echo "⚠  $INSTALL_DIR is not in your PATH."
    echo "   Add it to your shell profile, e.g.:"
    echo "     export PATH=\"$INSTALL_DIR:\$PATH\""
fi

echo
echo "==> Done!  Next steps:"
echo "   1. Make sure the AgentLoop Go server is running:"
echo "        cd ../agentloop && ./agentloop-server &"
echo "   2. Load the Zed extension (one-time, dev mode):"
echo "        Zed → Extensions → Install Dev Extension"
echo "        → select: $(pwd)/crates/zed-agentloop"
