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
has   "802-11-wireless.mode ap" stdin "motdepasse12" "$HELPER" ap-start Aurion 6
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
ko    stdin "court" "$HELPER" wifi-connect Maison
ko    stdin "motdepasse12" "$HELPER" wifi-connect "$(printf 'x\ny')"

# clock
ok    "$HELPER" set-time 1767225600
has   "@1767225600" "$HELPER" set-time 1767225600
ko    "$HELPER" set-time 0
ko    "$HELPER" set-time "1767225600; reboot"
ko    "$HELPER" set-time 9999999999
ok    "$HELPER" set-timezone Europe/Paris
ok    "$HELPER" set-timezone America/Argentina/Buenos_Aires
ok    "$HELPER" set-timezone UTC
ko    "$HELPER" set-timezone ../../etc/shadow
ko    "$HELPER" set-timezone "/etc/passwd"
ko    "$HELPER" set-timezone "Europe/Paris;reboot"

# usb
has   "systemd-mount" env AURION_FAKE_FSTYPE=exfat "$HELPER" usb-add sda1
has   "flush" env AURION_FAKE_FSTYPE=vfat "$HELPER" usb-add sdb
ko    env AURION_FAKE_FSTYPE=ext4 "$HELPER" usb-add sda1
ko    "$HELPER" usb-add "../../dev/mmcblk0"
ko    "$HELPER" usb-add "mmcblk0p2"
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
