#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
# Build the Raspberry Pi release archive (from a Linux PC or CI):
#   dist/aurion-<version>-rpi-arm64.tar.gz
#
# The binary is statically linked (musl): it runs on every 64-bit
# Raspberry Pi OS (Bullseye, Bookworm, Trixie) without any dependency.
#
# Requirements (Debian/Ubuntu): sudo apt install gcc-aarch64-linux-gnu
# (rustup target is added automatically).
# ─────────────────────────────────────────────────────────────
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET=aarch64-unknown-linux-musl
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
NAME="aurion-${VERSION}-rpi-arm64"
OUT=dist/$NAME

command -v aarch64-linux-gnu-gcc >/dev/null || { echo "Installez gcc-aarch64-linux-gnu"; exit 1; }
rustup target list --installed | grep -qx "$TARGET" || rustup target add "$TARGET"

echo "▶ Compilation $TARGET (release)"
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-linux-gnu-gcc \
  cargo build --release --locked --features rpi --target "$TARGET"

echo "▶ Assemblage de $NAME"
rm -rf "$OUT" && mkdir -p "$OUT/deploy" "$OUT/config"
install -m 0755 "target/$TARGET/release/aurion" "$OUT/aurion"
install -m 0755 scripts/install.sh "$OUT/install.sh"
install -m 0755 scripts/aurion-helper "$OUT/aurion-helper"
install -m 0644 deploy/aurion.service deploy/99-aurion-usb.rules "$OUT/deploy/"
install -m 0644 config/default.json "$OUT/config/"
cat >"$OUT/LISEZMOI.txt" <<TXT
# Aurion $VERSION pour Raspberry Pi (64 bits)

Installation sur un Raspberry Pi OS existant, sans compilation :

    tar xzf $NAME.tar.gz && sudo ./$NAME/install.sh

(ou le paquet : sudo apt install ./aurion_${VERSION}_arm64.deb)

Le plus simple pour une carte SD neuve : l'image aurion-${VERSION}-raspios-arm64.img.xz
à copier avec Raspberry Pi Imager (voir docs/GUIDE_DEMARRAGE.md).

À la fin, le nom et le mot de passe du Wi-Fi s'affichent : notez-les.
Mise à jour possible aussi depuis le téléphone : Diagnostics, Mise à jour du
logiciel, fichier "aurion".
TXT
tar -C dist -czf "dist/$NAME.tar.gz" "$NAME"
( cd dist && sha256sum "$NAME.tar.gz" >"$NAME.tar.gz.sha256" )
echo "✅ dist/$NAME.tar.gz"

# ─── Debian package: sudo apt install ./aurion_<version>_arm64.deb ───
if command -v dpkg-deb >/dev/null; then
  DEB="aurion_${VERSION}_arm64"
  PKG="dist/$DEB"
  rm -rf "$PKG" && mkdir -p "$PKG/DEBIAN" "$PKG/usr/lib/aurion"
  cp -a "$OUT/." "$PKG/usr/lib/aurion/"
  rm -f "$PKG/usr/lib/aurion/LISEZMOI.txt"
  cat >"$PKG/DEBIAN/control" <<CTRL
Package: aurion
Version: $VERSION
Architecture: arm64
Maintainer: PercyaDJ <noreply@users.noreply.github.com>
Section: graphics
Priority: optional
Depends: bash, sudo, iw, nftables, rfkill, dosfstools, exfatprogs
Recommends: rpicam-apps | rpicam-apps-lite, network-manager | hostapd, dnsmasq-base | dnsmasq
Description: Caméra autonome d'aurores boréales pour Raspberry Pi
 Hotspot Wi-Fi, interface web pour smartphone, capture RAW/JPEG de nuit
 avec détection d'aurores, timelapse et récupération des photos.
CTRL
  cat >"$PKG/DEBIAN/postinst" <<'POST'
#!/bin/sh
set -e
if [ "$1" = "configure" ]; then
  # Dependencies are handled by apt: no apt call from the installer
  bash /usr/lib/aurion/install.sh --binary /usr/lib/aurion/aurion --no-packages
fi
POST
  cat >"$PKG/DEBIAN/prerm" <<'PRERM'
#!/bin/sh
set -e
if [ "$1" = "remove" ] || [ "$1" = "purge" ]; then
  bash /usr/lib/aurion/install.sh --uninstall || true
fi
PRERM
  chmod 0755 "$PKG/DEBIAN/postinst" "$PKG/DEBIAN/prerm"
  dpkg-deb --root-owner-group --build "$PKG" "dist/$DEB.deb" >/dev/null
  rm -rf "$PKG"
  ( cd dist && sha256sum "$DEB.deb" >"$DEB.deb.sha256" )
  echo "✅ dist/$DEB.deb"
fi
