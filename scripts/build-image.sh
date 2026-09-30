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
# Pinned and checked: every build of a version starts from the same system,
# and a corrupted or substituted download stops the build. Security updates
# are still applied below (full-upgrade). Moving to a newer Raspberry Pi OS:
# take the address raspios_lite_arm64_latest redirects to and the SHA-256 of
# its .sha256 file, update both lines, then build and test the image.
BASE_URL=https://downloads.raspberrypi.com/raspios_lite_arm64/images/raspios_lite_arm64-2026-09-15/2026-09-15-raspios-trixie-arm64-lite.img.xz
BASE_SHA256=cdf4f3bfac35ae947b46e4e767f935453810549779ac3290e05a6754aee627e5
if [[ -z "$BASE" ]]; then
  echo "▶ Téléchargement de Raspberry Pi OS Lite (64 bits), version du ${BASE_URL##*/}"
  curl -fL --retry 3 -o "$WORK/base.img.xz" "$BASE_URL"
  echo "$BASE_SHA256  $WORK/base.img.xz" | sha256sum -c --quiet \
    || { echo "Empreinte de l'image de base différente de celle attendue : fabrication arrêtée"; exit 1; }
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
    apt-get install -y --no-install-recommends iw nftables rfkill dosfstools exfatprogs dnsmasq-base curl util-linux-extra \
      xz-utils
    command -v rpicam-still >/dev/null || apt-get install -y --no-install-recommends rpicam-apps-lite || apt-get install -y --no-install-recommends rpicam-apps
    # Security fixes published since the Raspberry Pi OS image
    apt-get -y -o Dpkg::Options::=--force-confold full-upgrade
    # Smaller attack surface: nothing an offline camera uses. Remote access
    # (rpi-connect), cloud-init (Imager seed, unused: userconf.txt is read by
    # userconfig.service), compilers, debugger, Bluetooth (disabled), SMB, archivers.
    # Also: kernel headers and their compiler (no module is ever built here),
    # firmware of Wi-Fi chips a Pi does not have (the Pi uses brcm80211), PPP,
    # rpi-update (untested firmware), pastebinit (uploads text), rich/pygments.
    # Also (1.11.3): Python package tools, debugging and diagnostic tools
    # (strace, htop, v4l-utils, wireless-tools), NetworkManager translations,
    # man-db (manuals are removed below) and ntfs-3g (NTFS keys go through the
    # kernel ntfs3 driver). avahi stays: aurion.local is used for maintenance.
    unused=$(dpkg-query -W -f="\${Package} \${db:Status-Status}\n" rpi-connect-lite cloud-init mkvtoolnix gdb \
      cifs-utils bluez p7zip-full 7zip build-essential g++ g++-14 gcc gcc-14 dpkg-dev ssh-import-id \
      linux-headers-rpi-2712 linux-headers-rpi-v8 firmware-atheros firmware-mediatek firmware-realtek \
      firmware-libertas ppp rpi-update pastebinit python3-rich wget xdg-user-dirs bash-completion \
      python3-venv python3-pip-whl python3-setuptools-whl strace htop v4l-utils wireless-tools \
      network-manager-l10n man-db ntfs-3g 2>/dev/null \
      | awk "\$2 == \"installed\" {print \$1}")
    # Versioned kernel headers are protected from autoremove (APT::NeverAutoRemove):
    # name them, and the compiler they pull, explicitly.
    unused="$unused $(dpkg-query -W -f="\${Package} \${db:Status-Status}\n" "linux-headers-*" "linux-kbuild-*" \
      gcc-14-for-host cpp-14-for-host gcc-14-aarch64-linux-gnu cpp-14-aarch64-linux-gnu 2>/dev/null \
      | awk "\$2 == \"installed\" {print \$1}")"
    # shellcheck disable=SC2086
    [ -z "${unused// /}" ] || apt-get purge -y --auto-remove $unused
    if dpkg-query -W -f="\${Package} \${db:Status-Status}\n" "linux-headers-*" gcc-14-for-host 2>/dev/null | grep -q " installed$"; then
      echo "En-têtes du noyau ou compilateur encore présents"; exit 1
    fi
    # What boots the Pi and its Wi-Fi must still be there
    for p in raspi-firmware firmware-brcm80211 linux-image-rpi-2712 linux-image-rpi-v8 network-manager rpi-eeprom \
             netplan.io xz-utils; do
      dpkg-query -W -f="\${db:Status-Status}" "$p" 2>/dev/null | grep -qx installed \
        || { echo "Paquet indispensable retiré : $p"; exit 1; }
    done
    # Lowest version carrying the security fixes (ARBOR plans): an older one means
    # the Debian security archive was not reached during the upgrade.
    # openssl: binary packages of the source package ARBOR reports (13 CVE fixed in deb13u3).
    for pv in "rsync 3.5.0+ds1-0+deb13u1" "libssl3t64 3.5.7-1~deb13u3" "openssl 3.5.7-1~deb13u3" \
              "openssl-provider-legacy 3.5.7-1~deb13u3"; do
      p=${pv% *}; min=${pv#* }
      v=$(dpkg-query -W -f="\${db:Status-Status} \${Version}" "$p" 2>/dev/null || true)
      case "$v" in
        "installed "*) dpkg --compare-versions "${v#installed }" ge "$min" \
          || { echo "Correctif de sécurité absent : $p ${v#installed } (minimum $min)"; exit 1; } ;;
      esac
    done
    # Documentation, manuals and translations: never read on a camera.
    # Licences (copyright files) stay. dpkg skips these paths for later updates too.
    cat >/etc/dpkg/dpkg.cfg.d/aurion-minimal <<CFG
