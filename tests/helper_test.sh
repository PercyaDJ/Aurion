#!/usr/bin/env bash
# Tests of scripts/aurion-helper in dry-run mode: argument validation and
# generated commands. Run: bash tests/helper_test.sh
set -uo pipefail
HELPER="$(cd "$(dirname "$0")/.." && pwd)/scripts/aurion-helper"
export AURION_HELPER_DRYRUN=1 AURION_ENV_FILE=/nonexistent
unset SUDO_USER
fails=0; passes=0

ok()   { if out=$("$@" 2>&1); then passes=$((passes+1)); else echo "FAIL (should succeed): $* → $out"; fails=$((fails+1)); fi; }
ko()   { if out=$("$@" 2>&1); then echo "FAIL (should be refused): $* → $out"; fails=$((fails+1)); else passes=$((passes+1)); fi; }
has()  { local pattern="$1"; shift; out=$("$@" 2>&1); if grep -qF -- "$pattern" <<<"$out"; then passes=$((passes+1)); else echo "FAIL: '$pattern' not in output of $* → $out"; fails=$((fails+1)); fi; }
hasnt(){ local pattern="$1"; shift; out=$("$@" 2>&1); if grep -qF -- "$pattern" <<<"$out"; then echo "FAIL: '$pattern' found in output of $*"; fails=$((fails+1)); else passes=$((passes+1)); fi; }
stdin(){ local data="$1"; shift; "$@" <<<"$data"; }

# ap-start
ok    stdin "motdepasse12" "$HELPER" ap-start Aurion 6
has   "mode=ap, 0600" stdin "motdepasse12" "$HELPER" ap-start Aurion 6
hasnt "motdepasse12" stdin "motdepasse12" "$HELPER" ap-start Aurion 6
has   "redirect 80 -> 8080" stdin "motdepasse12" "$HELPER" ap-start Aurion 6
ko    stdin "motdepasse12" "$HELPER" ap-start "" 6
ko    stdin "motdepasse12" "$HELPER" ap-start "$(printf 'Evil\nwpa=0')" 6
ko    stdin "motdepasse12" "$HELPER" ap-start "$(printf 'a%.0s' {1..33})" 6
ko    stdin "motdepasse12" "$HELPER" ap-start Aurion 0
ko    stdin "motdepasse12" "$HELPER" ap-start Aurion 14
ko    stdin "motdepasse12" "$HELPER" ap-start Aurion "6; reboot"
ko    stdin "court" "$HELPER" ap-start Aurion 6
ko    stdin "$(printf 'x%.0s' {1..64})" "$HELPER" ap-start Aurion 6
ko    stdin "$(printf 'mot\tdepasse12')" "$HELPER" ap-start Aurion 6
ok    stdin "motdepasse12" env AURION_FAKE_NM=0 "$HELPER" ap-start "Mon Wifi" 11
has   "hostapd" stdin "motdepasse12" env AURION_FAKE_NM=0 "$HELPER" ap-start Aurion 6
ok    "$HELPER" ap-stop
has   "radio wifi off" "$HELPER" ap-stop

# wifi-connect
has   "192.0.2.10" stdin "motdepasse12" "$HELPER" wifi-connect Maison
has   "mode=infrastructure, 0600" stdin "motdepasse12" "$HELPER" wifi-connect Maison
hasnt "motdepasse12" stdin "motdepasse12" "$HELPER" wifi-connect Maison
ko    stdin "court" "$HELPER" wifi-connect Maison
ko    stdin "motdepasse12" "$HELPER" wifi-connect "$(printf 'x\ny')"

# clock
ok    "$HELPER" set-time 1767225600
has   "@1767225600" "$HELPER" set-time 1767225600
ko    "$HELPER" set-time 0
ko    "$HELPER" set-time "1767225600; reboot"
ko    "$HELPER" set-time 9999999999
# version (checked by the application)
has   "5" "$HELPER" version

# app-rollback: a trial version that keeps failing is replaced by the previous one
APP=$(mktemp -d)
printf 'NEW' >"$APP/aurion"; printf 'OLD' >"$APP/aurion.prev"; printf '1.12.1' >"$APP/aurion.trial"
has   "version précédente rétablie" env AURION_FAKE_APP_DIR="$APP" "$HELPER" app-rollback
ok    test "$(cat "$APP/aurion")" = OLD
ok    test "$(cat "$APP/aurion.failed")" = NEW
ok    test ! -e "$APP/aurion.trial"
has   "1.12.1" cat "$APP/aurion.rolled-back"
# A confirmed version (no trial marker) is never swapped: the failure is elsewhere
printf 'NEW2' >"$APP/aurion"; printf 'OLD2' >"$APP/aurion.prev"
has   "pas de retour arrière" env AURION_FAKE_APP_DIR="$APP" "$HELPER" app-rollback
has   "systemctl start --no-block aurion.service" env AURION_FAKE_APP_DIR="$APP" "$HELPER" app-rollback
ok    test "$(cat "$APP/aurion")" = NEW2
rm -rf "$APP"

