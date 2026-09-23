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
Aurion $VERSION — Raspberry Pi 4/5, Raspberry Pi OS 64 bits

Installation ou mise à jour (sur le Raspberry Pi) :
    tar xzf $NAME.tar.gz
    sudo ./$NAME/install.sh

À la fin, le mot de passe du Wi-Fi "Aurion" s'affiche : notez-le.
Mise à jour ultérieure possible depuis le téléphone :
Diagnostics → Mise à jour du logiciel → fichier "aurion" de cette archive.
TXT

tar -C dist -czf "dist/$NAME.tar.gz" "$NAME"
( cd dist && sha256sum "$NAME.tar.gz" >"$NAME.tar.gz.sha256" )
echo "✅ dist/$NAME.tar.gz"
