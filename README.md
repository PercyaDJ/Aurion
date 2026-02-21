# Aurion

> 🌌 Caméra autonome de capture d'aurores boréales pour Raspberry Pi 4

Aurion est une application Rust qui transforme un Raspberry Pi 4 équipé d'une HQ Camera (IMX477) en station de capture d'aurores boréales totalement autonome.

## ✨ Fonctionnalités

- **Détection automatique** d'aurores par analyse multi-critères (dominance verte, luminosité, spread spatial)
- **Exposition adaptative** avec calibration par histogramme et anti-yoyo
- **Capture autonome** en mode nuit (Watch → detection → Run → capture continue)
- **Hotspot Wi-Fi** avec portail captif pour configuration depuis un smartphone
- **Interface web** mobile-first avec dashboard, réglages, presets, preview, galerie
- **Mode double** : Capture (soir) ou Récupération (matin, téléchargement des images)
- **Hardening SD** : logging RAM, tmpfs, noatime, protection undervoltage

## 🚀 Quick Start (Raspberry Pi)

```bash
# 1. Flash Raspberry Pi OS Lite sur la carte SD
# 2. Insérer la SD + clé USB dans la RPi, démarrer

# 3. Premier démarrage : mise à jour + clone
sudo apt update && sudo apt upgrade -y
git clone https://github.com/PercyaDJ/Aurion.git ~/Aurion

# 4. Bootstrap (hardening OS + automount USB)
sudo bash ~/Aurion/scripts/bootstrap.sh

# 5. Reboot
sudo reboot

# 6. Installation (Rust + compilation + service)
bash ~/Aurion/scripts/install.sh

# 7. Démarrer le service
sudo systemctl start aurion
```

## 📱 Utilisation sur le terrain

1. **Brancher** la Raspberry Pi
2. **Connecter** votre téléphone au Wi-Fi **Aurion** (mdp: `aurora2024`)
3. Le **portail captif** s'ouvre automatiquement
4. Choisir :
   - 🌙 **Mode Capture** → configurer + déconnecter → la capture démarre
   - 📸 **Mode Récupération** → parcourir et télécharger les images

## 🏗️ Architecture

```
src/
├── core/           # Logique pure (state machine, detection, exposure, config)
├── ports/          # Traits d'interface (camera, storage, network, clock, system)
├── adapters/
│   ├── pc/         # Mocks pour développement sur PC
│   └── rpi/        # Implémentations Raspberry Pi (libcamera, hostapd, mount)
├── web/            # Serveur Axum + API REST + fichiers statiques
│   └── static/     # Interface web (HTML/CSS/JS, dark theme)
└── cli/            # Simulation d'un cycle complet
```

## 💻 Développement PC

```bash
# Lancer le serveur web (avec mocks)
cargo run -- serve --port 8080

# Simuler un cycle complet nuit
cargo run -- simulate

# Tests
cargo test
```

## 📖 Documentation

- [Déploiement RPi détaillé](DEPLOY_RPI.md) — Guide étape par étape
- `config/default.json` — Configuration par défaut

## 📋 Pré-requis matériel

| Composant | Détail |
|-----------|--------|
| Raspberry Pi | 4 Model B (4 Go RAM recommandé) |
| Caméra | RPi HQ Camera (IMX477) |
| Stockage | Clé USB (vfat/exfat, 64 Go+ recommandé) |
| Alimentation | 5V/3A USB-C (stable) |

## 📄 Licence

MIT
