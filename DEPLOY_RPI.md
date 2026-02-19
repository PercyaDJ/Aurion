# Déploiement AuroraCam sur Raspberry Pi

## Prérequis matériel

- Raspberry Pi 4 Model B (4 Go RAM)
- Caméra RPi HQ (IMX477) connectée au port CSI
- Clé USB 128 Go formatée en ext4 ou exFAT
- Carte microSD avec Raspberry Pi OS Lite (64-bit Bookworm)
- Connexion réseau (Ethernet ou Wi-Fi temporaire) pour l'installation

---

## Étape 1 — Cloner le repo

```bash
cd ~
git clone https://github.com/PercyaDJ/Aurion.git
cd Aurion
```

---

## Étape 2 — Installer Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source ~/.cargo/env
rustc --version
```

---

## Étape 3 — Installer les dépendances système

```bash
sudo apt update && sudo apt upgrade -y
sudo apt install -y \
  build-essential \
  pkg-config \
  libssl-dev \
  libcamera-apps \
  hostapd \
  dnsmasq
```

> **Note :** `libcamera-apps` fournit `libcamera-still` pour la capture RAW/JPG.

---

## Étape 4 — Activer la caméra

```bash
# Vérifier que la caméra est détectée
libcamera-hello --list-cameras

# Si aucune caméra n'apparaît :
sudo raspi-config
# → Interface Options → Camera → Enable
# Puis rebooter
sudo reboot
```

---

## Étape 5 — Préparer le point de montage USB

```bash
sudo mkdir -p /mnt/usb

# Identifier la clé USB
lsblk

# Montage manuel (adapter sda1 si besoin)
sudo mount /dev/sda1 /mnt/usb

# Vérifier
df -h /mnt/usb
```

### Montage automatique au boot (optionnel)

```bash
# Récupérer l'UUID de la clé
sudo blkid /dev/sda1

# Ajouter dans /etc/fstab (adapter l'UUID et le type)
echo "UUID=VOTRE-UUID /mnt/usb ext4 defaults,nofail 0 2" | sudo tee -a /etc/fstab
```

---

## Étape 6 — Compiler le projet

```bash
cd ~/Aurion
cargo build --release --features rpi
```

> ⏱ La première compilation sur RPi 4 peut prendre **15–30 minutes**.
> Le binaire sera dans `target/release/aurora-cam`.

---

## Étape 7 — Configurer hostapd et dnsmasq

### hostapd

```bash
sudo systemctl unmask hostapd
sudo systemctl disable hostapd
# Le logiciel gère le démarrage/arrêt de hostapd lui-même
```

### dnsmasq

```bash
sudo systemctl disable dnsmasq
# Idem, géré par le logiciel
```

### Permettre l'exécution sans mot de passe sudo

```bash
sudo visudo
# Ajouter à la fin :
# pi ALL=(ALL) NOPASSWD: /usr/bin/hostapd, /usr/bin/killall, /sbin/shutdown, /bin/mount, /bin/umount, /bin/ip, /usr/bin/dnsmasq
```

> Adapter `pi` au nom de votre utilisateur.

---

## Étape 8 — Test rapide

```bash
cd ~/Aurion

# Tester la simulation (sans hardware)
./target/release/aurora-cam simulate

# Tester le serveur web
./target/release/aurora-cam serve --port 8080
# Ouvrir http://<IP-DU-PI>:8080 depuis un navigateur
```

---

## Étape 9 — Créer un service systemd

```bash
sudo tee /etc/systemd/system/aurora-cam.service > /dev/null << 'EOF'
[Unit]
Description=AuroraCam - Autonomous Aurora Capture
After=network.target

[Service]
Type=simple
User=pi
WorkingDirectory=/home/pi/Aurion
ExecStart=/home/pi/Aurion/target/release/aurora-cam serve
Restart=on-failure
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable aurora-cam
sudo systemctl start aurora-cam
```

> Adapter `pi` et les chemins si votre utilisateur est différent.

### Commandes utiles

```bash
sudo systemctl status aurora-cam    # Vérifier le statut
sudo journalctl -u aurora-cam -f    # Voir les logs en temps réel
sudo systemctl restart aurora-cam   # Redémarrer
sudo systemctl stop aurora-cam      # Arrêter
```

---

## Étape 10 — Connexion sur le terrain

1. Alimenter le Raspberry Pi
2. Le service `aurora-cam` démarre automatiquement
3. Se connecter au réseau Wi-Fi **AuroraCam** (mot de passe : `aurora2024`)
4. **L'interface s'ouvre automatiquement** grâce au portail captif ! 🎉
5. Configurer les paramètres
6. Cliquer **Déconnexion** → la capture démarre automatiquement

> **Comment ça marche ?** Quand votre téléphone se connecte au Wi-Fi, il teste sa
> connectivité en contactant des serveurs connus (Google, Apple, Microsoft...).
> AuroraCam redirige *toutes* les requêtes DNS vers le Pi (via dnsmasq) et répond
> aux URLs de test avec une redirection vers `http://192.168.4.1:8080/`.
> Le téléphone détecte un "portail captif" et ouvre automatiquement l'interface.
>
> Fonctionne sur : **iOS, Android, Windows, macOS, Firefox**

---

## Résolution de problèmes

| Problème | Solution |
|---|---|
| `libcamera-still` pas trouvé | `sudo apt install libcamera-apps` |
| Caméra non détectée | Vérifier le câble CSI, `sudo raspi-config` → activer caméra |
| Compilation échoue (mémoire) | Ajouter du swap : `sudo fallocate -l 2G /swapfile && sudo mkswap /swapfile && sudo swapon /swapfile` |
| Wi-Fi AP ne démarre pas | Vérifier `sudo hostapd -dd /tmp/aurora_hostapd.conf` |
| USB non montée | `lsblk` pour identifier le device, `sudo mount /dev/sdX1 /mnt/usb` |
| Permission denied (shutdown) | Vérifier la config `visudo` (étape 7) |
