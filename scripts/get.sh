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
# empreinte SHA-256 (obligatoire) et lance l'installeur.
# AURION_GET_DOWNLOAD_ONLY=1 : s'arrête après la vérification (tests).
# ─────────────────────────────────────────────────────────────
set -euo pipefail
REPO="${AURION_REPO:-PercyaDJ/Aurion}"
API="https://api.github.com/repos/$REPO/releases/latest"

[[ $EUID -eq 0 ]] || { echo "Lancez avec sudo"; exit 1; }
command -v curl >/dev/null || apt-get install -y curl
command -v python3 >/dev/null || { echo "python3 absent (présent sur Raspberry Pi OS)"; exit 1; }

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
# The token goes through a curl config file (0600, private folder), never on
# a command line that any process can read.
AUTH=()
if [[ -n "${GITHUB_TOKEN:-}" ]]; then
  (umask 077; printf 'header = "Authorization: Bearer %s"\n' "$GITHUB_TOKEN" >"$WORK/auth")
  AUTH=(-K "$WORK/auth")
fi
cd "$WORK"

echo "▶ Recherche de la dernière version de $REPO"
curl -fsSL "${AUTH[@]}" "$API" -o release.json
# Assets of the release (the API answers indented JSON over hundreds of
# lines: read it as JSON). API URLs work for private repositories too.
# asset <regex>: "name url" of the first asset whose name matches.
asset() {
  python3 - "$1" <<'PY'
import json, re, sys
for a in json.load(open("release.json")).get("assets", []):
    if re.fullmatch(sys.argv[1], a.get("name", "")):
        print(a["name"], a["url"])
        break
PY
}
read -r NAME URL <<<"$(asset 'aurion-.+-rpi-arm64\.tar\.gz')" || true
[[ -n "${NAME:-}" && -n "${URL:-}" ]] || { echo "Aucune archive Raspberry Pi dans la dernière release"; exit 1; }
read -r _ SUM_URL <<<"$(asset "${NAME//./\\.}\\.sha256")" || true
[[ -n "${SUM_URL:-}" ]] || { echo "Empreinte $NAME.sha256 absente de la release : installation refusée"; exit 1; }

echo "▶ Téléchargement de $NAME"
curl -fsSL "${AUTH[@]}" -H "Accept: application/octet-stream" "$URL" -o "$NAME"
curl -fsSL "${AUTH[@]}" -H "Accept: application/octet-stream" "$SUM_URL" -o "$NAME.sha256"
sha256sum -c "$NAME.sha256"
[[ "${AURION_GET_DOWNLOAD_ONLY:-0}" == "1" ]] && { echo "Archive vérifiée : $NAME"; exit 0; }
tar xzf "$NAME"
bash "./${NAME%.tar.gz}/install.sh" "$@"
