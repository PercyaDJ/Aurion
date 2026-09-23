#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
# Aurion — installation / mise à jour sur Raspberry Pi (une seule commande)
#
#   sudo ./install.sh                 # installe ou met à jour
#   sudo ./install.sh --uninstall     # désinstalle (garde les photos)
#
# Options :
#   --binary <fichier>   binaire aurion à installer (défaut : ./aurion)
#   --user <nom>         utilisateur du service (défaut : celui qui lance sudo)
#   --country <XX>       pays Wi-Fi (défaut : FR)
#   --no-hardening       ne pas optimiser le système pour la carte SD
#   --no-power-saving    garder Bluetooth, audio et LED actifs
#   --no-packages        ne pas lancer apt (installation depuis le paquet .deb)
#   --no-start           ne pas démarrer le service à la fin
#
# Réinstaller par-dessus une version existante conserve la configuration
# et le mot de passe Wi-Fi. Aucune compilation n'est nécessaire.
# ─────────────────────────────────────────────────────────────
set -euo pipefail

# Test hook: install into a fake root instead of "/" (tests/install_test.sh).
R="${AURION_TEST_ROOT:-}"

INSTALL_DIR="$R/opt/aurion"
CONFIG_DIR="$INSTALL_DIR/config"
HELPER="$R/usr/local/sbin/aurion-helper"
MOUNT_POINT=/mnt/capture
WEB_PORT=8080

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY=""
SERVICE_USER="${SUDO_USER:-}"
COUNTRY="FR"
HARDENING=1
POWER_SAVING=1
PACKAGES=1
START=1
UNINSTALL=0

info() { printf '\n\033[1;32m[+]\033[0m %s\n' "$*"; }
warn() { printf '\n\033[1;33m[!]\033[0m %s\n' "$*"; }
die()  { printf '\n\033[1;31m[x]\033[0m %s\n' "$*" >&2; exit 1; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --binary) BINARY="$2"; shift 2 ;;
    --user) SERVICE_USER="$2"; shift 2 ;;
    --country) COUNTRY="$2"; shift 2 ;;
    --no-hardening) HARDENING=0; shift ;;
    --no-power-saving) POWER_SAVING=0; shift ;;
    --no-packages) PACKAGES=0; shift ;;
    --no-start) START=0; shift ;;
    --uninstall) UNINSTALL=1; shift ;;
    -y|--yes) shift ;;
    -h|--help) sed -n '2,21p' "$0"; exit 0 ;;
    *) die "Option inconnue : $1" ;;
  esac
done

# Locate a file shipped next to this script (release archive) or in the repo.
find_file() {
  local c
  for c in "$@"; do
    [[ -e "$c" ]] && { echo "$c"; return 0; }
  done
  return 1
}

backup_file() {
  local f="$1"
  [[ -f "$f" && ! -f "$f.aurion-bak" ]] && cp -a "$f" "$f.aurion-bak"
  return 0
}

# ─── Checks ──────────────────────────────────────────────────

