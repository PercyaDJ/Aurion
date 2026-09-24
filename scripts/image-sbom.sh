#!/usr/bin/env bash
# SBOM CycloneDX des paquets du système de l'image carte SD (paquets Debian
# de Raspberry Pi OS, plus l'application installée dans /opt/aurion).
#   sudo scripts/image-sbom.sh <image.img[.xz]> <sortie.cdx.json> [version]
# La partition système est montée en lecture seule : l'image n'est jamais modifiée.
# Nécessite : root, syft, partx, losetup, xz (image .xz).
set -euo pipefail
IMG_IN="${1:-}"; OUT="${2:-}"; VERSION="${3:-}"
[[ -n "$IMG_IN" && -n "$OUT" ]] || { echo "Usage : $0 <image.img[.xz]> <sortie.cdx.json> [version]"; exit 1; }
[[ -f "$IMG_IN" ]] || { echo "Image introuvable : $IMG_IN"; exit 1; }
[[ $EUID -eq 0 ]] || { echo "À lancer en root (montage de l'image)"; exit 1; }
command -v syft >/dev/null || { echo "syft absent"; exit 1; }

WORK=$(mktemp -d)
LOOP=""
cleanup() {
  set +e
  mountpoint -q "$WORK/root" && umount "$WORK/root"
  [[ -n "$LOOP" ]] && losetup -d "$LOOP"
  rm -rf "$WORK"
}
trap cleanup EXIT

case "$IMG_IN" in
  *.xz) xz -dc "$IMG_IN" >"$WORK/image.img"; IMG="$WORK/image.img" ;;
  *) IMG="$IMG_IN" ;;
esac

# Partition 2 = système (ext4), attachée par son décalage (sans udev)
read -r start sectors < <(partx -g -o START,SECTORS -n 2 "$IMG")
LOOP=$(losetup -f --show -r --offset $((start * 512)) --sizelimit $((sectors * 512)) "$IMG")
mkdir -p "$WORK/root"
mount -o ro,noload "$LOOP" "$WORK/root"
[[ -f "$WORK/root/var/lib/dpkg/status" ]] || { echo "Pas de base dpkg dans l'image : ce n'est pas un Raspberry Pi OS"; exit 1; }

args=(--source-name aurion-image)
[[ -n "$VERSION" ]] && args+=(--source-version "$VERSION")
# Packages only: the per-file list would multiply the size by 30 (limit 25 MB)
# and --base-path keeps the temporary mount point out of the SBOM.
SYFT_FILE_METADATA_SELECTION=none syft scan "dir:$WORK/root" "${args[@]}" --base-path "$WORK/root" \
  --exclude ./proc --exclude ./sys --exclude ./dev --exclude ./tmp --exclude ./var/cache \
  -o "cyclonedx-json=$OUT" -q
echo "SBOM de l'image : $OUT ($(grep -o '"purl": *"pkg:deb/[^"]*' "$OUT" | sort -u | wc -l) paquets Debian)"
