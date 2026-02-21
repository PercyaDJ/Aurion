# Déploiement Aurion sur Raspberry Pi

Guide détaillé pour déployer Aurion sur une Raspberry Pi 4.

## Pré-requis

### Matériel
- Raspberry Pi 4 (4 Go RAM recommandé)
- Carte SD (16 Go minimum)
- Clé USB formatée en **vfat** ou **exfat** (pour les captures)
- RPi HQ Camera (IMX477) connectée au port CSI
- Alimentation USB-C 5V/3A stable

### Logiciel
- [Raspberry Pi OS Lite (64-bit)](https://www.raspberrypi.com/software/) flashé sur la carte SD
- Accès SSH ou clavier/écran pour le premier démarrage

---

## Étape 1 — Premier démarrage

```bash
# Mise à jour système
sudo apt update && sudo apt upgrade -y

# Activer la caméra (si pas fait dans le Pi Imager)
sudo raspi-config
# → Interface Options → Camera → Enable → Reboot
```

## Étape 2 — Cloner le repo

Le repo étant privé, il faut un **Personal Access Token** GitHub :
1. Sur GitHub : **Settings → Developer settings → Personal access tokens → Tokens (classic)**
2. Cliquer **Generate new token** → cocher `repo` → copier le token

```bash
sudo apt install -y git
git clone https://<TON_TOKEN>@github.com/PercyaDJ/Aurion.git ~/Aurion
cd ~/Aurion
```

## Étape 3 — Bootstrap

Le script `bootstrap.sh` harden le système pour une utilisation terrain :

```bash
sudo bash scripts/bootstrap.sh
```

**Ce que fait le bootstrap :**
- ✅ Log2ram (logs en RAM, préserve la carte SD)
- ✅ `/tmp` en tmpfs
- ✅ Root filesystem en `noatime,commit=60`
- ✅ Journald volatile (50 Mo max en RAM)
- ✅ Désactivation des mises à jour automatiques APT
- ✅ Détection automatique de la clé USB (UUID) + automount `/mnt/capture`
- ✅ Timer `aurion-flush` (sync disque toutes les 2 min)
- ✅ Timer `aurion-power-watch` (arrêt propre si undervoltage)
- ❓ Désactivation optionnelle de zram (prompt interactif)

> **Important** : Insérez la clé USB **avant** de lancer le bootstrap.
> Le script la détecte automatiquement et configure le montage permanent.

```bash
# Reboot obligatoire après le bootstrap
sudo reboot
```

## Étape 4 — Installation

```bash
bash ~/Aurion/scripts/install.sh
```

**Ce que fait l'install :**
1. Installe Rust via rustup
2. Installe les dépendances : `build-essential`, `libssl-dev`, `rpicam-apps`, `hostapd`, `dnsmasq`
3. Compile le projet : `cargo build --release --features rpi` (~15-30 min)
4. Configure hostapd/dnsmasq (désactivés au boot, gérés par l'app)
5. Configure sudoers (commandes hardware sans mot de passe)
6. Crée et active le service systemd `aurion.service`
7. Génère `config/aurion.json` depuis les valeurs par défaut

## Étape 5 — Démarrer

```bash
# Démarrer le service
sudo systemctl start aurion

# Vérifier le statut
sudo systemctl status aurion
```

---

## Commandes utiles

```bash
sudo systemctl start aurion      # Démarrer
sudo systemctl stop aurion       # Arrêter
sudo systemctl restart aurion    # Redémarrer
sudo systemctl status aurion     # Statut
sudo journalctl -u aurion -f     # Logs en temps réel
```

## Test manuel (sans service)

```bash
cd ~/Aurion
./target/release/aurion serve --port 8080
# Ouvrir http://<IP_RPI>:8080 dans un navigateur
```

---

## Utilisation terrain

1. Brancher la Raspberry Pi (alimentation USB-C)
2. Le service `aurion` démarre automatiquement
3. Connecter un smartphone au Wi-Fi **Aurion** (mot de passe : `aurora2024`)
4. Le portail captif s'ouvre automatiquement avec l'interface Aurion
5. Choisir le mode :
   - 🌙 **Capture** : configurer les paramètres, cliquer "Déconnexion" → la capture nocturne démarre
   - 📸 **Récupération** : parcourir la galerie, télécharger les images

### Portail captif

Aurion redirige **toutes** les requêtes DNS vers le Pi (via dnsmasq) et répond
aux URLs de détection captive portal de chaque OS :

| OS | URL testée | Réponse |
|---|---|---|
| iOS/macOS | `/hotspot-detect.html` | Redirect → Dashboard |
| Android | `/generate_204` | Redirect → Dashboard |
| Windows | `/connecttest.txt` | Redirect → Dashboard |
| Firefox | `/canonical.html` | Redirect → Dashboard |

---

## Troubleshooting

| Problème | Solution |
|----------|----------|
| Service ne démarre pas | `sudo journalctl -u aurion -n 50` pour voir les erreurs |
| Caméra non détectée | Vérifier le câble CSI, `rpicam-hello --list-cameras` |
| Wi-Fi ne se crée pas | `sudo journalctl -u aurion -f`, vérifier hostapd |
| USB non montée | `lsblk` pour vérifier, `sudo mount -a` pour forcer le montage |
| Compilation échoue (RAM) | Le script crée un swap temporaire de 2 Go automatiquement |
| Portail captif ne s'ouvre pas | Se connecter manuellement à `http://192.168.4.1:8080` |