# app-update: signed package (application + system files), checked again as root
FR=$(mktemp -d); APP=$(mktemp -d); PK=$(mktemp -d)
mkdir -p "$FR/usr/local/share/aurion/keys" "$FR/usr/local/sbin" "$FR/etc/systemd/system"
openssl genpkey -algorithm ed25519 -out "$PK/projet.key" 2>/dev/null
openssl pkey -in "$PK/projet.key" -pubout -out "$FR/usr/local/share/aurion/keys/aurion-signing.pub"
openssl genpkey -algorithm ed25519 -out "$PK/autre.key" 2>/dev/null
printf 'OLD-HELPER' >"$FR/usr/local/sbin/aurion-helper"; printf 'OLD-UNIT' >"$FR/etc/systemd/system/aurion.service"
printf '#!/bin/sh\necho "aurion 1.12.1"\n' >"$APP/aurion"; chmod +x "$APP/aurion"
make_bundle() { # <dir> <version> [file to leave out]
  local d="$1/p"; rm -rf "$d"; mkdir -p "$d/deploy" "$d/config" "$d/keys"
  printf '#!/bin/sh\necho "aurion %s"\n' "$2" >"$d/aurion"; chmod +x "$d/aurion"
  for f in aurion-helper install.sh deploy/aurion.service deploy/99-aurion-usb.rules config/default.json keys/aurion-signing.pub; do
    [[ "$f" == "${3:-}" ]] || printf 'x' >"$d/$f"
  done
  tar -C "$d" -czf "$1/paquet.tar.gz" .
  openssl pkeyutl -sign -rawin -inkey "$PK/projet.key" -in "$1/paquet.tar.gz" -out "$1/paquet.sig"
}
upd() { env AURION_FAKE_ROOT="$FR" AURION_FAKE_APP_DIR="$APP" "$HELPER" "$@"; }
make_bundle "$PK" 1.13.0
has   "aurion 1.13.0" upd app-update "$PK/paquet.tar.gz" "$PK/paquet.sig"
has   "install.sh --update --user aurion" upd app-update "$PK/paquet.tar.gz" "$PK/paquet.sig"
ok    test "$(cat "$APP/aurion.trial")" = "aurion 1.13.0"
ok    test "$(cat "$FR/var/lib/aurion/prev/pour-version")" = "aurion 1.13.0"
has   "usr/local/sbin/aurion-helper" tar -tf "$FR/var/lib/aurion/prev/fichiers.tar"
has   "etc/systemd/system/aurion.service" tar -tf "$FR/var/lib/aurion/prev/fichiers.tar"
ok    test ! -e "$FR/var/lib/aurion/update"
# Refused: altered package, other key, bad signature file, incomplete package, no such file
cp "$PK/paquet.tar.gz" "$PK/altere.tar.gz"; printf 'x' >>"$PK/altere.tar.gz"
has   "signature refusée" upd app-update "$PK/altere.tar.gz" "$PK/paquet.sig"
openssl pkeyutl -sign -rawin -inkey "$PK/autre.key" -in "$PK/paquet.tar.gz" -out "$PK/autre.sig"
has   "signature refusée" upd app-update "$PK/paquet.tar.gz" "$PK/autre.sig"
printf 'court' >"$PK/court.sig"
ko    upd app-update "$PK/paquet.tar.gz" "$PK/court.sig"
ko    upd app-update "$PK/absent.tar.gz" "$PK/paquet.sig"
ko    upd app-update "$PK/paquet.tar.gz"
ln -s "$PK/paquet.tar.gz" "$PK/lien.tar.gz"
ko    upd app-update "$PK/lien.tar.gz" "$PK/paquet.sig"
make_bundle "$PK" 1.13.0 install.sh
has   "paquet incomplet" upd app-update "$PK/paquet.tar.gz" "$PK/paquet.sig"
# A trial version that keeps failing: system files AND application put back
make_bundle "$PK" 1.13.0
printf '#!/bin/sh\necho "aurion 1.12.1"\n' >"$APP/aurion"
upd app-update "$PK/paquet.tar.gz" "$PK/paquet.sig" >/dev/null 2>&1
printf 'NEW-HELPER' >"$FR/usr/local/sbin/aurion-helper"; printf 'NEW-UNIT' >"$FR/etc/systemd/system/aurion.service"
printf 'NEW-BIN' >"$APP/aurion"
has   "version précédente rétablie" upd app-rollback
ok    test "$(cat "$FR/usr/local/sbin/aurion-helper")" = OLD-HELPER
ok    test "$(cat "$FR/etc/systemd/system/aurion.service")" = OLD-UNIT
has   "aurion 1.12.1" "$APP/aurion" --version
ok    test "$(cat "$APP/aurion.failed")" = NEW-BIN
has   "aurion 1.13.0" cat "$APP/aurion.rolled-back"
ok    test ! -e "$APP/aurion.trial"
ok    test ! -e "$FR/var/lib/aurion/prev"
# Manual rollback (Diagnostics): only with a full copy made for the running version
upd app-update "$PK/paquet.tar.gz" "$PK/paquet.sig" >/dev/null 2>&1
printf '#!/bin/sh\necho "aurion 1.13.0"\n' >"$APP/aurion"
printf 'NEW-HELPER' >"$FR/usr/local/sbin/aurion-helper"
has   "programme et fichiers système" upd app-rollback manual
ok    test "$(cat "$FR/usr/local/sbin/aurion-helper")" = OLD-HELPER
hasnt "systemctl start" upd app-rollback manual
out=$(upd app-rollback manual 2>&1); code=$?
if [[ $code -eq 3 ]]; then passes=$((passes+1)); else echo "FAIL: app-rollback manual sans copie → code $code ($out)"; fails=$((fails+1)); fi
ko    upd app-rollback autre
rm -rf "$FR" "$APP" "$PK"

