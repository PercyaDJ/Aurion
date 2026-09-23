# Aurion

Caméra autonome de capture d'aurores boréales pour Raspberry Pi 4/5 et HQ Camera (IMX477).
Le Pi crée son propre Wi-Fi ; depuis le téléphone on règle, on lance la nuit, puis on récupère les photos.

## Installation (aucune connaissance technique nécessaire)

1. Téléchargez l'image (système + application, environ 600 Mo) : **[aurion-raspios-arm64.img.xz](https://github.com/PercyaDJ/Aurion/releases/latest/download/aurion-raspios-arm64.img.xz)**.
2. Copiez-la sur une carte micro-SD avec **Raspberry Pi Imager** (*Utiliser une image personnalisée*, puis **Non** aux réglages personnalisés).
3. Branchez la caméra, la carte SD, la clé USB (exFAT), puis l'alimentation.
4. Après 3 à 5 minutes, rejoignez le Wi-Fi **Aurion** (mot de passe **aurora2024**) : la page s'ouvre toute seule.

Guide illustré pas à pas, branchements et dépannage : **[docs/GUIDE_DEMARRAGE.md](docs/GUIDE_DEMARRAGE.md)**.

L'image est construite et publiée automatiquement par GitHub Actions à chaque nouvelle version.
Mise à jour ensuite depuis le téléphone, sans ordinateur : *Diagnostics*, **Mettre à jour depuis GitHub** (version stable
ou de développement, par le partage de connexion du téléphone), et retour arrière en un geste.
Boucle essai / correction : [docs/GUIDE_DEVELOPPEMENT.md](docs/GUIDE_DEVELOPPEMENT.md).

<details>
<summary>Installation sur un Raspberry Pi OS déjà installé (utilisateurs avancés)</summary>

- Paquet Debian (onglet *Releases*) : `sudo apt install ./aurion_1.9.0_arm64.deb`
- Depuis un clone du dépôt : `sudo ./install.sh` (télécharge la dernière version précompilée, ou compile en dernier recours)
- Depuis un PC : `.\scripts\deploy.ps1 pi@aurion.local .\aurion-1.9.0-rpi-arm64.tar.gz` (Windows), `scripts/deploy.sh pi@aurion.local` (Linux / macOS)

Détails : [DEPLOY_RPI.md](DEPLOY_RPI.md).
</details>

Réglages photo (RAW, timelapse, darks) : [docs/GUIDE_PHOTO.md](docs/GUIDE_PHOTO.md).

## Utilisation sur le terrain

1. Brancher le Pi (USB-C 5 V / 3 A) avec la clé USB.
2. Se connecter au Wi-Fi **Aurion** : la page s'ouvre toute seule (portail captif), sinon `http://192.168.4.1:8080`.
   L'heure du Pi est réglée automatiquement sur celle du téléphone.
