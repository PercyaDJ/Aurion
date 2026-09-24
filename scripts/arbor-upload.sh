#!/usr/bin/env bash
# Envoi direct d'un rapport à ARBOR (API documentée, sans l'agent arbor-scan).
#   scripts/arbor-upload.sh sboms <fichier.cdx.json>
#   scripts/arbor-upload.sh sarif <fichier.sarif>
# Variables : ARBOR_URL, ARBOR_API_KEY, ARBOR_PROJECT.
# En cas de refus, affiche le code HTTP, le serveur et le début de la réponse
# (jamais la clé) pour savoir qui refuse : ARBOR ou un pare-feu devant lui.
set -euo pipefail
KIND="${1:-}"; FILE="${2:-}"
case "$KIND" in sboms|sarif) ;; *) echo "Usage : $0 sboms|sarif <fichier>"; exit 1 ;; esac
[[ -s "$FILE" ]] || { echo "Fichier vide ou absent : $FILE"; exit 1; }
: "${ARBOR_URL:?ARBOR_URL manquant}" "${ARBOR_API_KEY:?ARBOR_API_KEY manquant}" "${ARBOR_PROJECT:?ARBOR_PROJECT manquant}"
[[ "$ARBOR_PROJECT" =~ ^[0-9a-f-]{36}$ ]] || { echo "ARBOR_PROJECT n'est pas un identifiant de projet"; exit 1; }

size=$(stat -c %s "$FILE")
(( size <= 25 * 1024 * 1024 )) || { echo "$FILE dépasse 25 Mo (limite ARBOR)"; exit 1; }

body=$(mktemp); headers=$(mktemp)
trap 'rm -f "$body" "$headers"' EXIT
# The key goes through a curl config on stdin: never on the command line
code=$(printf 'header = "Authorization: Bearer %s"\n' "$ARBOR_API_KEY" |
  curl -sS --retry 3 --retry-all-errors -K - -A "aurion-ci/1 curl" \
    -X POST "${ARBOR_URL%/}/api/v1/projects/$ARBOR_PROJECT/$KIND" \
    -F "file=@$FILE" -o "$body" -D "$headers" -w '%{http_code}') || code=000

if [[ "$code" =~ ^2 ]]; then
  echo "ARBOR : $(basename "$FILE") envoyé ($KIND, HTTP $code)"
  exit 0
fi
echo "::error::ARBOR a refusé $(basename "$FILE") ($KIND) : HTTP $code"
grep -iE '^(server|cf-ray|cf-mitigated|x-.*id):' "$headers" || true
head -c 400 "$body"; echo
exit 1
