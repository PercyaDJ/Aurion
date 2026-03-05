# 🌌 Aurion

> Caméra autonome de capture d'aurores boréales pour Raspberry Pi 4

Aurion transforme un Raspberry Pi 4 équipé d'une HQ Camera (IMX477) en station de capture d'aurores boréales totalement autonome. L'interface web permet de configurer, lancer et récupérer les captures depuis un smartphone.

---

## ✨ Fonctionnalités

### Capture intelligente
- **Détection multi-couleur** — Vert, rouge, violet avec seuils indépendants
- **Masque lune** — Exclusion automatique des blobs lumineux (lune, lampadaires)
- **Hystérésis** — Transitions on/off anti-flickering pour éviter les faux positifs
- **Vérification spatiale** — Anti phares (détection de dispersion des sources lumineuses)
- **2 modes** : SAFE (capture toute la nuit) ou FILTER (détection → capture)

### Exposition adaptative
- **Auto-exposition EMA** avec lissage pour des timelapses fluides
- **Rate limiting par phase** — 3-5% en Run vs 15-20% en Calibration
- **Priorité shutter → ISO** — Shutter d'abord, ISO en compensation
- **Calibration** automatique au démarrage (3 frames)

### Interface web
- **9 pages** — Dashboard, Preview, Settings, Réglages avancés, Presets, Stockage, Galerie, Diagnostics
- **Mobile-first** — Design dark premium, responsive
- **PWA** — Installable sur l'écran d'accueil (manifest + icône)
- **Toasts** — Notifications visuelles sur actions et erreurs
- **Portail captif** — Ouverture automatique sur iOS, Android, Windows, Firefox

### Stockage et récupération
- **Sauvegarde USB** — Images JPEG, RAW (DNG), ou les deux
- **Thumbnails** — Pré-générés en 320×240 pour la galerie
- **Galerie** — Sélection multiple, aperçu, suppression avec confirmation
- **ZIP download** — Téléchargement batch nommé `aurion_YYYY-MM-DD.zip`
- **Session events** — Logging NDJSON de chaque capture (ISO, shutter, détection)

### Réseau
- **Hotspot Wi-Fi** — Créé au démarrage, coupé en capture
- **Portail captif** — Redirige tous les OS vers le dashboard
- **Sécurité** — Mot de passe WiFi masqué dans l'API

### Robustesse terrain
- **Hardening SD** — Log2ram, tmpfs, noatime, journald volatil
- **Warning USB** — Alerte si la clé USB n'est pas montée
- **Intervalle configurable** — Pour des timelapses à cadence régulière
- **Arrêt propre** — Sync + unmount + shutdown système

---

## 🚀 Quick Start

### Raspberry Pi

```bash
# 1. Flash Raspberry Pi OS Lite (64-bit) sur la carte SD
# 2. Insérer la SD + clé USB + caméra CSI, démarrer
# 3. Se connecter en SSH

# Mise à jour + clone
sudo apt update && sudo apt upgrade -y
sudo apt install -y git
git clone https://<TOKEN>@github.com/PercyaDJ/Aurion.git ~/Aurion

# Installation Tout-en-un (Bootstrap + Compilation + Reboot auto)
bash ~/Aurion/scripts/setup.sh
```

### Développement PC

```bash
# Serveur web avec mocks caméra
cargo run -- serve --port 8080

# Simulation cycle nuit complet
cargo run -- simulate

# Tests
cargo test    # 39 tests
```

---

## 📱 Utilisation terrain

1. **Brancher** la Raspberry Pi (USB-C 5V/3A)
2. **Connecter** le téléphone au Wi-Fi **Aurion** (mdp: `aurora2024`)
3. Le **portail captif** s'ouvre automatiquement
4. Choisir :
   - 🌙 **Mode Capture** — Configurer → Déconnexion → Capture autonome toute la nuit
   - 📸 **Mode Récupération** — Parcourir la galerie, sélectionner, télécharger en ZIP

---

## 🏗️ Architecture

Architecture hexagonale (ports/adapters) — le cœur métier est testable sans matériel.

```
src/
├── core/               # Logique pure (0 dépendance I/O)
│   ├── config.rs       # 7 structs de config, 3 presets builtin
│   ├── models.rs       # CaptureFrame, Phase, DetectionResult, etc.
│   ├── state_machine.rs # Boot → Arm → Disconnect → Calibration → Watch/Run → Shutdown
│   ├── detection.rs    # Multi-couleur, moon mask, hystérésis, spatial spread
│   ├── exposure.rs     # EMA auto-exposure, histogramme, rate limiting
│   ├── orchestrator.rs # Boucle principale, sauvegarde, thumbnails
│   └── session_logger.rs # NDJSON event logger
├── ports/              # Traits abstraits
│   ├── camera.rs       # CameraPort (capture_jpg, capture_raw)
│   ├── storage.rs      # StoragePort (save_file, mount, sync)
│   ├── system.rs       # SystemPort (shutdown)
│   ├── network.rs      # NetworkApPort (start_ap, stop_ap)
│   └── clock.rs        # ClockPort (now)
├── adapters/
│   ├── pc/             # Mocks pour développement local
│   └── rpi/            # Raspberry Pi (rpicam-still, hostapd, mount)
├── web/                # Serveur Axum + 35 routes API
│   ├── api.rs          # Endpoints REST
│   ├── mod.rs          # Router + static files
│   └── static/         # 9 pages HTML, CSS, JS
└── cli/                # Commandes (run, serve, simulate)
```