3. **Accueil** : la liste *Prêt pour la nuit ?* vérifie caméra, clé (et nombre d'heures de photos possibles),
   heure et alimentation. Choisir le mode, la durée, le format, puis **Lancer la nuit**. Le Wi-Fi se coupe au bout
   de 15 s, la nuit se déroule seule et le Pi s'éteint à la fin.
4. **Le lendemain** : rallumer, l'accueil affiche la *Dernière nuit* avec **Télécharger les RAW**,
   **Voir les plus belles** et **Tout télécharger**.

Deux modes :
- **Toute la nuit** (SAFE, défaut, idéal timelapse) : capture toute la nuit, les images avec aurore sont marquées `_AURORA`.
- **Aurores seulement** (FILTER) : surveille le ciel et n'enregistre qu'après N détections consécutives.

Plusieurs nuits d'affilée (expédition) : cocher *Plusieurs nuits* ; chaque soir la nuit démarre seule, un Raspberry Pi 5
se rallume même tout seul. Guide : [docs/GUIDE_EXPEDITION.md](docs/GUIDE_EXPEDITION.md).
Sur la clé, chaque nuit a son dossier `sessions/<nuit>/` avec `RAW/`, `JPG/` et `aurores.csv`.

Coupure de courant pendant la nuit : au redémarrage, la nuit reprend seule (annulable 5 min depuis le téléphone).
Mot de passe Wi-Fi oublié : fichier vide `aurion-reset-wifi.txt` à la racine de la clé USB, puis rallumer.
Menu simple par défaut ; *Mode expert* (en bas du menu) pour les réglages fins.

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
(binaire statique musl : fonctionne sur Bullseye, Bookworm et Trixie). Changer `version` dans `Cargo.toml` et pousser sur `main` publie la release automatiquement.

## Tests

| Suite | Contenu |
|---|---|
| `src/**` (unitaires) | validation, config, exposition, détection, machine d'état, sécurité HTTP, stockage |
| `tests/api_tests.rs` | contrat de l'API utilisé par chaque page |
| `tests/security_tests.rs` | traversée de chemin, injection Wi-Fi, CSRF, DNS rebinding, fuite du mot de passe, mise à jour OTA |
| `tests/gallery_tests.rs` | galerie, miniatures, sessions, ZIP, suppression |
| `tests/simulation_tests.rs` | nuits complètes en temps virtuel : SAFE, FILTER, plage horaire, caméra en panne, disque plein, RAW, reprise après coupure |
| `tests/regression_tests.rs` | un test par bug corrigé + valeurs de référence de la détection |
| `tests/integration_tests.rs` | cycle de vie avec les mocks |
| `tests/helper_test.sh` | `aurion-helper` (seul programme exécuté en root) |
| `tests/install_test.sh` | installation, mise à jour, réparation, désinstallation |
| `tests/e2e/ui_test.mjs` | les pages dans un navigateur réel : accueil, mode expert, lancement de la nuit, téléchargements |

Détail, chiffres et couverture : [docs/TESTS.md](docs/TESTS.md).

## Architecture

Hexagonale (ports / adapters) : le cœur est testable sans matériel.

```
src/
├── core/        config, validation, exposition, détection, débruitage, JPEG/EXIF, orchestrateur
├── ports/       traits caméra, stockage, horloge, réseau, système
├── adapters/    pc/ (mocks) et rpi/ (rpicam-still, clé USB, hotspot, arrêt)
├── web/         API Axum, sécurité HTTP, galerie, portail captif, pages embarquées
├── sys.rs       commandes système (helper root via sudo, espace disque)
└── cli/         simulation, bench (coût des traitements sur le Pi)
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
| | `lock_in_run` | false | exposition figée pendant la capture (timelapse sans scintillement) |
| detection | `detection_capture_enabled` | false | false = SAFE, true = FILTER |
| | `roi_top_percent` | 65 | part haute de l'image analysée (50 à 99) |
| | `consecutive_required` | 2 | détections consécutives pour confirmer |
| capture | `output_format` | RawDng | `RawDng` (RAW seul), `RawAndJpg`, `Jpg` ou `JpgAuroraRaw` (RAW pendant les aurores) |
| | `awb` | daylight | balance des blancs fixe (pas de scintillement en timelapse) |
| | `denoise.stack_frames` | 0 | JPEG empilé toutes les N images (0 = non) |
| | `capture_interval_secs` | 0 | pause entre deux photos (0 = à la suite, la pose donne la cadence) |
| time_range | `start` / `end` | 21:00 / 06:00 | plage horaire (heure locale) |
| | `duration_hours` | null | minuteur, prioritaire sur la plage |
| network | `ssid` / `password` / `channel` | Aurion / unique / 6 | hotspot (mot de passe 10 à 63 caractères) |
| expedition | `enabled` | false | démarrage automatique chaque soir, réveil programmé du Pi 5 |

Documentation complète (DAT, DEX, spécifications, UX, audits, tests, énergie, plan d'action) : [docs/](docs/README.md). Nouveautés : [CHANGELOG.md](CHANGELOG.md).

## Licence

MIT