# usb-format: USB disks only, never the SD card or the system disk
SB=$(mktemp -d); mkdir -p "$SB/dev/usb1/1-1/host0/block/sda" "$SB/dev/mmc/block/mmcblk0" "$SB/block"
ln -s "$SB/dev/usb1/1-1/host0/block/sda" "$SB/block/sda"
ln -s "$SB/dev/mmc/block/mmcblk0" "$SB/block/mmcblk0"
has   "mkfs.exfat -L AURION /dev/sda1" env AURION_FAKE_SYSBLOCK="$SB/block" "$HELPER" usb-format sda
has   "wipefs" env AURION_FAKE_SYSBLOCK="$SB/block" "$HELPER" usb-format sda
ko    env AURION_FAKE_SYSBLOCK="$SB/block" "$HELPER" usb-format mmcblk0
ko    env AURION_FAKE_SYSBLOCK="$SB/block" "$HELPER" usb-format sdb
ko    env AURION_FAKE_SYSBLOCK="$SB/block" "$HELPER" usb-format "sda; reboot"
ko    env AURION_FAKE_SYSBLOCK="$SB/block" AURION_FAKE_ROOTDEV=/dev/sda2 "$HELPER" usb-format sda
mkdir -p "$SB/dev/usb1/1-2/host1/block/sdb"; ln -s "$SB/dev/usb1/1-2/host1/block/sdb" "$SB/block/sdb"
ko    env AURION_FAKE_SYSBLOCK="$SB/block" "$HELPER" usb-format sda   # two keys: refused
# Pi booting from a USB SSD (sda): the key (sdb) is the only one, the SSD is never touched
has   "mkfs.exfat -L AURION /dev/sdb1" env AURION_FAKE_SYSBLOCK="$SB/block" AURION_FAKE_ROOTDEV=/dev/sda2 "$HELPER" usb-format sdb
ko    env AURION_FAKE_SYSBLOCK="$SB/block" AURION_FAKE_ROOTDEV=/dev/sda2 "$HELPER" usb-format sda
rm -rf "$SB"
# A SATA/PCIe disk listed after the key does not hide it
SB=$(mktemp -d); mkdir -p "$SB/dev/usb1/1-1/host0/block/sda" "$SB/dev/pcie/ata1/block/sdb" "$SB/block"
ln -s "$SB/dev/usb1/1-1/host0/block/sda" "$SB/block/sda"
ln -s "$SB/dev/pcie/ata1/block/sdb" "$SB/block/sdb"
has   "mkfs.exfat -L AURION /dev/sda1" env AURION_FAKE_SYSBLOCK="$SB/block" "$HELPER" usb-format sda
rm -rf "$SB"

