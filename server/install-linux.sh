#!/usr/bin/env bash
set -euo pipefail

# ─────────────────────────────────────────────
# MCProxy Server — Linux one-click installer
# Detects deps, builds, registers systemd, starts
# ─────────────────────────────────────────────

SERVICE_NAME="mcproxy"
INSTALL_DIR="/opt/mcproxy"
BIN_NAME="mcproxy-server"
BIND_ADDR="${BIND_ADDR:-0.0.0.0:9800}"
SERVICE_FILE="/etc/systemd/system/${SERVICE_NAME}.service"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

info()  { echo -e "${CYAN}[INFO]${NC} $*"; }
ok()    { echo -e "${GREEN}[  OK]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
fail()  { echo -e "${RED}[FAIL]${NC} $*"; exit 1; }

# ── Root check ─────────────────────────────
if [ "$(id -u)" -ne 0 ]; then
    fail "Please run with sudo: sudo bash $0"
fi

REAL_USER="${SUDO_USER:-$(whoami)}"
REAL_HOME=$(eval echo "~${REAL_USER}")

echo ""
echo -e "${CYAN}╔══════════════════════════════════════╗${NC}"
echo -e "${CYAN}║   MCProxy Server Installer (Linux)   ║${NC}"
echo -e "${CYAN}╚══════════════════════════════════════╝${NC}"
echo ""

# ── 1. Check & install Rust ────────────────
info "Checking Rust toolchain..."
if sudo -u "$REAL_USER" bash -c 'source "$HOME/.cargo/env" 2>/dev/null; command -v cargo' &>/dev/null; then
    CARGO_VER=$(sudo -u "$REAL_USER" bash -c 'source "$HOME/.cargo/env" 2>/dev/null; cargo --version')
    ok "Rust found: $CARGO_VER"
else
    info "Rust not found. Installing via rustup..."
    sudo -u "$REAL_USER" bash -c 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y'
    ok "Rust installed"
fi

CARGO="sudo -u $REAL_USER bash -c 'source \$HOME/.cargo/env 2>/dev/null; cargo'"

# ── 2. Check system deps ──────────────────
info "Checking system dependencies..."
MISSING_PKGS=()

for cmd in gcc make pkg-config; do
    if ! command -v "$cmd" &>/dev/null; then
        MISSING_PKGS+=("$cmd")
    fi
done

if [ ${#MISSING_PKGS[@]} -gt 0 ]; then
    info "Installing missing packages: ${MISSING_PKGS[*]}"
    if command -v apt-get &>/dev/null; then
        apt-get update -qq
        apt-get install -y -qq build-essential pkg-config libssl-dev
    elif command -v dnf &>/dev/null; then
        dnf install -y gcc make pkg-config openssl-devel
    elif command -v yum &>/dev/null; then
        yum install -y gcc make pkgconfig openssl-devel
    elif command -v pacman &>/dev/null; then
        pacman -Sy --noconfirm base-devel openssl pkg-config
    else
        fail "Unknown package manager. Please install: build-essential pkg-config libssl-dev"
    fi
    ok "System dependencies installed"
else
    ok "System dependencies present"
fi

# ── 3. Build ──────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
info "Building release binary..."
cd "$SCRIPT_DIR"
sudo -u "$REAL_USER" bash -c "source \$HOME/.cargo/env 2>/dev/null; cd '$SCRIPT_DIR' && cargo build --release" 2>&1 | tail -3
ok "Build complete"

# ── 4. Install binary ────────────────────
info "Installing to $INSTALL_DIR ..."
mkdir -p "$INSTALL_DIR"
cp "$SCRIPT_DIR/target/release/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"
chmod 755 "$INSTALL_DIR/$BIN_NAME"
ok "Binary installed: $INSTALL_DIR/$BIN_NAME"

# ── 5. Create service user ───────────────
if ! id "$SERVICE_NAME" &>/dev/null; then
    useradd -r -s /bin/false "$SERVICE_NAME" 2>/dev/null || true
    ok "Service user created: $SERVICE_NAME"
else
    ok "Service user exists: $SERVICE_NAME"
fi

# ── 6. Register systemd service ──────────
if [ -f "$SERVICE_FILE" ]; then
    warn "Service file already exists: $SERVICE_FILE"
    info "Updating binary and restarting..."
else
    info "Creating systemd service..."
    cat > "$SERVICE_FILE" <<UNIT
[Unit]
Description=MCProxy Relay Server
After=network.target

[Service]
Type=simple
User=${SERVICE_NAME}
ExecStart=${INSTALL_DIR}/${BIN_NAME}
Environment=BIND_ADDR=${BIND_ADDR}
Environment=RUST_LOG=info
Restart=always
RestartSec=5
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
UNIT
    ok "Service file created"
fi

# ── 7. Start / restart ──────────────────
systemctl daemon-reload

if systemctl is-active --quiet "$SERVICE_NAME"; then
    info "Service already running, restarting..."
    systemctl restart "$SERVICE_NAME"
    ok "Service restarted"
else
    systemctl enable "$SERVICE_NAME"
    systemctl start "$SERVICE_NAME"
    ok "Service enabled and started"
fi

sleep 1

# ── 8. Verify ────────────────────────────
if systemctl is-active --quiet "$SERVICE_NAME"; then
    ok "MCProxy server is running"
else
    fail "Service failed to start. Check: journalctl -u $SERVICE_NAME -n 20"
fi

# ── 9. Firewall ──────────────────────────
PORT=$(echo "$BIND_ADDR" | cut -d: -f2)
if command -v ufw &>/dev/null; then
    if ! ufw status | grep -q "$PORT/tcp"; then
        ufw allow "$PORT/tcp" >/dev/null 2>&1 && ok "Firewall: opened port $PORT (ufw)"
    else
        ok "Firewall: port $PORT already open (ufw)"
    fi
elif command -v firewall-cmd &>/dev/null; then
    if ! firewall-cmd --list-ports | grep -q "$PORT/tcp"; then
        firewall-cmd --add-port="$PORT/tcp" --permanent >/dev/null 2>&1
        firewall-cmd --reload >/dev/null 2>&1
        ok "Firewall: opened port $PORT (firewalld)"
    else
        ok "Firewall: port $PORT already open (firewalld)"
    fi
else
    warn "No firewall tool detected. Make sure port $PORT is open."
fi

echo ""
echo -e "${GREEN}════════════════════════════════════════${NC}"
echo -e "${GREEN}  MCProxy server is running!${NC}"
echo -e "${GREEN}  Listening on: ${BIND_ADDR}${NC}"
echo -e "${GREEN}════════════════════════════════════════${NC}"
echo ""
echo "  Commands:"
echo "    sudo systemctl status mcproxy    # check status"
echo "    sudo systemctl stop mcproxy      # stop"
echo "    sudo systemctl restart mcproxy   # restart"
echo "    sudo journalctl -u mcproxy -f    # view logs"
echo ""
echo "  Clients connect to: ws://$(hostname -I 2>/dev/null | awk '{print $1}' || echo 'YOUR_IP'):${PORT}"
echo ""
