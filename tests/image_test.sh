#!/usr/bin/env bash
# Test of scripts/build-image.sh on a fake Raspberry Pi OS image (same
# layout: FAT boot partition + ext4 root), without the apt step.
# Needs root and loop devices: sudo bash tests/image_test.sh
set -euo pipefail
REPO="$(cd "$(dirname "$0")/.." && pwd)"
PKG=$(ls -d "$REPO"/dist/aurion-*-rpi-arm64 2>/dev/null | head -n1)
[[ -n "$PKG" ]] || { echo "Lancez d'abord scripts/package.sh"; exit 1; }
[[ $EUID -eq 0 ]] || { echo "À lancer en root"; exit 1; }

W=$(mktemp -d)
LOOPS=()
cleanup() { set +e; mountpoint -q "$W/m" && umount "$W/m"; for l in "${LOOPS[@]}"; do losetup -d "$l"; done; rm -rf "$W"; }
trap cleanup EXIT
part() {
  local start sectors dev
  read -r start sectors < <(partx -g -o START,SECTORS -n "$2" "$1")
  dev=$(losetup -f --show --offset $((start * 512)) --sizelimit $((sectors * 512)) "$1")
  LOOPS+=("$dev"); echo "$dev"
}

# Fake base image: 256 MB FAT + 512 MB ext4 with the Raspberry Pi OS directories
truncate -s 800M "$W/base.img"
parted -s "$W/base.img" mklabel msdos mkpart primary fat32 4MiB 260MiB mkpart primary ext4 260MiB 772MiB
P1=$(part "$W/base.img" 1); P2=$(part "$W/base.img" 2)
mkfs.vfat -n bootfs "$P1" >/dev/null
mkfs.ext4 -q -L rootfs "$P2"
mkdir -p "$W/m"; mount "$P2" "$W/m"
mkdir -p "$W/m/boot/firmware" "$W/m/etc/systemd/system/multi-user.target.wants" "$W/m/usr/lib"
echo "fake" >"$W/m/etc/os-release"
umount "$W/m"; for l in "${LOOPS[@]}"; do losetup -d "$l"; done; LOOPS=()

bash "$REPO/scripts/build-image.sh" "$PKG" "$W/out/aurion.img.xz" --base "$W/base.img" --no-apt >"$W/build.log" 2>&1 \
  || { cat "$W/build.log"; exit 1; }

fails=0
check() { if eval "$2"; then echo "  ok  $1"; else echo "  FAIL $1"; fails=$((fails+1)); fi; }
check "image compressée et empreinte" '[[ -s "$W/out/aurion.img.xz" && -f "$W/out/aurion.img.xz.sha256" ]]'
( cd "$W/out" && sha256sum -c --quiet aurion.img.xz.sha256 ) && echo "  ok  empreinte SHA-256 valide" || { echo "  FAIL empreinte"; fails=$((fails+1)); }

xz -dc "$W/out/aurion.img.xz" >"$W/final.img"
check "partition système agrandie (+1,5 Go)" '[[ $(stat -c %s "$W/final.img") -gt $((2000*1024*1024)) ]]'
P1=$(part "$W/final.img" 1); P2=$(part "$W/final.img" 2)
check "système de fichiers sain" 'e2fsck -fn "$P2" >/dev/null 2>&1'
mount "$P2" "$W/m"
mtype -i "$P1" ::/userconf.txt >"$W/userconf.txt" 2>/dev/null
mtype -i "$P1" ::/AURION-LISEZMOI.txt >"$W/lisezmoi.txt"
mdir -b -i "$P1" ::/ >"$W/bootfiles.txt"
# shellcheck disable=SC2034  # used inside check()
R="$W/m"
check "binaire Aurion dans l'image" '[[ -x "$R/usr/lib/aurion/aurion" ]] && cmp -s "$PKG/aurion" "$R/usr/lib/aurion/aurion"'
check "installeur et helper présents" '[[ -f "$R/usr/lib/aurion/install.sh" && -x "$R/usr/lib/aurion/aurion-helper" ]]'
check "installation au premier démarrage activée" '[[ -L "$R/etc/systemd/system/multi-user.target.wants/aurion-firstboot.service" && -f "$R/usr/lib/aurion/firstboot-pending" ]]'
check "premier démarrage : mot de passe d'usine, sans apt" 'grep -q -- "--no-packages --default-wifi-password" "$R/usr/lib/aurion/aurion-firstboot.sh"'
check "compte de maintenance (pas d'assistant bloquant)" 'grep -q "^pi:" "$W/userconf.txt" && cut -d: -f2 "$W/userconf.txt" | grep -q "^.6."'
check "notice lisible depuis un PC" 'grep -q "aurora2024" "$W/lisezmoi.txt"'
check "SSH non activé" '! grep -qiE "/ssh(\.txt)?$" "$W/bootfiles.txt"'

echo
if [[ $fails -eq 0 ]]; then echo "build-image.sh : tous les contrôles OK"; else echo "build-image.sh : $fails échec(s)"; exit 1; fi
