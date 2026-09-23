#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
# Aurion — installation en une ligne sur le Raspberry Pi (avec internet) :
#
#   curl -fsSL https://raw.githubusercontent.com/PercyaDJ/Aurion/main/scripts/get.sh | sudo bash
#
# Dépôt privé : créer un jeton GitHub (lecture seule "Contents") puis
#   curl -fsSL -H "Authorization: Bearer $JETON" \
#     https://raw.githubusercontent.com/PercyaDJ/Aurion/main/scripts/get.sh | sudo GITHUB_TOKEN=$JETON bash
#
# Télécharge la dernière release (archive précompilée), vérifie son
# empreinte SHA-256 et lance l'installeur.
# ─────────────────────────────────────────────────────────────
set -euo pipefail
REPO="${AURION_REPO:-PercyaDJ/Aurion}"
API="https://api.github.com/repos/$REPO/releases/latest"
AUTH=()
[[ -n "${GITHUB_TOKEN:-}" ]] && AUTH=(-H "Authorization: Bearer $GITHUB_TOKEN")

[[ $EUID -eq 0 ]] || { echo "Lancez avec sudo"; exit 1; }
command -v curl >/dev/null || apt-get install -y curl

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
cd "$WORK"

echo "▶ Recherche de la dernière version de $REPO"
curl -fsSL "${AUTH[@]}" "$API" -o release.json
# Asset API URLs (work for private repositories with a token)
asset_url() {
  grep -o "\"url\": *\"https://api.github.com/repos/$REPO/releases/assets/[0-9]*\"[^}]*\"name\": *\"$1\"" release.json \
    | head -n1 | sed 's/"url": *"\([^"]*\)".*/\1/'
}
NAME=$(grep -o '"name": *"aurion-[^"]*-rpi-arm64.tar.gz"' release.json | head -n1 | sed 's/.*: *"//; s/"$//')
[[ -n "$NAME" ]] || { echo "Aucune archive Raspberry Pi dans la dernière release"; exit 1; }
URL=$(asset_url "$NAME")
SUM_URL=$(asset_url "$NAME.sha256")

echo "▶ Téléchargement de $NAME"
curl -fsSL "${AUTH[@]}" -H "Accept: application/octet-stream" "$URL" -o "$NAME"
if [[ -n "$SUM_URL" ]]; then
  curl -fsSL "${AUTH[@]}" -H "Accept: application/octet-stream" "$SUM_URL" -o "$NAME.sha256"
  sha256sum -c "$NAME.sha256"
fi
tar xzf "$NAME"
bash "./${NAME%.tar.gz}/install.sh" "$@"
