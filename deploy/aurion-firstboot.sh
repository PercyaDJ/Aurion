#!/bin/bash
# Aurion : installation automatique au premier démarrage de l'image SD.
# Lancé une seule fois par aurion-firstboot.service, puis désactivé.
set -euo pipefail
exec >>/var/log/aurion-firstboot.log 2>&1
echo "=== Aurion premier démarrage $(date) ==="

# Dedicated system account for the service (no login)
if ! id aurion >/dev/null 2>&1; then
  useradd --system --create-home --home-dir /var/lib/aurion --shell /usr/sbin/nologin aurion
fi

# Packages are already in the image: no internet needed.
# Default Wi-Fi password kept (printed in the quick start guide); the
# interface asks to change it.
bash /usr/lib/aurion/install.sh --binary /usr/lib/aurion/aurion --user aurion \
  --no-packages --default-wifi-password

systemctl disable aurion-firstboot.service
rm -f /usr/lib/aurion/firstboot-pending
echo "=== Aurion installé ==="
