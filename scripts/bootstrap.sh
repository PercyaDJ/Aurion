#!/usr/bin/env bash
set -euo pipefail

# ─────────────────────────────────────────────────────────────
# Aurion Bootstrap (interactive, one-shot)
# https://github.com/PercyaDJ/Aurion
#
# Hardens Raspberry Pi OS Lite for field use (SD endurance):
# - /var/log and journald to RAM, /tmp tmpfs, root noatime+commit=60
# - Disables apt-daily auto timers (mask)
# - Configures USB capture automount (vfat/exfat) on /mnt/capture
# - Creates aurion-flush + aurion-power-watch timers
# - Installs git and libclang-dev for Rust compilation
# - Optionally disables zram (interactive prompt, skip with --yes)
#
# Usage:
#   sudo bash scripts/bootstrap.sh          # interactive
#   sudo bash scripts/bootstrap.sh --yes    # non-interactive (skip prompts)
# ─────────────────────────────────────────────────────────────

AUTO_YES=false
if [[ "${1:-}" == "--yes" || "${1:-}" == "-y" ]]; then
  AUTO_YES=true
fi

info() { echo -e "\n[+] $*\n"; }
warn() { echo -e "\n[!] $*\n"; }
die()  { echo -e "\n[✗] $*\n" >&2; exit 1; }

require_root() {
  [[ "${EUID}" -eq 0 ]] || die "Run as root: sudo bash $0"
}

backup_file() {
  local f="$1"
  [[ -f "$f" ]] && cp -a "$f" "${f}.bak.$(date +%F-%H%M%S)"
}

ask_yes_no() {
  local prompt="$1"
  local default="${2:-y}" # y or n
  local ans=""
  while true; do
    if [[ "$default" == "y" ]]; then
      read -r -p "$prompt [Y/n] " ans || true
      ans="${ans:-Y}"
    else
      read -r -p "$prompt [y/N] " ans || true
      ans="${ans:-N}"
    fi
    case "${ans,,}" in
      y|yes) return 0 ;;
      n|no)  return 1 ;;
      *) echo "Please answer yes or no." ;;
    esac
  done
}

ensure_packages() {
  info "Installing packages: log2ram, util-linux, exfat support, git, build deps"
  apt update
  apt install -y --no-install-recommends log2ram util-linux exfat-fuse exfat-utils git libclang-dev
}

enable_log2ram() {
  info "Enabling log2ram"
  systemctl enable --now log2ram
}

patch_fstab_root() {
  info "Ensuring / is mounted with noatime,commit=60"
  backup_file /etc/fstab

  awk '
    BEGIN{OFS="\t"}
    $2=="/" && $3=="ext4" {
      opts=$4
      if (opts !~ /(^|,)noatime(,|$)/)  opts=opts ",noatime"
      if (opts !~ /(^|,)commit=60(,|$)/) opts=opts ",commit=60"
      gsub(/^,/, "", opts)
      $4=opts
    }
    {print}
  ' /etc/fstab > /etc/fstab.tmp

  mv /etc/fstab.tmp /etc/fstab
  chmod 644 /etc/fstab
}

ensure_tmpfs_tmp() {
  info "Ensuring /tmp is tmpfs"
  backup_file /etc/fstab
  if ! grep -qE '^\s*tmpfs\s+/tmp\s+tmpfs\s+' /etc/fstab; then
    echo 'tmpfs /tmp tmpfs defaults,noatime,nosuid,nodev,size=200m 0 0' >> /etc/fstab
  fi
}

configure_journald() {
  info "Configuring journald to use RAM (volatile)"
  backup_file /etc/systemd/journald.conf
  touch /etc/systemd/journald.conf

  sed -i -E '/^\s*#?\s*Storage\s*=/d' /etc/systemd/journald.conf
  sed -i -E '/^\s*#?\s*SystemMaxUse\s*=/d' /etc/systemd/journald.conf
  sed -i -E '/^\s*#?\s*RuntimeMaxUse\s*=/d' /etc/systemd/journald.conf

  cat >> /etc/systemd/journald.conf <<'EOF'

# Aurion hardening
Storage=volatile
SystemMaxUse=50M
RuntimeMaxUse=50M
EOF

  systemctl restart systemd-journald
}

disable_apt_auto() {
  info "Disabling automatic APT timers (apt-daily)"
  systemctl stop apt-daily.timer apt-daily-upgrade.timer 2>/dev/null || true
  systemctl mask apt-daily.service apt-daily-upgrade.service apt-daily.timer apt-daily-upgrade.timer 2>/dev/null || true
}

