#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
# Aurion — installation clé en main depuis un « git clone »
#
#   git clone https://github.com/PercyaDJ/Aurion.git ~/Aurion
#   sudo ~/Aurion/install.sh
#
# 1. Archive déjà construite dans dist/ : installée telle quelle.
# 2. Sinon : télécharge l'archive précompilée de la dernière release
#    (jeton repris de l'URL du clone pour un dépôt privé).
# 3. Sinon : compile sur le Pi (15 à 30 min, secours).
#
# Le plus rapide : cloner directement la branche prête à l'emploi
#   git clone -b rpi https://github.com/PercyaDJ/Aurion.git ~/aurion && sudo ~/aurion/install.sh
# ─────────────────────────────────────────────────────────────
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[[ $EUID -eq 0 ]] || { echo "Lancez avec sudo : sudo $0"; exit 1; }

# 1. Archive already built on this machine (scripts/package.sh)
for dir in "$HERE"/dist/aurion-*-rpi-arm64; do
  if [[ -x "$dir/aurion" ]] && "$dir/aurion" --version >/dev/null 2>&1; then
    exec bash "$dir/install.sh" "$@"
  fi
done

# 2. Latest release (token of the clone URL for a private repository)
if [[ -z "${GITHUB_TOKEN:-}" ]] && command -v git >/dev/null; then
  url=$(git -C "$HERE" remote get-url origin 2>/dev/null || true)
  if [[ "$url" =~ ^https://([^@/:]+)(:([^@/]+))?@github\.com/ ]]; then
    token="${BASH_REMATCH[3]:-${BASH_REMATCH[1]}}"
    [[ "$token" == "x-access-token" || "$token" == "git" ]] && token=""
    [[ -n "$token" ]] && export GITHUB_TOKEN="$token"
  fi
fi
if [[ "$(uname -m)" == "aarch64" ]] && bash "$HERE/scripts/get.sh" "$@"; then
  exit 0
fi

# 3. Build on the Pi
echo "Aucune archive précompilée disponible : compilation locale."
SUDO_USER_NAME="${SUDO_USER:-$(getent passwd 1000 | cut -d: -f1)}"
exec sudo -u "$SUDO_USER_NAME" bash "$HERE/scripts/setup.sh" "$@"