# power-profile (night energy saving)
CPU_FAKE=$(mktemp -d); NET_FAKE=$(mktemp -d)
mkdir -p "$CPU_FAKE/policy0" "$NET_FAKE/eth0"
echo ondemand >"$CPU_FAKE/policy0/scaling_governor"
echo "ondemand powersave performance" >"$CPU_FAKE/policy0/scaling_available_governors"
echo 0 >"$NET_FAKE/eth0/carrier"
ok    env AURION_FAKE_CPUFREQ="$CPU_FAKE" AURION_FAKE_NET="$NET_FAKE" "$HELPER" power-profile watch
ok    test "$(cat "$CPU_FAKE/policy0/scaling_governor")" = powersave
has   "eth0 down" env AURION_FAKE_CPUFREQ="$CPU_FAKE" AURION_FAKE_NET="$NET_FAKE" "$HELPER" power-profile capture
ok    test "$(cat "$CPU_FAKE/policy0/scaling_governor")" = ondemand
echo 1 >"$NET_FAKE/eth0/carrier"
out=$(env AURION_FAKE_CPUFREQ="$CPU_FAKE" AURION_FAKE_NET="$NET_FAKE" "$HELPER" power-profile watch 2>&1)
ok    test "${out/eth0 down/}" = "$out"   # cable plugged: Ethernet kept
has   "eth0 up" env AURION_FAKE_CPUFREQ="$CPU_FAKE" AURION_FAKE_NET="$NET_FAKE" "$HELPER" power-profile day
ko    "$HELPER" power-profile turbo
ko    "$HELPER" power-profile "watch; reboot"
rm -rf "$CPU_FAKE" "$NET_FAKE"

# rtc-wake (Raspberry Pi 5 power-on alarm)
RTC_FAKE=$(mktemp -d); : >"$RTC_FAKE/wakealarm"
WAKE_AT=$(( $(date +%s) + 3600 ))
ok    env AURION_FAKE_RTC="$RTC_FAKE" "$HELPER" rtc-wake "$WAKE_AT"
ok    test "$(cat "$RTC_FAKE/wakealarm")" = "$WAKE_AT"
ko    env AURION_FAKE_RTC="$RTC_FAKE" "$HELPER" rtc-wake 1704067200
ko    env AURION_FAKE_RTC="$RTC_FAKE" "$HELPER" rtc-wake "$(( $(date +%s) + 30 * 86400 ))"
ko    env AURION_FAKE_RTC="$RTC_FAKE" "$HELPER" rtc-wake "$WAKE_AT; reboot"
ko    "$HELPER" rtc-wake "$WAKE_AT"
rm -rf "$RTC_FAKE"
ok    "$HELPER" set-timezone Europe/Paris
ok    "$HELPER" set-timezone America/Argentina/Buenos_Aires
ok    "$HELPER" set-timezone UTC
ko    "$HELPER" set-timezone ../../etc/shadow
ko    "$HELPER" set-timezone "/etc/passwd"
ko    "$HELPER" set-timezone "Europe/Paris;reboot"

# usb
has   "systemd-mount" env AURION_FAKE_FSTYPE=exfat "$HELPER" usb-add sda1
has   "--fsck=yes" env AURION_FAKE_FSTYPE=exfat "$HELPER" usb-add sda1
has   "flush" env AURION_FAKE_FSTYPE=vfat "$HELPER" usb-add sdb
ko    env AURION_FAKE_FSTYPE=ext4 "$HELPER" usb-add sda1
ko    "$HELPER" usb-add "../../dev/mmcblk0"
ko    "$HELPER" usb-add "mmcblk0p2"
ko    env AURION_FAKE_FSTYPE=vfat AURION_FAKE_ROOTDEV=/dev/sda2 "$HELPER" usb-add sda1   # boot partition of a USB SSD
has   "systemd-mount" env AURION_FAKE_FSTYPE=exfat AURION_FAKE_ROOTDEV=/dev/sda2 "$HELPER" usb-add sdb1
ok    "$HELPER" mount-usb

# misc
has   "poweroff" "$HELPER" shutdown
ko    "$HELPER" rm -rf /
ko    "$HELPER"

# Test hooks must be ignored through sudo. Use a command that is harmless
# for real: an unknown time zone is accepted in dry-run (syntax only) but
# refused for real (not in /usr/share/zoneinfo) or refused for non-root.
# NEVER use set-time here: as root it would really change the clock.
ok    "$HELPER" set-timezone Aurion/Nowhere_Test
ko    env SUDO_USER=pi AURION_HELPER_DRYRUN=1 "$HELPER" set-timezone Aurion/Nowhere_Test

rm -rf "${TMPDIR:-/tmp}"/aurion-helper-dryrun.*

echo "aurion-helper: $passes OK, $fails échec(s)"
[[ $fails -eq 0 ]]