setup_capture_automount() {
  info "Setting up /mnt/capture automount"
  mkdir -p /mnt/capture

  # Find USB vfat/exfat partition directly (no subshells, no pipes)
  local target_dev="" target_uuid="" target_fstype=""

  local disk part ft uu
  for disk in $(lsblk -dnpo NAME,TRAN | awk '$2=="usb"{print $1}'); do
    for part in $(lsblk -lnpo NAME "$disk"); do
      [[ "$part" == "$disk" ]] && continue
      ft=$(blkid -s TYPE -o value "$part" 2>/dev/null) || true
      uu=$(blkid -s UUID -o value "$part" 2>/dev/null) || true
      if [[ "$ft" == "vfat" || "$ft" == "exfat" ]] && [[ -n "$uu" ]]; then
        target_dev="$part"
        target_uuid="$uu"
        target_fstype="$ft"
        break 2
      fi
    done
  done

  if [[ -z "$target_uuid" ]]; then
    warn "No USB vfat/exfat drive detected."
    echo "  Plug your USB capture drive and re-run this script."
    return 0
  fi

  info "Found USB: $target_dev (UUID=$target_uuid, FSTYPE=$target_fstype)"

  backup_file /etc/fstab
  sed -i -E '\|/mnt/capture|d' /etc/fstab
  # Detect uid/gid of the aurion user so the USB drive is writable without sudo
  local AURION_UID AURION_GID
  AURION_UID=$(id -u "${SUDO_USER:-aurion}" 2>/dev/null || echo "1000")
  AURION_GID=$(id -g "${SUDO_USER:-aurion}" 2>/dev/null || echo "1000")

  # uid/gid required for vfat/exfat: without these the mount is root:root and aurion cannot write
  echo "UUID=${target_uuid}  /mnt/capture  ${target_fstype}  defaults,noatime,nofail,uid=${AURION_UID},gid=${AURION_GID},umask=0002  0  0" >> /etc/fstab

  systemctl daemon-reload
  mount -a || true
  ls /mnt/capture >/dev/null 2>&1 || true
  info "USB automount configured on /mnt/capture (UUID=$target_uuid)."
}

create_aurion_flush() {
  info "Creating aurion-flush timer (sync every 2 minutes)"
  cat > /usr/local/bin/aurion-flush.sh <<'EOF'
#!/bin/bash
sync
EOF
  chmod +x /usr/local/bin/aurion-flush.sh

  cat > /etc/systemd/system/aurion-flush.service <<'EOF'
[Unit]
Description=Aurion periodic disk flush (sync)

[Service]
Type=oneshot
ExecStart=/usr/local/bin/aurion-flush.sh
EOF

  cat > /etc/systemd/system/aurion-flush.timer <<'EOF'
[Unit]
Description=Run Aurion flush every 2 minutes

[Timer]
OnBootSec=5min
OnUnitActiveSec=2min

[Install]
WantedBy=timers.target
EOF

  systemctl daemon-reload
  systemctl enable --now aurion-flush.timer
}

create_aurion_power_watch() {
  info "Creating aurion-power-watch timer (shutdown on undervoltage)"
  cat > /usr/local/bin/aurion-power-watch.sh <<'EOF'
#!/bin/bash
THROTTLED=$(vcgencmd get_throttled 2>/dev/null || true)

# If vcgencmd is unavailable, do nothing.
[[ -n "$THROTTLED" ]] || exit 0

if [[ "$THROTTLED" != "throttled=0x0" ]]; then
  logger "Aurion: Undervoltage detected ($THROTTLED), shutting down safely."
  sync
  sleep 3
  shutdown -h now
fi
EOF
  chmod +x /usr/local/bin/aurion-power-watch.sh

  cat > /etc/systemd/system/aurion-power-watch.service <<'EOF'
[Unit]
Description=Aurion Power Watch

[Service]
Type=oneshot
ExecStart=/usr/local/bin/aurion-power-watch.sh
EOF

  cat > /etc/systemd/system/aurion-power-watch.timer <<'EOF'
[Unit]
Description=Run Aurion Power Watch every 30s

[Timer]
OnBootSec=30
OnUnitActiveSec=30

[Install]
WantedBy=timers.target
EOF

  systemctl daemon-reload
  systemctl enable --now aurion-power-watch.timer
}

maybe_disable_zram_interactive() {
  if ! swapon --show | grep -q zram; then
    info "No zram swap detected."
    return 0
  fi

  # Non-interactive: keep zram
  if $AUTO_YES; then
    info "Keeping zram enabled (non-interactive mode)."
    return 0
  fi

  if ask_yes_no "ZRAM swap is enabled (RAM-compressed swap). Keep it enabled?" "y"; then
    info "Keeping zram enabled."
    return 0
  fi

  warn "Disabling zram swap (best-effort)."
  swapoff -a 2>/dev/null || true

  # Try common units; ignore failures.
  systemctl stop systemd-zram-setup@zram0.service 2>/dev/null || true
  systemctl mask systemd-zram-setup@zram0.service 2>/dev/null || true

  info "zram disable attempted. After reboot, check: swapon --show"
}

add_shutdown_alias() {
  info "Adding shutdown alias 'off' for current user (optional convenience)"
  local user_home
  user_home="$(getent passwd "${SUDO_USER:-root}" | cut -d: -f6)"
  [[ -d "$user_home" ]] || return 0
  local bashrc="${user_home}/.bashrc"
  if ! grep -q "alias off='sudo shutdown -h now'" "$bashrc" 2>/dev/null; then
    echo "alias off='sudo shutdown -h now'" >> "$bashrc"
  fi
}

final_report() {
  info "═══ Aurion Bootstrap Complete ═══"
  echo ""
  echo "[mount]"
  mount | egrep "on / |/var/log| on /tmp " || true
  echo
  echo "[swap]"
  swapon --show || true
  echo
  echo "[aurion timers]"
  systemctl list-timers | grep aurion || true
  echo
  echo "[throttled]"
  vcgencmd get_throttled 2>/dev/null || true
  echo
  warn "Reboot recommended: sudo reboot"
  echo "After reboot, run: bash ~/Aurion/scripts/install.sh"
}

main() {
  require_root
  ensure_packages
  enable_log2ram
  patch_fstab_root
  ensure_tmpfs_tmp
  configure_journald
  disable_apt_auto
  setup_capture_automount
  create_aurion_flush
  create_aurion_power_watch
  maybe_disable_zram_interactive
  add_shutdown_alias
  final_report
}

main "$@"
