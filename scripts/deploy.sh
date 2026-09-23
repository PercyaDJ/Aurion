#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
# Send Aurion to a Raspberry Pi and install it, from a Linux/macOS PC.
#
#   scripts/deploy.sh pi@aurion.local                 # builds then deploys
#   scripts/deploy.sh pi@192.168.1.42 dist/aurion-1.10.0-rpi-arm64.tar.gz
#
# Windows: scripts\deploy.ps1 (same arguments).
# ─────────────────────────────────────────────────────────────
set -euo pipefail
cd "$(dirname "$0")/.."

HOST="${1:-}"
ARCHIVE="${2:-}"
[[ -n "$HOST" ]] || { echo "Usage : $0 utilisateur@raspberry [archive.tar.gz]"; exit 1; }

if [[ -z "$ARCHIVE" ]]; then
  bash scripts/package.sh
  ARCHIVE=$(ls -t dist/aurion-*-rpi-arm64.tar.gz | head -n1)
fi
[[ -f "$ARCHIVE" ]] || { echo "Archive introuvable : $ARCHIVE"; exit 1; }
NAME=$(basename "$ARCHIVE" .tar.gz)

echo "▶ Envoi de $NAME vers $HOST"
scp "$ARCHIVE" "$HOST:/tmp/$NAME.tar.gz"
echo "▶ Installation (le mot de passe sudo du Pi peut être demandé)"
ssh -t "$HOST" "cd /tmp && tar xzf $NAME.tar.gz && sudo ./$NAME/install.sh && rm -rf /tmp/$NAME /tmp/$NAME.tar.gz"
