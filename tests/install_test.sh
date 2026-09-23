#!/usr/bin/env bash
# End-to-end simulation of scripts/install.sh inside a fake root directory
# with stubbed system commands (apt, systemd, udev). Needs root (file
# ownership); in CI: sudo -E bash tests/install_test.sh
# Usage: bash tests/install_test.sh [path/to/aurion/binary]
set -euo pipefail
REPO="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${1:-$REPO/target/debug/aurion}"
[[ -x "$BIN" ]] || { echo "binaire absent: $BIN (cargo build)"; exit 1; }
[[ $EUID -eq 0 ]] || { echo "à lancer en root (sudo)"; exit 1; }

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
export AURION_TEST_ROOT="$WORK/root"
export AURION_HELPER_DRYRUN=1
unset SUDO_USER
mkdir -p "$AURION_TEST_ROOT/etc" "$AURION_TEST_ROOT/boot/firmware" "$WORK/stubs"

# Old install artefacts that must be cleaned up
cat >"$AURION_TEST_ROOT/etc/fstab" <<'FSTAB'
proc            /proc           proc    defaults          0       0
PARTUUID=1234-01  /boot/firmware  vfat    defaults          0       2
PARTUUID=1234-02  /               ext4    defaults          0       1
UUID=ABCD-1234  /mnt/capture  vfat  defaults,noatime,nofail,uid=1000,gid=1000,umask=0002  0  0
FSTAB
echo "arm_64bit=1" >"$AURION_TEST_ROOT/boot/firmware/config.txt"

# Stubs: record calls instead of touching the machine
for cmd in apt-get systemctl udevadm raspi-config rfkill usermod systemd-run; do
  printf '#!/bin/sh\necho "%s $*" >>"%s/calls.log"\nexit 0\n' "$cmd" "$WORK" >"$WORK/stubs/$cmd"
  chmod +x "$WORK/stubs/$cmd"
done
# sudo -u <user> cmd... → run cmd directly
printf '#!/bin/sh\n[ "$1" = "-u" ] && shift 2\nexec "$@"\n' >"$WORK/stubs/sudo"; chmod +x "$WORK/stubs/sudo"
command -v visudo >/dev/null || { printf '#!/bin/sh\nexit 0\n' >"$WORK/stubs/visudo"; chmod +x "$WORK/stubs/visudo"; }
export PATH="$WORK/stubs:$PATH"

fails=0
check() { if eval "$2"; then echo "  ok  $1"; else echo "  FAIL $1"; fails=$((fails+1)); fi; }

echo "▶ première installation"
bash "$REPO/scripts/install.sh" --binary "$BIN" --user nobody >"$WORK/out1.log" 2>&1 || { cat "$WORK/out1.log"; exit 1; }
R="$AURION_TEST_ROOT"
CFG="$R/opt/aurion/config/aurion.json"
# shellcheck disable=SC2034  # used inside check()
PW=$(grep -o '"password": *"[^"]*"' "$CFG" | sed 's/.*: *"//; s/"$//')

