#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
# Aurion — compilation sur le Raspberry Pi, puis installation (secours).
# Appelé par ./install.sh quand aucune archive précompilée n'est disponible ;
# se lance aussi à la main depuis un « git clone », en utilisateur normal :
#
#   bash scripts/setup.sh [options de scripts/install.sh]
#
# 15 à 30 minutes sur un Pi 4. Le plus simple reste l'image carte SD ou
# l'archive de release (scripts/get.sh).
# ─────────────────────────────────────────────────────────────
set -euo pipefail
AURION_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$AURION_DIR"
[[ $EUID -ne 0 ]] || { echo "Lancez ce script en utilisateur normal (il demande sudo quand il le faut)"; exit 1; }

echo "▶ [1/3] Outils de compilation (gcc, Rust)"
# The SD image removes the compilers: the linker is needed again here
sudo apt-get install -y --no-install-recommends gcc libc6-dev curl ca-certificates
if ! command -v cargo >/dev/null; then
  [[ -x "$HOME/.cargo/bin/cargo" ]] || curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
fi

echo "▶ [2/3] Compilation d'Aurion (15 à 30 min)"
cargo build --release --locked --features rpi

echo "▶ [3/3] Installation"
# scripts/install.sh finds target/release/aurion by itself
sudo bash scripts/install.sh --binary "$AURION_DIR/target/release/aurion" "$@"
