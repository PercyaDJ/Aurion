#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
# Fabrique l'image carte SD « prête à brancher » :
#   Raspberry Pi OS Lite 64 bits + Aurion installé au premier démarrage.
#
#   sudo scripts/build-image.sh <dossier_archive> <sortie.img.xz> [--base <raspios.img[.xz]>] [--no-apt]
#
# <dossier_archive> : dist/aurion-<version>-rpi-arm64 (scripts/package.sh)
# --base   : image Raspberry Pi OS déjà téléchargée (sinon : dernière version Lite arm64)
# --no-apt : ne pas installer de paquets dans l'image (tests sans émulation ARM)
#
# Utilisé par la CI (release.yml). Nécessite : root, losetup, parted,
# e2fsck/resize2fs, partx, mtools, xz, et pour l'étape apt :
# qemu-user-static + binfmt. La partition FAT de démarrage est écrite avec
# mtools, sans être montée.
# ─────────────────────────────────────────────────────────────
set -euo pipefail

PKG="${1:-}"; OUT="${2:-}"; shift 2 || true
BASE=""; APT=1
while [[ $# -gt 0 ]]; do
  case "$1" in
    --base) BASE="$2"; shift 2 ;;
    --no-apt) APT=0; shift ;;
    *) echo "Option inconnue : $1"; exit 1 ;;
  esac
done
[[ $EUID -eq 0 ]] || { echo "À lancer en root"; exit 1; }
[[ -x "$PKG/aurion" && -f "$PKG/install.sh" ]] || { echo "Dossier d'archive invalide : $PKG"; exit 1; }
[[ -n "$OUT" ]] || { echo "Usage : $0 <dossier_archive> <sortie.img.xz>"; exit 1; }
HERE="$(cd "$(dirname "$0")/.." && pwd)"

WORK=$(mktemp -d)
ROOT="$WORK/root"
LOOPS=()
cleanup() {
  set +e
  for m in "$ROOT/boot/firmware" "$ROOT/boot" "$ROOT/dev/pts" "$ROOT/dev" "$ROOT/proc" "$ROOT/sys" "$ROOT"; do
    mountpoint -q "$m" && umount -l "$m"
  done
  for l in "${LOOPS[@]}"; do losetup -d "$l" 2>/dev/null; done
  rm -rf "$WORK"
}
trap cleanup EXIT

# Attach partition N of an image to a loop device by its offset (works
# without udev, e.g. in containers and CI runners).
attach_part() {
  local img="$1" n="$2" start sectors dev
  read -r start sectors < <(partx -g -o START,SECTORS -n "$n" "$img")
  dev=$(losetup -f --show --offset $((start * 512)) --sizelimit $((sectors * 512)) "$img")
  LOOPS+=("$dev")
  echo "$dev"
}

detach_all() {
  local l
  for l in "${LOOPS[@]}"; do losetup -d "$l" 2>/dev/null || true; done
  LOOPS=()
}

# ─── 1. Base image ───────────────────────────────────────────
if [[ -z "$BASE" ]]; then
  echo "▶ Téléchargement de Raspberry Pi OS Lite (64 bits)"
  curl -fL --retry 3 -o "$WORK/base.img.xz" https://downloads.raspberrypi.com/raspios_lite_arm64_latest
  BASE="$WORK/base.img.xz"
fi
IMG="$WORK/aurion.img"
case "$BASE" in
  *.xz) xz -dc "$BASE" >"$IMG" ;;
  *) cp "$BASE" "$IMG" ;;
esac

# ─── 2. Room for the packages ────────────────────────────────
echo "▶ Agrandissement de la partition système"
truncate -s +1536M "$IMG"
parted -s "$IMG" resizepart 2 100%
P1=$(attach_part "$IMG" 1)
P2=$(attach_part "$IMG" 2)
e2fsck -fy "$P2" >/dev/null || true
resize2fs "$P2" >/dev/null

mkdir -p "$ROOT"
mount "$P2" "$ROOT"