preflight() {
  [[ $EUID -eq 0 ]] || die "Lancez avec sudo : sudo $0"
  [[ "$(uname -m)" == "aarch64" ]] || warn "Architecture $(uname -m) : Aurion est prévu pour Raspberry Pi OS 64 bits (aarch64)."
  [[ -n "$SERVICE_USER" ]] || SERVICE_USER=$(getent passwd 1000 | cut -d: -f1 || true)
  [[ -n "$SERVICE_USER" && "$SERVICE_USER" != "root" ]] || die "Utilisateur du service introuvable : utilisez --user <nom>"
  id "$SERVICE_USER" >/dev/null 2>&1 || die "L'utilisateur '$SERVICE_USER' n'existe pas"
  [[ "$COUNTRY" =~ ^[A-Z]{2}$ ]] || die "Code pays invalide : $COUNTRY"

  if [[ -z "$BINARY" ]]; then
    BINARY=$(find_file "$HERE/aurion" "$HERE/../target/aarch64-unknown-linux-musl/release/aurion" \
      "$HERE/../target/release/aurion") || die "Binaire aurion introuvable. Utilisez --binary <fichier> ou l'archive de release."
  fi
  [[ -f "$BINARY" ]] || die "Binaire introuvable : $BINARY"
  chmod +x "$BINARY"
  "$BINARY" --version >/dev/null 2>&1 || die "Le binaire $BINARY ne démarre pas sur cette machine (mauvaise architecture ?)"

  HELPER_SRC=$(find_file "$HERE/aurion-helper" "$HERE/../scripts/aurion-helper") || die "aurion-helper introuvable"
  SERVICE_SRC=$(find_file "$HERE/deploy/aurion.service" "$HERE/../deploy/aurion.service") || die "aurion.service introuvable"
  UDEV_SRC=$(find_file "$HERE/deploy/99-aurion-usb.rules" "$HERE/../deploy/99-aurion-usb.rules") || die "règle udev introuvable"
  DEFAULT_CFG=$(find_file "$HERE/config/default.json" "$HERE/../config/default.json") || die "config/default.json introuvable"
}

# ─── Packages ────────────────────────────────────────────────

install_packages() {
  info "Installation des paquets système"
  # dosfstools / exfatprogs: repair of the USB key after a power cut
  local pkgs=(rpicam-apps iw nftables rfkill dosfstools exfatprogs)
  if systemctl is-active --quiet NetworkManager 2>/dev/null; then
    pkgs+=(dnsmasq-base)
  else
    pkgs+=(hostapd dnsmasq wpasupplicant)
  fi
  if ! DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "${pkgs[@]}"; then
    warn "apt-get install a échoué (pas d'internet ?). Nouvel essai après apt-get update…"
    apt-get update || warn "apt-get update impossible"
    DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "${pkgs[@]}" \
      || warn "Paquets manquants : ${pkgs[*]} — la caméra ou le hotspot peuvent ne pas fonctionner"
  fi
  # The standalone dnsmasq/hostapd services must not grab wlan0 at boot:
  # Aurion starts them itself when needed.
  if systemctl list-unit-files dnsmasq.service >/dev/null 2>&1; then
    systemctl disable --now dnsmasq.service 2>/dev/null || true
  fi
  if systemctl list-unit-files hostapd.service >/dev/null 2>&1; then
    systemctl unmask hostapd.service 2>/dev/null || true
    systemctl disable --now hostapd.service 2>/dev/null || true
  fi
}

# ─── Files ───────────────────────────────────────────────────