### Machine d'état

```
Boot → Arm → Disconnect → Calibration ─┬→ Watch ─→ Run → Shutdown
                                        └→ Run (SAFE mode) → Shutdown
```

---

## ⚙️ Configuration

Fichier : `config/aurion.json`

| Section | Paramètre | Défaut | Description |
|---------|-----------|--------|-------------|
| **Exposure** | `iso_min` / `iso_max` | 100 / 3200 | Plage ISO |
| | `shutter_min_us` / `shutter_max_us` | 1M / 30M | Plage obturateur (µs) |
| | `target_brightness` | 60 | Luminosité cible histogramme |
| **Detection** | `green_threshold` | 15.0 | Seuil dominance verte |
| | `red_threshold` / `blue_threshold` | 10.0 / 8.0 | Seuils rouge et violet |
| | `roi_top_percent` | 65 | Zone d'analyse (% du haut) |
| | `consecutive_required` | 2 | Détections consécutives pour confirmer |
| | `detection_capture_enabled` | false | `false` = SAFE, `true` = FILTER |
| | `moon_mask_enabled` | true | Masque lune (anti faux positifs) |
| **Capture** | `watch_interval_secs` | 60 | Intervalle en Watch (s) |
| | `capture_interval_secs` | 10 | Intervalle en Run pour timelapse (s) |
| | `output_format` | RawDng | `Jpg`, `RawDng`, ou `RawAndJpg` |
| **Time Range** | `start` / `end` | 21:00 / 06:00 | Plage horaire de surveillance |
| | `duration_hours` | null | Mode minuteur (alternative à plage) |
| **Storage** | `mount_point` | /mnt/capture | Point de montage USB |
| | `warning_percent` | 15 | Seuil alerte stockage (%) |
| **Network** | `ssid` | Aurion | Nom du hotspot Wi-Fi |
| | `password` | aurora2024 | Mot de passe Wi-Fi |

### Presets builtin

| Preset | ISO max | Shutter max | Seuil vert | Usage |
|--------|---------|-------------|------------|-------|
| **FullDark** | 3200 | 30s | 15 | Ciel noir, pas de pollution |
| **SemiPolluted** | 1600 | 15s | 20 | Pollution lumineuse modérée |
| **Moonlight** | 800 | 10s | 25 | Pleine lune |

---

## 🌐 API REST

| Méthode | Route | Description |
|---------|-------|-------------|
| GET | `/api/status` | Phase, heure, stockage, warnings, USB |
| GET | `/api/config` | Configuration (password masqué) |
| POST | `/api/config` | Mise à jour configuration |
| GET | `/api/config/wifi-password` | Mot de passe WiFi réel |
| GET | `/api/preview` | Dernière preview (JPEG) |
| POST | `/api/preview/capture` | Capturer une preview |
| GET | `/api/presets` | Liste des presets |
| POST | `/api/presets` | Sauvegarder un preset |
| POST | `/api/presets/{name}/apply` | Appliquer un preset |
| POST | `/api/disconnect` | Déconnexion → lancer capture |
| GET | `/api/storage` | Info stockage |
| GET | `/api/logs` | Logs temps réel |
| GET | `/api/gallery` | Liste des images |
| GET | `/api/gallery/stats` | Statistiques galerie |
| GET | `/api/gallery/{file}` | Télécharger une image |
| GET | `/api/gallery/thumbnail/{file}` | Thumbnail d'une image |
| POST | `/api/gallery/delete` | Supprimer des images |
| POST | `/api/gallery/download-zip` | Télécharger un ZIP |
| GET | `/api/diagnostics` | Infos système (CPU, RAM, uptime) |
| POST | `/api/system/time` | Régler l'heure système |
| POST | `/api/system/shutdown` | Arrêt système |
| GET | `/api/wifi/scan` | Scanner les réseaux WiFi |
| POST | `/api/wifi/connect` | Se connecter à un réseau |
| GET | `/api/wifi/status` | Statut connexion WiFi |

---

## 📋 Pré-requis matériel

| Composant | Détail |
|-----------|--------|
| Raspberry Pi | 4 Model B (4 Go RAM recommandé) |
| Caméra | RPi HQ Camera (IMX477) + objectif grand angle |
| Stockage | Clé USB (vfat/exfat, 64 Go+ recommandé) |
| Alimentation | 5V/3A USB-C stable (batterie externe recommandée) |
| Carte SD | 16 Go minimum (Raspberry Pi OS Lite) |

---

## 🧪 Tests

```bash
cargo test
# running 39 tests
# test result: ok. 39 passed; 0 failed
```

Tests couvrant : config (validation, serialization), state machine (lifecycle, transitions), detection (multi-couleur, lune, hystérésis, spatial), exposure (EMA, rate limiting, histogramme), orchestrator (intégration), adapters (mocks).

---

## 📖 Documentation

- [Déploiement RPi détaillé](DEPLOY_RPI.md) — Guide étape par étape
- `config/default.json` — Configuration par défaut
- `scripts/bootstrap.sh` — Hardening OS + automount USB
- `scripts/install.sh` — Installation complète (Rust, compilation, service)

---

## 📄 Licence

MIT