# Write a file into the FAT boot partition (no mount needed)
boot_write() {
  mcopy -o -i "$P1" "$1" "::/$2"
}

# ─── 3. Packages (inside the image, ARM emulation) ───────────
if [[ $APT -eq 1 ]]; then
  echo "▶ Installation des paquets dans l'image"
  mount -t proc proc "$ROOT/proc"
  mount -t sysfs sys "$ROOT/sys"
  mount --bind /dev "$ROOT/dev"
  mount --bind /dev/pts "$ROOT/dev/pts"
  cp /etc/resolv.conf "$ROOT/etc/resolv.conf.aurion"
  mv "$ROOT/etc/resolv.conf" "$ROOT/etc/resolv.conf.orig" 2>/dev/null || true
  cp "$ROOT/etc/resolv.conf.aurion" "$ROOT/etc/resolv.conf"
  chroot "$ROOT" /bin/bash -c '
    set -e
    export DEBIAN_FRONTEND=noninteractive
    apt-get update
    apt-get install -y --no-install-recommends iw nftables rfkill dosfstools exfatprogs dnsmasq-base
    command -v rpicam-still >/dev/null || apt-get install -y --no-install-recommends rpicam-apps-lite || apt-get install -y --no-install-recommends rpicam-apps
    apt-get clean
    rm -rf /var/lib/apt/lists/*
  '
  rm -f "$ROOT/etc/resolv.conf" "$ROOT/etc/resolv.conf.aurion"
  mv "$ROOT/etc/resolv.conf.orig" "$ROOT/etc/resolv.conf" 2>/dev/null || true
  umount "$ROOT/dev/pts" "$ROOT/dev" "$ROOT/proc" "$ROOT/sys"
fi

# ─── 4. Aurion + first boot installation ─────────────────────
echo "▶ Copie d'Aurion"
install -d "$ROOT/usr/lib/aurion"
cp -a "$PKG/." "$ROOT/usr/lib/aurion/"
install -m 0755 "$HERE/deploy/aurion-firstboot.sh" "$ROOT/usr/lib/aurion/aurion-firstboot.sh"
touch "$ROOT/usr/lib/aurion/firstboot-pending"
install -m 0644 "$HERE/deploy/aurion-firstboot.service" "$ROOT/etc/systemd/system/aurion-firstboot.service"
install -d "$ROOT/etc/systemd/system/multi-user.target.wants"
ln -sf /etc/systemd/system/aurion-firstboot.service "$ROOT/etc/systemd/system/multi-user.target.wants/aurion-firstboot.service"

# Maintenance account (keyboard + screen only, SSH stays off): pi / aurion.
# Its presence also skips the interactive first-boot user wizard.
echo "pi:$(openssl passwd -6 aurion)" >"$WORK/userconf.txt"
boot_write "$WORK/userconf.txt" userconf.txt

cat >"$WORK/AURION-LISEZMOI.txt" <<TXT
Aurion - caméra d'aurores boréales

1. Carte SD dans le Raspberry Pi, caméra et clé USB branchées.
2. Allumez. Le premier démarrage prend 3 à 5 minutes.
3. Sur le téléphone, rejoignez le Wi-Fi « Aurion » (mot de passe : aurora2024).
4. La page Aurion s'ouvre toute seule (sinon : http://192.168.4.1:8080).
   Changez le mot de passe Wi-Fi dans Réglages avancés.
TXT
boot_write "$WORK/AURION-LISEZMOI.txt" AURION-LISEZMOI.txt

# ─── 5. Finish ───────────────────────────────────────────────
sync
umount "$ROOT"
e2fsck -fy "$P2" >/dev/null || true
detach_all

echo "▶ Compression"
mkdir -p "$(dirname "$OUT")"
xz -T0 -6 -c "$IMG" >"$OUT"
( cd "$(dirname "$OUT")" && sha256sum "$(basename "$OUT")" >"$(basename "$OUT").sha256" )
echo "✅ $OUT"