path-exclude=/usr/share/doc/*
path-include=/usr/share/doc/*/copyright
path-exclude=/usr/share/man/*
path-exclude=/usr/share/info/*
path-exclude=/usr/share/locale/*
path-include=/usr/share/locale/locale.alias
CFG
    find /usr/share/doc -mindepth 1 ! -name copyright ! -type d -delete
    find /usr/share/doc -mindepth 1 -type d -empty -delete
    rm -rf /usr/share/man/* /usr/share/info/*
    find /usr/share/locale -mindepth 1 -maxdepth 1 ! -name locale.alias -exec rm -rf {} +
    # Everything Aurion calls must still be there
    for c in nmcli iw nft rfkill mkfs.exfat wipefs sfdisk blkid findmnt hwclock timedatectl udevadm \
             ip curl sudo rpicam-still vcgencmd dnsmasq openssl tar; do
      command -v "$c" >/dev/null || { echo "Outil manquant après nettoyage : $c"; exit 1; }
    done
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
   L'accueil propose de choisir votre mot de passe, vérifie la caméra et la clé,
   puis « Lancer la nuit ».

Nouvelle carte SD avec une clé déjà utilisée : vos réglages sont repris
depuis la clé (fichier aurion-reglages.json), vos photos restent sur la clé.
TXT
boot_write "$WORK/AURION-LISEZMOI.txt" AURION-LISEZMOI.txt

# config.txt is read by the firmware at boot: written now (same block as
# install.sh), LEDs, Bluetooth, audio and the hardware watchdog are already
# right on the first boot, which would otherwise only apply them at the next.
if mcopy -n -i "$P1" ::/config.txt "$WORK/config.txt" 2>/dev/null; then
  mkdir -p "$WORK/bootroot/boot/firmware"
  cp "$WORK/config.txt" "$WORK/bootroot/boot/firmware/config.txt"
  AURION_TEST_ROOT="$WORK/bootroot" bash "$HERE/scripts/install.sh" --boot-config-only
  boot_write "$WORK/bootroot/boot/firmware/config.txt" config.txt
fi

# ─── 5. Finish ───────────────────────────────────────────────
sync
# Blocks freed by the package purge still hold old data, which xz cannot
# compress: discard them so they read as zeros and the image stays small.
fstrim "$ROOT" 2>/dev/null || true
umount "$ROOT"
e2fsck -fy "$P2" >/dev/null || true
detach_all

echo "▶ Compression"
mkdir -p "$(dirname "$OUT")"
xz -T0 -6 -c "$IMG" >"$OUT"
( cd "$(dirname "$OUT")" && sha256sum "$(basename "$OUT")" >"$(basename "$OUT").sha256" )
echo "✅ $OUT"
