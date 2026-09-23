# Stratégie et résultats des tests

Résultat au 23/09/2026 : **tous les tests passent.**

| Type de test | Où | Nombre | Ce qui est vérifié |
|---|---|---|---|
| Unitaires | `src/**` (`#[cfg(test)]`) | 114 | validation des entrées, config, exposition, détection, débruitage, JPEG/EXIF, machine d'état, sécurité HTTP, stockage, parseurs, reprise de nuit, estimation de l'autonomie de la clé, réinitialisation du mot de passe par la clé, rangement par nuit et `aurores.csv`, horloge matérielle (RTC) |
| Intégration | `tests/integration_tests.rs` | 7 | cycle de vie complet avec les simulateurs |
| Contrat d'API | `tests/api_tests.rs` | 23 | chaque route utilisée par les pages répond avec les champs attendus ; règles métier (darks, presets, heure) ; vérifications avant la nuit (prêt, clé absente, mode expédition) |
| Sécurité | `tests/security_tests.rs` | 19 | traversée de chemin, injection Wi-Fi, CSRF, DNS rebinding, fuite du mot de passe, taille des requêtes, mise à jour OTA |
| Galerie | `tests/gallery_tests.rs` | 13 | dossiers par nuit et photos des anciennes versions, liste, miniatures, sessions, ZIP (contenu vérifié, RAW seuls, JPEG seuls, filtre inconnu refusé), dernière nuit, suppression |
| Simulation | `tests/simulation_tests.rs` | 30 | nuits entières en temps virtuel : SAFE, FILTER, plage horaire, caméra en panne ou absente, disque plein, RAW, rechargement à chaud, hotspot, captures façon Pi (JPEG 1280×960 + EXIF), pixels chauds, empilement, verrou d'exposition, classement des aurores, **reprise après coupure** (reprise seule pour le temps restant, annulation depuis le téléphone, nuit déjà finie ignorée, marqueur présent seulement pendant la nuit, même dossier et numérotation continue), **expédition** (démarrage automatique après 5 min sans activité, jamais sans heure fiable, réveil programmé du Pi 5, veille jusqu'à la nuit puis reprise), dossier par nuit et `aurores.csv`, RAW seulement pendant les aurores, **RAW seul à la suite** (cadence donnée par la vraie durée de pose), **surveillance sans aucun RAW**, profils d'énergie selon la phase |
| Non-régression | `tests/regression_tests.rs` | 11 | un test par bug corrigé + valeurs de référence de la détection (écart toléré 5 %) |
| Mise à jour en ligne | `tests/update_tests.rs` | 3 | téléchargement depuis un faux GitHub (curl), fichier non conforme refusé, requêtes invalides, retour arrière |
| Helper root | `tests/helper_test.sh` | 57 cas | arguments valides et malveillants, commandes générées, crochets ignorés sous sudo |
| Image carte SD | `tests/image_test.sh` | 11 contrôles | fabrication de l'image sur une copie factice de Raspberry Pi OS : partition agrandie, système de fichiers sain, Aurion et l'installation au premier démarrage présents, compte de maintenance, SSH désactivé |
| Installeur | `tests/install_test.sh` | 39 contrôles | installation dans une fausse racine, mise à jour, mode paquet, réparation d'une config invalide, désinstallation |
| Interface (bout en bout) | `tests/e2e/ui_test.mjs` | 29 étapes | les 8 pages dans Chromium, format téléphone : aucune erreur JavaScript, accueil (vérifications, dernière nuit, lien RAW), menu simple puis expert, redirection de l'ancien tableau de bord, mot de passe au premier démarrage, réglages enregistrés sur disque, presets, galerie, téléchargements RAW / JPG par nuit, meilleures aurores, darks, mode expédition et format RAW des aurores, lancement de la nuit depuis l'accueil |
| Fumée ARM | manuel (qemu) | - | le binaire arm64 final démarre, sert les pages et l'API, bloque le CSRF |
| Analyse statique | clippy, shellcheck | - | 0 avertissement en mode `-D warnings` |
| Dépendances | cargo audit | 233 crates | aucune vulnérabilité ; voir AUDIT_SECURITE.md |
| Couverture | cargo llvm-cov | - | 82,4 % des lignes (mesure de la 1.5.0) |

Total Rust : **220 tests**, dont 30 simulations de nuit qui s'exécutent en environ 25 s grâce au temps virtuel
(`tokio::time::pause` : chaque attente de l'orchestrateur avance l'horloge instantanément).

## Lancer les tests

```bash
cargo test                                          # 220 tests Rust
cargo clippy --all-targets -- -D warnings           # analyse statique (ajouter --features rpi)
bash tests/helper_test.sh                           # helper root
sudo -E bash tests/install_test.sh target/debug/aurion   # installeur (fausse racine, root requis)
bash scripts/package.sh && sudo bash tests/image_test.sh  # image carte SD (root, périphériques loop)
cd tests/e2e && npm install && node ui_test.mjs     # navigateur (après cargo build)
cargo llvm-cov --summary-only                       # couverture (cargo install cargo-llvm-cov)
cargo audit                                         # dépendances (cargo install cargo-audit)
```

La CI GitHub Actions (`.github/workflows/ci.yml`) exécute tout cela à chaque push, puis construit l'archive et le
paquet Raspberry Pi.

## Principes

- **Aucun test ne touche la machine** : dossiers temporaires, simulateurs de caméra / stockage / réseau / système,
  helper en simulation, installeur dans une fausse racine.
- **Temps virtuel** pour l'orchestrateur : horloge injectée (`TokioClock`), nuits de 8 h testées en quelques secondes.
- **Injection de pannes** : caméra qui échoue N fois, caméra absente, clé qui se remplit, config invalide.
- **Valeurs de référence** : les scores de détection sur ciel synthétique sont figés ; tout changement d'algorithme se voit.
- **Un test par bug** : chaque défaut corrigé a son test de non-régression.

## Ce que les tests ne couvrent pas (matériel requis)

| Point | Comment le valider |
|---|---|
| `rpicam-still` réel (options `--thumb`, `--denoise`, `--awb`, `--raw`) | preview puis courte nuit, vérifier les DNG et la miniature de galerie |
| Hotspot NetworkManager et portail captif | se connecter au Wi-Fi « Aurion » depuis iOS et Android |
| Montage USB udev + réparation `--fsck` | brancher une clé, débrancher l'alimentation pendant une capture, rebrancher |
| Économie d'énergie (`config.txt`) | mesurer le courant avec un testeur USB avant / après |
| Temps de traitement sur Pi | `/opt/aurion/aurion bench` |

Protocole conseillé pour le premier essai : voir PLAN_ACTION.md, tâche V1.