check "binaire installé et exécutable" '[[ -x "$R/opt/aurion/aurion" ]] && cmp -s "$BIN" "$R/opt/aurion/aurion"'
check "binaire appartient à l utilisateur du service (mises à jour OTA)" '[[ $(stat -c %U "$R/opt/aurion/aurion") == nobody ]]'
check "config créée en 0600" '[[ $(stat -c %a "$CFG") == 600 ]]'
check "mot de passe Wi-Fi unique généré" '[[ ${#PW} -eq 14 && "$PW" != aurora2024 ]]'
check "config valide pour le binaire" '"$BIN" --config-dir "$R/opt/aurion/config" check-config >/dev/null'
check "mot de passe affiché dans le résumé" 'grep -qF "$PW" "$WORK/out1.log"'
check "helper root installé" '[[ -x "$R/usr/local/sbin/aurion-helper" && $(stat -c %U "$R/usr/local/sbin/aurion-helper") == root ]]'
check "sudoers limité au helper" '[[ $(grep -v "^#" "$R/etc/sudoers.d/aurion" | grep -c NOPASSWD) -eq 1 ]] && grep -q "NOPASSWD: /usr/local/sbin/aurion-helper$" "$R/etc/sudoers.d/aurion"'
check "sudoers sans cp/mount/dnsmasq" '! grep -v "^#" "$R/etc/sudoers.d/aurion" | grep -Eq "/(cp|mount|dnsmasq|killall|date)\b"'
check "sudoers en 0440" '[[ $(stat -c %a "$R/etc/sudoers.d/aurion") == 440 ]]'
check "unité systemd pour nobody" 'grep -q "^User=nobody$" "$R/etc/systemd/system/aurion.service" && ! grep -q "@AURION_USER@" "$R/etc/systemd/system/aurion.service"'
check "service de type notify + watchdog" 'grep -q "^Type=notify" "$R/etc/systemd/system/aurion.service" && grep -q "^WatchdogSec=" "$R/etc/systemd/system/aurion.service"'
check "pas de dépendance dure à la clé USB" '! grep -q RequiresMountsFor "$R/etc/systemd/system/aurion.service"'
check "règle udev de montage USB" 'grep -q "aurion-helper usb-add" "$R/etc/udev/rules.d/99-aurion-usb.rules"'
check "ancienne ligne fstab liée à une clé supprimée" '! grep -q "/mnt/capture" "$R/etc/fstab"'
check "fstab sauvegardé" '[[ -f "$R/etc/fstab.aurion-bak" ]]'
check "racine en noatime" 'awk "\$2==\"/\" {print \$4}" "$R/etc/fstab" | grep -q noatime'
check "watchdog matériel activé" 'grep -q "^dtparam=watchdog=on" "$R/boot/firmware/config.txt"'
check "surveillance alimentation corrigée (bit 0 seulement)" 'grep -q "t & 0x1" "$R/usr/local/sbin/aurion-power-watch"'
check "journald en RAM" 'grep -q "Storage=volatile" "$R/etc/systemd/journald.conf.d/aurion.conf"'
check "service activé et démarré" 'grep -q "systemctl enable aurion.service" "$WORK/calls.log" && grep -q "systemd-run.*restart aurion.service" "$WORK/calls.log"'
check "paquets installés" 'grep -q "apt-get install.*rpicam-apps" "$WORK/calls.log"'
check "outils de réparation de la clé USB installés" 'grep -q "apt-get install.*dosfstools exfatprogs" "$WORK/calls.log"'
check "économie d énergie : Bluetooth, audio, LED" 'grep -q "^dtoverlay=disable-bt" "$R/boot/firmware/config.txt" && grep -q "^dtparam=audio=off" "$R/boot/firmware/config.txt" && grep -q "^dtparam=act_led_trigger=none" "$R/boot/firmware/config.txt"'
check "services inutiles désactivés" 'grep -q "systemctl disable --now bluetooth.service" "$WORK/calls.log"'

echo "▶ réinstallation (mise à jour)"
bash "$REPO/scripts/install.sh" --binary "$BIN" --user nobody >"$WORK/out2.log" 2>&1 || { cat "$WORK/out2.log"; exit 1; }
# shellcheck disable=SC2034
PW2=$(grep -o '"password": *"[^"]*"' "$CFG" | sed 's/.*: *"//; s/"$//')
check "mot de passe conservé" '[[ "$PW" == "$PW2" ]]'
check "ancienne version gardée en secours" '[[ -f "$R/opt/aurion/aurion.prev" ]]'
check "pas de doublon dans config.txt" '[[ $(grep -c "^dtparam=watchdog=on" "$R/boot/firmware/config.txt") -eq 1 ]]'
check "bloc énergie non dupliqué" '[[ $(grep -c "^dtoverlay=disable-bt" "$R/boot/firmware/config.txt") -eq 1 ]]'

echo "▶ mode paquet (.deb) : pas d apt"
: >"$WORK/calls.log"
bash "$REPO/scripts/install.sh" --binary "$BIN" --user nobody --no-packages >"$WORK/out2b.log" 2>&1 || { cat "$WORK/out2b.log"; exit 1; }
check "aucun appel apt en mode paquet" '! grep -q "^apt-get" "$WORK/calls.log"'
check "pas de doublon noatime" '! grep -q "noatime,noatime" "$R/etc/fstab"'

echo "▶ configuration invalide réparée"
echo '{"broken": true}' >"$CFG"
bash "$REPO/scripts/install.sh" --binary "$BIN" --user nobody --no-hardening >"$WORK/out3.log" 2>&1 || { cat "$WORK/out3.log"; exit 1; }
check "config invalide sauvegardée" '[[ -f "$CFG.invalid" ]]'
check "nouvelle config valide" '"$BIN" --config-dir "$R/opt/aurion/config" check-config >/dev/null'

echo "▶ désinstallation"
bash "$REPO/scripts/install.sh" --uninstall >"$WORK/out4.log" 2>&1 || { cat "$WORK/out4.log"; exit 1; }
check "service supprimé" '[[ ! -f "$R/etc/systemd/system/aurion.service" ]]'
check "sudoers supprimé" '[[ ! -f "$R/etc/sudoers.d/aurion" ]]'
check "helper supprimé" '[[ ! -f "$R/usr/local/sbin/aurion-helper" ]]'
check "configuration conservée" '[[ -f "$CFG" ]]'
check "bloc énergie retiré de config.txt" '! grep -q "disable-bt" "$R/boot/firmware/config.txt" && grep -q "^arm_64bit=1" "$R/boot/firmware/config.txt"'

echo
if [[ $fails -eq 0 ]]; then echo "install.sh : tous les contrôles OK"; else echo "install.sh : $fails échec(s)"; exit 1; fi
