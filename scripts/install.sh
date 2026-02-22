#!/usr/bin/env bash
set -euo pipefail

# ─────────────────────────────────────────────────────────────
# Aurion Install Script
# https://github.com/PercyaDJ/Aurion
#
# Installs the Aurion application on Raspberry Pi:
# 1. Installs Rust (rustup) if not present
# 2. Installs system dependencies (rpicam-apps, hostapd, dnsmasq)
# 3. Compiles the project (cargo build --release --features rpi)
# 4. Configures hostapd/dnsmasq (disabled at boot, managed by app)
# 5. Sets up sudoers for passwordless hardware commands
# 6. Creates and enables the aurion.service systemd unit
# 7. Generates config/aurion.json from defaults if needed
#
# Prerequisites:
#   - Run bootstrap.sh first and reboot
#   - USB capture drive mounted on /mnt/capture
#
# Usage:
#   bash scripts/install.sh
# ─────────────────────────────────────────────────────────────

AURION_DIR="$(cd "$(dirname "$0")/.." && pwd)"
AURION_USER="${SUDO_USER:-$(whoami)}"
AURION_HOME="$(getent passwd "$AURION_USER" | cut -d: -f6)"
AURION_BIN="${AURION_DIR}/target/release/aurion"

info() { echo -e "\n\033[1;32m[+]\033[0m $*\n"; }
warn() { echo -e "\n\033[1;33m[!]\033[0m $*\n"; }
die()  { echo -e "\n\033[1;31m[✗]\033[0m $*\n" >&2; exit 1; }

# ─── Step 1: Rust ────────────────────────────────────────────

install_rust() {
  if command -v rustc &>/dev/null; then
    info "Rust already installed: $(rustc --version)"
    return 0
  fi

  info "Installing Rust via rustup..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  source "${AURION_HOME}/.cargo/env"
  info "Rust installed: $(rustc --version)"
}

# ─── Step 2: System Dependencies ─────────────────────────────

install_dependencies() {
  info "Installing system dependencies..."
  sudo apt update
  sudo apt install -y --no-install-recommends \
    build-essential \
    pkg-config \
    libssl-dev \
    rpicam-apps \
    hostapd \
    dnsmasq \
    nftables
  info "System dependencies installed."
}

# ─── Step 3: Compile ─────────────────────────────────────────

compile_aurion() {
  info "Compiling Aurion (release + rpi features)..."
  info "This may take 15-30 minutes on RPi 4. Please be patient."

  cd "$AURION_DIR"

  # Ensure cargo is in PATH
  if [[ -f "${AURION_HOME}/.cargo/env" ]]; then
    source "${AURION_HOME}/.cargo/env"
  fi

  # Add swap if RAM might be tight for compilation
  if [[ $(free -m | awk '/Mem:/{print $2}') -lt 3000 ]]; then
    if [[ ! -f /tmp/aurion_swap ]]; then
      warn "Low RAM detected, creating temporary 2GB swap for compilation..."
      sudo fallocate -l 2G /tmp/aurion_swap
      sudo chmod 600 /tmp/aurion_swap
      sudo mkswap /tmp/aurion_swap
      sudo swapon /tmp/aurion_swap
    fi
  fi

  cargo build --release --features rpi

  # Remove temporary swap
  if [[ -f /tmp/aurion_swap ]]; then
    sudo swapoff /tmp/aurion_swap 2>/dev/null || true
    sudo rm -f /tmp/aurion_swap
  fi

  info "Compilation complete: ${AURION_BIN}"
}

# ─── Step 4: Configure hostapd/dnsmasq ──────────────────────

configure_network_services() {
  info "Configuring hostapd and dnsmasq (disabled at boot, managed by Aurion)..."

  # Unmask hostapd (may be masked by default on Bookworm)
  sudo systemctl unmask hostapd 2>/dev/null || true
  sudo systemctl disable hostapd 2>/dev/null || true

  sudo systemctl disable dnsmasq 2>/dev/null || true

  info "hostapd and dnsmasq configured (disabled at boot)."
}

# ─── Step 5: Sudoers ─────────────────────────────────────────