random_password() {
  # 14 characters without ambiguous ones (0/O, 1/l/I)
  # (tr is killed by SIGPIPE when head exits: ignore it under pipefail)
  local pw
  pw=$(LC_ALL=C tr -dc 'ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnpqrstuvwxyz23456789' </dev/urandom 2>/dev/null | head -c 14 || true)
  [[ ${#pw} -eq 14 ]] || die "Génération du mot de passe impossible"
  printf '%s' "$pw"
}

install_files() {
  info "Installation dans $INSTALL_DIR (utilisateur du service : $SERVICE_USER)"
  local group
  group=$(id -gn "$SERVICE_USER")
  install -d -o "$SERVICE_USER" -g "$group" -m 0755 "$INSTALL_DIR"
  install -d -o "$SERVICE_USER" -g "$group" -m 0700 "$CONFIG_DIR"

  # Binary owned by the service user: web updates (OTA) need no root.
  if [[ -f "$INSTALL_DIR/aurion" ]]; then
    cp -a "$INSTALL_DIR/aurion" "$INSTALL_DIR/aurion.prev"
  fi
  install -o "$SERVICE_USER" -g "$group" -m 0755 "$BINARY" "$INSTALL_DIR/aurion.new"
  mv -f "$INSTALL_DIR/aurion.new" "$INSTALL_DIR/aurion"

  install -D -o root -g root -m 0755 "$HELPER_SRC" "$HELPER"
  mkdir -p "$R/etc"
  cat >"$R/etc/aurion.env" <<EOF
# Aurion — lu par /usr/local/sbin/aurion-helper (root). Ne pas rendre modifiable par l'utilisateur.
AURION_USER=$SERVICE_USER
MOUNT_POINT=$MOUNT_POINT
WEB_PORT=$WEB_PORT
EOF
  chmod 0644 "$R/etc/aurion.env"

  # Config: keep the existing one, migrate an old ~/Aurion install, or
  # create a new one with a unique Wi-Fi password.
  local cfg="$CONFIG_DIR/aurion.json"
  local home old
  home=$(getent passwd "$SERVICE_USER" | cut -d: -f6)
  old="$home/Aurion/config/aurion.json"
  if [[ ! -f "$cfg" && -f "$old" ]]; then
    info "Reprise de la configuration existante ($old)"
    cp "$old" "$cfg"
    [[ -d "$home/Aurion/config/presets" ]] && cp -r "$home/Aurion/config/presets" "$CONFIG_DIR/" || true
  fi
  if [[ ! -f "$cfg" ]]; then
    WIFI_PASSWORD=$(random_password)
    sed "s/\"password\": \"aurora2024\"/\"password\": \"$WIFI_PASSWORD\"/" "$DEFAULT_CFG" >"$cfg"
    NEW_PASSWORD=1
  fi
  chown -R "$SERVICE_USER:$group" "$CONFIG_DIR"
  chmod 0600 "$cfg"
  if ! sudo -u "$SERVICE_USER" "$INSTALL_DIR/aurion" --config-dir "$CONFIG_DIR" check-config >/dev/null 2>&1; then
    warn "La configuration $cfg est invalide : sauvegarde en .invalid et remplacement par les valeurs par défaut"
    mv "$cfg" "$cfg.invalid"
    WIFI_PASSWORD=$(random_password)
    sed "s/\"password\": \"aurora2024\"/\"password\": \"$WIFI_PASSWORD\"/" "$DEFAULT_CFG" >"$cfg"
    NEW_PASSWORD=1
    chown "$SERVICE_USER:$group" "$cfg"; chmod 0600 "$cfg"
  fi

  # Camera access
  getent group video >/dev/null && usermod -aG video "$SERVICE_USER"
  getent group render >/dev/null && usermod -aG render "$SERVICE_USER"

  # Capture directory (the USB key is mounted over it)
  mkdir -p "$R$MOUNT_POINT"
  mountpoint -q "$R$MOUNT_POINT" || chown "$SERVICE_USER:$group" "$R$MOUNT_POINT" || true
}

install_sudoers() {
  info "Droits root limités au seul aurion-helper"
  local tmp
  tmp=$(mktemp)
  cat >"$tmp" <<EOF
# Aurion : le service ne peut lancer en root QUE ce script, qui valide
# lui-même chaque argument. (Remplace l'ancienne liste cp/mount/dnsmasq…
# qui donnait de fait un accès root complet.)
$SERVICE_USER ALL=(root) NOPASSWD: /usr/local/sbin/aurion-helper
EOF
  visudo -cf "$tmp" >/dev/null || { rm -f "$tmp"; die "Fichier sudoers invalide"; }
  install -D -o root -g root -m 0440 "$tmp" "$R/etc/sudoers.d/aurion"
  rm -f "$tmp"
}

install_usb_automount() {
  info "Montage automatique de n'importe quelle clé USB sur $MOUNT_POINT"
  install -D -o root -g root -m 0644 "$UDEV_SRC" "$R/etc/udev/rules.d/99-aurion-usb.rules"
  # Old installs bound the capture dir to ONE key (UUID in fstab)
  if grep -qE "[[:space:]]${MOUNT_POINT}[[:space:]]" "$R/etc/fstab" 2>/dev/null; then
    backup_file "$R/etc/fstab"
    sed -i -E "\|[[:space:]]${MOUNT_POINT}[[:space:]]|d" "$R/etc/fstab"
    systemctl daemon-reload
  fi
  udevadm control --reload-rules || true
  if ! mountpoint -q "$R$MOUNT_POINT"; then
    "$HELPER" mount-usb >/dev/null 2>&1 || warn "Aucune clé USB détectée pour l'instant (elle sera montée dès son branchement)"
  fi
}

install_network() {
  info "Configuration du hotspot et du portail captif"
  rfkill unblock wifi 2>/dev/null || true
  if command -v raspi-config >/dev/null 2>&1; then
    raspi-config nonint do_wifi_country "$COUNTRY" >/dev/null 2>&1 || true
  fi
  if systemctl is-active --quiet NetworkManager 2>/dev/null; then
    # DNS of the hotspot answers the Pi address for every domain, so phones
    # open the Aurion page automatically (captive portal).
    install -d -m 0755 "$R/etc/NetworkManager/dnsmasq-shared.d"
    echo "address=/#/192.168.4.1" >"$R/etc/NetworkManager/dnsmasq-shared.d/aurion-captive.conf"
  fi
}

install_service() {
  info "Service systemd aurion.service"
  mkdir -p "$R/etc/systemd/system"
  sed "s/@AURION_USER@/$SERVICE_USER/" "$SERVICE_SRC" >"$R/etc/systemd/system/aurion.service"
  chmod 0644 "$R/etc/systemd/system/aurion.service"
  systemctl daemon-reload
  systemctl enable aurion.service >/dev/null
}

# ─── SD card endurance (safe, idempotent) ────────────────────

harden_system() {
  info "Optimisation du système pour une utilisation sur le terrain"

  # Logs in RAM (50 Mo max)
  install -d "$R/etc/systemd/journald.conf.d"
  printf '[Journal]\nStorage=volatile\nRuntimeMaxUse=50M\n' >"$R/etc/systemd/journald.conf.d/aurion.conf"
  systemctl restart systemd-journald || true

  # No automatic apt runs in the field
  systemctl disable --now apt-daily.timer apt-daily-upgrade.timer >/dev/null 2>&1 || true

  # Root filesystem without access time updates
  if awk '$2=="/" && $3=="ext4" && $4 !~ /noatime/ {found=1} END {exit !found}' "$R/etc/fstab" 2>/dev/null; then
    backup_file "$R/etc/fstab"
    awk 'BEGIN{OFS="\t"} $2=="/" && $3=="ext4" && $4 !~ /noatime/ {$4=$4",noatime"} {print}' "$R/etc/fstab" >"$R/etc/fstab.aurion"
    mv "$R/etc/fstab.aurion" "$R/etc/fstab"
  fi

  # /tmp in RAM (already the case on Trixie)
  if ! findmnt -n -o FSTYPE /tmp | grep -q tmpfs && ! grep -qE '^\s*tmpfs\s+/tmp\s' "$R/etc/fstab" 2>/dev/null; then
    backup_file "$R/etc/fstab"
    echo 'tmpfs /tmp tmpfs defaults,noatime,nosuid,nodev,size=200m 0 0' >>"$R/etc/fstab"
  fi

  # Hardware watchdog: reboots the Pi if the system freezes
  local boot_cfg="$R/boot/firmware/config.txt"
  [[ -f "$boot_cfg" ]] || boot_cfg="$R/boot/config.txt"
  if [[ -f "$boot_cfg" ]] && ! grep -q '^dtparam=watchdog=on' "$boot_cfg"; then
    backup_file "$boot_cfg"
    echo 'dtparam=watchdog=on' >>"$boot_cfg"
  fi
  install -d "$R/etc/systemd/system.conf.d"
  printf '[Manager]\nRuntimeWatchdogSec=15s\nRebootWatchdogSec=5min\n' >"$R/etc/systemd/system.conf.d/aurion-watchdog.conf"

  # Periodic flush of the USB key (power cut safety)
  cat >"$R/etc/systemd/system/aurion-flush.service" <<'EOF'
[Unit]
Description=Aurion : écriture périodique des données sur la clé USB

[Service]
Type=oneshot
ExecStart=/bin/sync
EOF
  cat >"$R/etc/systemd/system/aurion-flush.timer" <<'EOF'
[Unit]
Description=Aurion : sync toutes les 2 minutes

[Timer]
OnBootSec=5min
OnUnitActiveSec=2min

[Install]
WantedBy=timers.target
EOF

  # Clean shutdown on PERSISTENT under-voltage only (bit 0 = under-voltage
  # right now, 3 checks in a row). The previous script reacted to any
  # historical flag and could power the Pi off right after every boot.
  cat >"$R/usr/local/sbin/aurion-power-watch" <<'EOF'
#!/bin/bash
state=/run/aurion-undervoltage
t=$(vcgencmd get_throttled 2>/dev/null | cut -d= -f2) || exit 0
[[ -n "$t" ]] || exit 0
if (( t & 0x1 )); then
  n=$(( $(cat "$state" 2>/dev/null || echo 0) + 1 ))
  echo "$n" >"$state"
  if (( n >= 3 )); then
    logger "Aurion: sous-tension persistante ($t), arrêt propre"
    sync; systemctl poweroff
  fi
else
  rm -f "$state"
fi
EOF
  chmod 0755 "$R/usr/local/sbin/aurion-power-watch"
  cat >"$R/etc/systemd/system/aurion-power-watch.service" <<'EOF'
[Unit]
Description=Aurion : surveillance de l'alimentation

[Service]
Type=oneshot
ExecStart=/usr/local/sbin/aurion-power-watch
EOF
  cat >"$R/etc/systemd/system/aurion-power-watch.timer" <<'EOF'
[Unit]
Description=Aurion : surveillance de l'alimentation toutes les 20 s

[Timer]
OnBootSec=60
OnUnitActiveSec=20

[Install]
WantedBy=timers.target
EOF
  rm -f "$R/usr/local/bin/aurion-power-watch.sh" "$R/usr/local/bin/aurion-flush.sh"
  systemctl daemon-reload
  systemctl enable --now aurion-flush.timer aurion-power-watch.timer >/dev/null 2>&1 || true
}

# ─── Battery: switch off what a field camera does not need ───

POWER_BEGIN="# >>> Aurion : économie d'énergie (retiré par install.sh --uninstall)"
POWER_END="# <<< Aurion"

power_saving() {
  info "Économie d'énergie : Bluetooth, audio et LED désactivés"
  local boot_cfg="$R/boot/firmware/config.txt"
  [[ -f "$boot_cfg" ]] || boot_cfg="$R/boot/config.txt"
  if [[ -f "$boot_cfg" ]]; then
    backup_file "$boot_cfg"
    # Replace a previous block (idempotent)
    sed -i "/^$POWER_BEGIN/,/^$POWER_END/d" "$boot_cfg"
    cat >>"$boot_cfg" <<CFG
$POWER_BEGIN
# Bluetooth et audio inutiles sur le terrain
dtoverlay=disable-bt
dtparam=audio=off
# LED éteintes : moins de consommation, aucune lumière parasite près de l'objectif
dtparam=act_led_trigger=none
dtparam=act_led_activelow=off
dtparam=pwr_led_trigger=none
dtparam=pwr_led_activelow=off
$POWER_END
CFG
  else
    warn "config.txt introuvable : économie d'énergie matérielle non appliquée"
  fi
  local svc
  for svc in bluetooth.service hciuart.service ModemManager.service triggerhappy.service triggerhappy.socket; do
    systemctl disable --now "$svc" >/dev/null 2>&1 || true
  done
}

# ─── Uninstall ───────────────────────────────────────────────

uninstall() {
  [[ $EUID -eq 0 ]] || die "Lancez avec sudo : sudo $0 --uninstall"
  info "Désinstallation d'Aurion (les photos de la clé USB sont conservées)"
  systemctl disable --now aurion.service aurion-flush.timer aurion-power-watch.timer 2>/dev/null || true
  "$HELPER" ap-stop >/dev/null 2>&1 || true
  rm -f "$R"/etc/systemd/system/aurion.service "$R"/etc/systemd/system/aurion-flush.* "$R"/etc/systemd/system/aurion-power-watch.*
  rm -f "$R/etc/sudoers.d/aurion" "$R/etc/udev/rules.d/99-aurion-usb.rules" "$R/etc/aurion.env" "$HELPER" "$R/usr/local/sbin/aurion-power-watch"
  local bc
  for bc in "$R/boot/firmware/config.txt" "$R/boot/config.txt"; do
    [[ -f "$bc" ]] && sed -i "/^$POWER_BEGIN/,/^$POWER_END/d" "$bc"
  done
  rm -f "$R/etc/NetworkManager/dnsmasq-shared.d/aurion-captive.conf" "$R/etc/systemd/journald.conf.d/aurion.conf" "$R/etc/systemd/system.conf.d/aurion-watchdog.conf"
  nmcli connection delete aurion-ap >/dev/null 2>&1 || true
  systemctl daemon-reload
  udevadm control --reload-rules 2>/dev/null || true
  info "Supprimé. La configuration reste dans $CONFIG_DIR (rm -rf $INSTALL_DIR pour tout effacer)."
}

# ─── Main ────────────────────────────────────────────────────

main() {
  if [[ $UNINSTALL -eq 1 ]]; then uninstall; exit 0; fi

  echo "════════════════════════════════════════════"
  echo "  Aurion — installation"
  echo "════════════════════════════════════════════"
  preflight
  local version
  version=$("$BINARY" --version | awk '{print $2}')
  info "Version à installer : $version"

  local was_running=0
  systemctl is-active --quiet aurion.service 2>/dev/null && was_running=1
  [[ $was_running -eq 1 ]] && { info "Arrêt de la version en cours"; systemctl stop aurion.service || true; }

  [[ $PACKAGES -eq 1 ]] && install_packages
  install_files
  install_sudoers
  install_usb_automount
  install_network
  install_service
  [[ $HARDENING -eq 1 ]] && harden_system
  [[ $POWER_SAVING -eq 1 ]] && power_saving

  local ssid password
  ssid=$(grep -o '"ssid": *"[^"]*"' "$CONFIG_DIR/aurion.json" | sed 's/.*: *"//; s/"$//')
  password=$(grep -o '"password": *"[^"]*"' "$CONFIG_DIR/aurion.json" | sed 's/.*: *"//; s/"$//')

  echo
  echo "════════════════════════════════════════════"
  echo "  ✅ Aurion $version installé"
  echo "════════════════════════════════════════════"
  echo "  Wi-Fi        : $ssid"
  echo "  Mot de passe : $password"
  [[ "${NEW_PASSWORD:-0}" == "1" ]] && echo "                 (généré pour cet appareil : notez-le !)"
  echo "  Interface    : http://192.168.4.1:$WEB_PORT (s'ouvre seule sur le téléphone)"
  echo "  Photos       : clé USB branchée → $MOUNT_POINT"
  echo
  echo "  Logs         : sudo journalctl -u aurion -f"
  echo "  Redémarrer   : sudo systemctl restart aurion"
  echo
  echo "  Le hotspot remplace le Wi-Fi de la maison : la connexion SSH par"
  echo "  Wi-Fi va se couper. Utilisez le Wi-Fi \"$ssid\" ou un câble réseau."
  echo "════════════════════════════════════════════"

  # Started last and without waiting: when installing over Wi-Fi SSH the
  # hotspot takes wlan0 over, the summary above must be printed before.
  if [[ $START -eq 1 ]]; then
    info "Démarrage d'Aurion dans 5 secondes"
    systemd-run --quiet --on-active=5 --unit=aurion-start-after-install systemctl restart aurion.service 2>/dev/null \
      || systemctl restart --no-block aurion.service
  fi
}

main
