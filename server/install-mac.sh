#!/usr/bin/env bash
set -euo pipefail

# ─────────────────────────────────────────────
# MCProxy Server — macOS one-click installer
# Detects deps, builds, registers launchd, starts
# ─────────────────────────────────────────────

SERVICE_LABEL="com.mcproxy.server"
INSTALL_DIR="$HOME/.mcproxy"
BIN_NAME="mcproxy-server"
BIND_ADDR="${BIND_ADDR:-0.0.0.0:9800}"
PLIST_FILE="$HOME/Library/LaunchAgents/${SERVICE_LABEL}.plist"
LOG_DIR="$INSTALL_DIR/logs"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

info()  { echo -e "${CYAN}[INFO]${NC} $*"; }
ok()    { echo -e "${GREEN}[  OK]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
fail()  { echo -e "${RED}[FAIL]${NC} $*"; exit 1; }

echo ""
echo -e "${CYAN}╔══════════════════════════════════════╗${NC}"
echo -e "${CYAN}║   MCProxy Server Installer (macOS)   ║${NC}"
echo -e "${CYAN}╚══════════════════════════════════════╝${NC}"
echo ""

# ── 1. Xcode command line tools ───────────
info "Checking Xcode CLI tools..."
if xcode-select -p &>/dev/null; then
    ok "Xcode CLI tools found"
else
    info "Installing Xcode CLI tools..."
    xcode-select --install 2>/dev/null || true
    echo "    Please complete the installation dialog, then re-run this script."
    exit 0
fi

# ── 2. Check & install Rust ──────────────
info "Checking Rust toolchain..."
if command -v cargo &>/dev/null; then
    ok "Rust found: $(cargo --version)"
elif [ -f "$HOME/.cargo/env" ]; then
    source "$HOME/.cargo/env"
    ok "Rust found: $(cargo --version)"
else
    info "Rust not found. Installing via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
    ok "Rust installed: $(cargo --version)"
fi

# ── 3. Build ─────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
info "Building release binary..."
cd "$SCRIPT_DIR"
cargo build --release 2>&1 | tail -3
ok "Build complete"

# ── 4. Install binary ───────────────────
info "Installing to $INSTALL_DIR ..."
mkdir -p "$INSTALL_DIR" "$LOG_DIR"
cp "$SCRIPT_DIR/target/release/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"
chmod 755 "$INSTALL_DIR/$BIN_NAME"
ok "Binary installed: $INSTALL_DIR/$BIN_NAME"

# ── 5. Stop existing service if running ──
if launchctl list | grep -q "$SERVICE_LABEL" 2>/dev/null; then
    info "Stopping existing service..."
    launchctl unload "$PLIST_FILE" 2>/dev/null || true
    ok "Existing service stopped"
fi

# ── 6. Register launchd service ──────────
info "Creating LaunchAgent..."
mkdir -p "$HOME/Library/LaunchAgents"

PORT=$(echo "$BIND_ADDR" | cut -d: -f2)

cat > "$PLIST_FILE" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>${SERVICE_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>${INSTALL_DIR}/${BIN_NAME}</string>
    </array>
    <key>EnvironmentVariables</key>
    <dict>
        <key>BIND_ADDR</key>
        <string>${BIND_ADDR}</string>
        <key>RUST_LOG</key>
        <string>info</string>
    </dict>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>${LOG_DIR}/stdout.log</string>
    <key>StandardErrorPath</key>
    <string>${LOG_DIR}/stderr.log</string>
</dict>
</plist>
PLIST
ok "LaunchAgent created: $PLIST_FILE"

# ── 7. Start ─────────────────────────────
info "Starting service..."
launchctl load "$PLIST_FILE"
sleep 1

if launchctl list | grep -q "$SERVICE_LABEL"; then
    ok "Service running"
else
    fail "Service failed to start. Check: cat $LOG_DIR/stderr.log"
fi

# ── 8. Verify ────────────────────────────
sleep 1
if lsof -i :"$PORT" -sTCP:LISTEN &>/dev/null; then
    ok "Port $PORT is listening"
else
    warn "Port $PORT not yet listening, may need a moment..."
fi

LOCAL_IP=$(ipconfig getifaddr en0 2>/dev/null || echo "YOUR_IP")

echo ""
echo -e "${GREEN}════════════════════════════════════════${NC}"
echo -e "${GREEN}  MCProxy server is running!${NC}"
echo -e "${GREEN}  Listening on: ${BIND_ADDR}${NC}"
echo -e "${GREEN}════════════════════════════════════════${NC}"
echo ""
echo "  Commands:"
echo "    launchctl list | grep mcproxy              # check status"
echo "    launchctl unload '$PLIST_FILE'             # stop"
echo "    launchctl load '$PLIST_FILE'               # start"
echo "    tail -f $LOG_DIR/stderr.log                # view logs"
echo ""
echo "  Clients connect to: ws://${LOCAL_IP}:${PORT}"
echo ""
echo "  Uninstall:"
echo "    launchctl unload '$PLIST_FILE'"
echo "    rm '$PLIST_FILE'"
echo "    rm -rf '$INSTALL_DIR'"
echo ""