configure_sudoers() {
  info "Configuring sudoers for passwordless hardware commands..."

  local sudoers_file="/etc/sudoers.d/aurion"
  cat <<EOF | sudo tee "$sudoers_file" > /dev/null
# Aurion — passwordless access to hardware control commands
${AURION_USER} ALL=(ALL) NOPASSWD: /usr/bin/hostapd, /usr/bin/killall, /sbin/shutdown, /bin/mount, /bin/umount, /bin/ip, /usr/bin/dnsmasq, /usr/sbin/nft, /bin/date
EOF
  sudo chmod 440 "$sudoers_file"
  # Validate syntax
  sudo visudo -c -f "$sudoers_file" || die "Sudoers file has syntax errors!"

  info "Sudoers configured for user '${AURION_USER}'."
}

# ─── Step 6: systemd Service ─────────────────────────────────

create_service() {
  info "Creating aurion.service..."

  cat <<EOF | sudo tee /etc/systemd/system/aurion.service > /dev/null
[Unit]
Description=Aurion — Autonomous Aurora Capture
After=network.target
After=mnt-capture.automount

[Service]
Type=simple
User=${AURION_USER}
WorkingDirectory=${AURION_DIR}
ExecStart=${AURION_BIN} serve
Restart=on-failure
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
EOF

  sudo systemctl daemon-reload
  sudo systemctl enable aurion.service

  info "aurion.service enabled (will start on next boot)."
}

# ─── Step 7: Config File ─────────────────────────────────────

generate_config() {
  local config_file="${AURION_DIR}/config/aurion.json"

  if [[ -f "$config_file" ]]; then
    info "Config already exists: ${config_file}"
    return 0
  fi

  info "Generating config from defaults..."
  cp "${AURION_DIR}/config/default.json" "$config_file"
  info "Config created: ${config_file}"
}

# ─── Step 8: Verify Camera ───────────────────────────────────

verify_camera() {
  info "Checking camera..."
  if rpicam-hello --list-cameras 2>/dev/null | grep -q "Available cameras"; then
    info "✅ Camera detected!"
  else
    warn "⚠️  No camera detected. Check:"
    echo "  - CSI cable properly connected"
    echo "  - Run: sudo raspi-config → Interface Options → Camera → Enable"
    echo "  - Reboot after enabling"
  fi
}

# ─── Final Report ─────────────────────────────────────────────

final_report() {
  echo ""
  echo "═══════════════════════════════════════════════════"
  echo "  ✅  Aurion Installation Complete!"
  echo "═══════════════════════════════════════════════════"
  echo ""
  echo "  Binary:  ${AURION_BIN}"
  echo "  Config:  ${AURION_DIR}/config/aurion.json"
  echo "  Service: aurion.service (enabled)"
  echo "  USB:     /mnt/capture"
  echo ""
  echo "  Useful commands:"
  echo "    sudo systemctl start aurion     # Start now"
  echo "    sudo systemctl status aurion    # Check status"
  echo "    sudo journalctl -u aurion -f    # Live logs"
  echo "    sudo systemctl restart aurion   # Restart"
  echo "    sudo systemctl stop aurion      # Stop"
  echo ""
  echo "  Quick test (without service):"
  echo "    cd ${AURION_DIR}"
  echo "    ./target/release/aurion serve --port 8080"
  echo ""
  echo "  On the field:"
  echo "    1. Power on the Raspberry Pi"
  echo "    2. Connect to Wi-Fi: Aurion (password: aurora2024)"
  echo "    3. The dashboard opens automatically (captive portal)"
  echo "    4. Choose: 🌙 Capture or 📸 Recovery"
  echo ""
  warn "Start the service now? Run: sudo systemctl start aurion"
  echo ""
}

# ─── Main ─────────────────────────────────────────────────────

main() {
  echo ""
  echo "🌌 Aurion Installer"
  echo "───────────────────"
  echo ""

  install_rust
  install_dependencies
  compile_aurion
  configure_network_services
  configure_sudoers
  create_service
  generate_config
  verify_camera
  final_report
}

main "$@"
