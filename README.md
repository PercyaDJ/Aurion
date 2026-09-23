# Aurion

Caméra autonome de capture d'aurores boréales pour Raspberry Pi 4/5 et HQ Camera (IMX477).
Le Pi crée son propre Wi-Fi ; depuis le téléphone on règle, on lance la nuit, puis on récupère les photos.

## Installation sur le Raspberry Pi (5 minutes, sans compilation)

Prérequis : Raspberry Pi OS **64 bits** (Lite ou Desktop), caméra branchée, clé USB en exFAT ou FAT32.

**Option A : depuis le Pi (avec internet)**
```bash
curl -fsSL https://raw.githubusercontent.com/PercyaDJ/Aurion/main/scripts/get.sh | sudo bash
```
Dépôt privé : voir l'en-tête de `scripts/get.sh` (jeton GitHub en lecture seule).

**Option B : depuis le PC**
1. Télécharger `aurion-<version>-rpi-arm64.tar.gz` dans l'onglet *Releases* du dépôt.
2. Windows (PowerShell) : `.\scripts\deploy.ps1 pi@aurion.local .\aurion-1.5.0-rpi-arm64.tar.gz`
   Linux / macOS : `scripts/deploy.sh pi@aurion.local aurion-1.5.0-rpi-arm64.tar.gz`

**Option C : à la main**
```bash
tar xzf aurion-1.5.0-rpi-arm64.tar.gz
sudo ./aurion-1.5.0-rpi-arm64/install.sh
```

À la fin, l'installeur affiche **le nom et le mot de passe du Wi-Fi** (générés pour cet appareil) : notez-les.
Réinstaller par-dessus une version existante conserve la configuration.

**Mise à jour sans câble ni SSH** : Diagnostics, *Mise à jour du logiciel*, choisir le fichier `aurion` de la nouvelle archive.
Le binaire est vérifié (format, architecture, démarrage) avant d'être installé, et l'ancien est gardé en secours (`/opt/aurion/aurion.prev`).

Guide détaillé et dépannage : [DEPLOY_RPI.md](DEPLOY_RPI.md).

## Utilisation sur le terrain

1. Brancher le Pi (USB-C 5 V / 3 A) avec la clé USB.
2. Se connecter au Wi-Fi **Aurion** : la page s'ouvre toute seule (portail captif), sinon `http://192.168.4.1:8080`.
   L'heure du Pi est réglée automatiquement sur celle du téléphone (le Pi n'a pas d'horloge sauvegardée).
3. **Capture** : régler, puis *Déconnexion, lancer la capture*. Le Wi-Fi se coupe au bout de 15 s, la nuit se déroule seule et le Pi s'éteint à la fin.
4. **Récupération** : rallumer, aller dans *Galerie*, télécharger une nuit complète en ZIP.

Deux modes :
- **SAFE** (défaut) : capture toute la nuit, les images avec aurore sont marquées `_AURORA`.
- **FILTER** : surveille le ciel et ne commence à enregistrer qu'après N détections consécutives.

## Développement sur PC

```bash
cargo run -- serve --port 8080      # interface web avec caméra simulée
cargo run -- simulate               # nuit simulée dans la console
cargo test                          # tous les tests Rust
```

Tests complets (comme la CI) :
```bash
cargo clippy --all-targets -- -D warnings
bash tests/helper_test.sh                         # helper root, validation des arguments
sudo -E bash tests/install_test.sh target/debug/aurion   # installeur simulé dans une fausse racine
cd tests/e2e && npm install && node ui_test.mjs   # interface dans Chromium
```

Archive Raspberry Pi depuis Linux : `sudo apt install gcc-aarch64-linux-gnu && bash scripts/package.sh`
(binaire statique musl : fonctionne sur Bullseye, Bookworm et Trixie). Pousser un tag `vX.Y.Z` publie l'archive automatiquement.

## Tests

| Suite | Contenu |
|---|---|
| `src/**` (unitaires) | validation, config, exposition, détection, machine d'état, sécurité HTTP, stockage |
| `tests/api_tests.rs` | contrat de l'API utilisé par chaque page |
| `tests/security_tests.rs` | traversée de chemin, injection Wi-Fi, CSRF, DNS rebinding, fuite du mot de passe, mise à jour OTA |
| `tests/gallery_tests.rs` | galerie, miniatures, sessions, ZIP, suppression |
| `tests/simulation_tests.rs` | nuits complètes en temps virtuel : SAFE, FILTER, plage horaire, caméra en panne, disque plein, RAW |
| `tests/regression_tests.rs` | un test par bug corrigé + valeurs de référence de la détection |
| `tests/integration_tests.rs` | cycle de vie avec les mocks |
| `tests/helper_test.sh` | `aurion-helper` (seul programme exécuté en root) |
| `tests/install_test.sh` | installation, mise à jour, réparation, désinstallation |
| `tests/e2e/ui_test.mjs` | les 9 pages dans un navigateur réel |

## Architecture

Hexagonale (ports / adapters) : le cœur est testable sans matériel.

```
src/
├── core/        config, validation, exposition, détection, machine d'état, orchestrateur
├── ports/       traits caméra, stockage, horloge, réseau, système
├── adapters/    pc/ (mocks) et rpi/ (rpicam-still, clé USB, hotspot, arrêt)
├── web/         API Axum, sécurité HTTP, galerie, portail captif, pages embarquées
├── sys.rs       commandes système (helper root via sudo, espace disque)
└── cli/         simulation
scripts/         install.sh, aurion-helper, package.sh, deploy.sh/.ps1, get.sh, setup.sh
deploy/          aurion.service, règle udev de montage USB
```

Sur le Pi : `/opt/aurion/aurion` (binaire unique, interface incluse), `/opt/aurion/config/aurion.json`,
`/usr/local/sbin/aurion-helper` (seul accès root autorisé), photos sur la clé montée en `/mnt/capture`.

## Configuration

Fichier `/opt/aurion/config/aurion.json`, modifiable depuis l'interface. Principaux paramètres :

| Section | Paramètre | Défaut | Rôle |
|---|---|---|---|
| exposure | `iso_min` / `iso_max` | 100 / 3200 | plage ISO |
| | `shutter_min_us` / `shutter_max_us` | 1 s / 30 s | plage d'obturation |
| detection | `detection_capture_enabled` | false | false = SAFE, true = FILTER |
| | `roi_top_percent` | 65 | part haute de l'image analysée (50 à 99) |
| | `consecutive_required` | 2 | détections consécutives pour confirmer |
| capture | `output_format` | Jpg | `Jpg`, `RawDng` ou `RawAndJpg` |
| | `capture_interval_secs` | 10 | cadence du timelapse |
| time_range | `start` / `end` | 21:00 / 06:00 | plage horaire (heure locale) |
| | `duration_hours` | null | minuteur, prioritaire sur la plage |
| network | `ssid` / `password` / `channel` | Aurion / unique / 6 | hotspot (mot de passe 10 à 63 caractères) |

Rapport de la revue de code et de sécurité : [docs/AUDIT.md](docs/AUDIT.md).

## Licence

MIT
