#!/usr/bin/env bash
# Analyses envoyées à ARBOR (https://arbor.norkenix.com), communes aux workflows Release et ARBOR.
#   scripts/arbor-scan.sh app [aurion-sbom.cdx.json]   SBOM Syft du dépôt, Semgrep, Trivy -> Aurion_Application
#   scripts/arbor-scan.sh image <aurion-image-sbom.cdx.json>                         -> Aurion_Image
# Une clé par projet : ARBOR_API_KEY (application), ARBOR_IMAGE_API_KEY (image). Sans clé, rien n'est
# installé ni envoyé (avertissement) : pas de minutes perdues. Avec ARBOR_REQUIRED=1 (workflows Release et
# ARBOR), une clé absente est une erreur : l'envoi ne peut pas manquer sans que le job soit rouge.
# Outils en versions figées, scripts d'installation pris sur le tag : un changement amont ne modifie
# pas ce qui s'exécute ici.
set -euo pipefail
cd "$(dirname "$0")/.."
SYFT_VERSION=v1.20.0
TRIVY_VERSION=v0.70.0
SEMGREP_VERSION=1.177.0
export ARBOR_URL="${ARBOR_URL:-https://arbor.norkenix.com}"
APP_PROJECT=46e8a16d-58a7-4300-a6ae-e0cbac7ee6c1   # Aurion_Application
IMAGE_PROJECT=7d095ce7-6d86-4c48-92ef-b9df2fbd3327 # Aurion_Image

need_syft() {
  command -v syft >/dev/null || curl -sSfL "https://raw.githubusercontent.com/anchore/syft/${SYFT_VERSION}/install.sh" \
    | sudo sh -s -- -b /usr/local/bin "$SYFT_VERSION"
}
missing_key() { # <message>
  if [[ "${ARBOR_REQUIRED:-0}" == "1" ]]; then
    echo "::error::$1"
    exit 1
  fi
  echo "::warning::$1"
  exit 0
}
upload() { # <project> <key> <kind> <file>
  ARBOR_PROJECT="$1" ARBOR_API_KEY="$2" bash scripts/arbor-upload.sh "$3" "$4"
}

case "${1:-}" in
  app)
    key="${ARBOR_API_KEY:-}"
    [[ -n "$key" ]] || missing_key "Secret ARBOR_API_KEY absent (Settings > Secrets and variables > Actions) : rien n'est envoyé."
    out=$(mktemp -d)
    sbom="${2:-}"
    if [[ -z "$sbom" ]]; then
      need_syft
      v=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
      sbom="$out/aurion-sbom.cdx.json"
      syft scan dir:. --exclude ./target --exclude ./dist --source-name aurion --source-version "$v" \
        -o cyclonedx-json="$sbom"
    fi
    command -v trivy >/dev/null || curl -sSfL "https://raw.githubusercontent.com/aquasecurity/trivy/${TRIVY_VERSION}/contrib/install.sh" \
      | sudo sh -s -- -b /usr/local/bin "$TRIVY_VERSION"
    command -v semgrep >/dev/null || pipx install --quiet "semgrep==${SEMGREP_VERSION}"
    # Semgrep only reads files tracked by git; Trivy skips build outputs
    semgrep scan --config p/default --metrics=off --quiet --sarif -o "$out/semgrep.sarif" .
    trivy fs --quiet --scanners secret,misconfig --skip-dirs target,dist,tests/e2e/node_modules \
      --format sarif -o "$out/trivy.sarif" .
    upload "$APP_PROJECT" "$key" sboms "$sbom"
    upload "$APP_PROJECT" "$key" sarif "$out/semgrep.sarif"
    upload "$APP_PROJECT" "$key" sarif "$out/trivy.sarif"
    ;;
  image)
    [[ -n "${2:-}" ]] || { echo "Usage : $0 image <aurion-image-sbom.cdx.json>"; exit 1; }
    key="${ARBOR_IMAGE_API_KEY:-}"
    [[ -n "$key" ]] || missing_key "Secret ARBOR_IMAGE_API_KEY absent : SBOM de l'image non envoyé."
    upload "$IMAGE_PROJECT" "$key" sboms "$2"
    ;;
  *) echo "Usage : $0 app [sbom] | image <sbom>"; exit 1 ;;
esac
