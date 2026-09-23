# Audit du code

## 1. Vue d'ensemble

| Élément | Valeur |
|---|---|
| Langage | Rust 2021 (serveur Axum, runtime Tokio), HTML/CSS/JS sans framework, Bash |
| Code applicatif | 8 178 lignes Rust, 4 054 lignes HTML/CSS/JS, 1 259 lignes Bash |
| Code de test | 1 723 lignes Rust (tests d'intégration) + tests unitaires dans les modules + 2 scripts Bash + 1 test navigateur |
| Architecture | hexagonale : `core` (logique pure) / `ports` (traits) / `adapters` (`pc` = simulateurs, `rpi` = matériel) / `web` |
| Livrable | un binaire statique arm64 de 5,3 Mo, interface web incluse |
| Analyse statique | `clippy -D warnings` : 0 avertissement (PC et Raspberry Pi), `shellcheck` : 0 avertissement |
| Couverture | 82,4 % des lignes, 83,2 % des fonctions (détail § 4) |

## 2. État de départ (v1.4.13)

- **La branche principale ne compilait pas** : `update_config` supprimée par erreur par le dernier commit (`api.rs` tronqué à la ligne 262).
- `api.rs` : un fichier de 1 690 lignes mélangeant API, galerie, Wi-Fi, OTA, portail captif ; appels système bloquants dans des handlers asynchrones (`std::process::Command`, `df`, `iwlist`).
- Logique métier dépendante de l'horloge système (`chrono::Local::now()` partout) : orchestrateur impossible à tester.
- Fichiers de débogage commités (`error.txt`, icônes `-test`), icônes de 1,5 Mo.

## 3. Défauts corrigés (hors sécurité)

| Domaine | Défaut | Impact | Correction |
|---|---|---|---|
| Config | Mot de passe masqué validé avant substitution | aucune sauvegarde possible depuis *Réglages avancés* | ordre corrigé |
| Détection | ROI recadrée deux fois | 42 % du ciel analysés au lieu de 65 % | image complète passée au détecteur |
| Galerie | `GET /sessions` supprimait les dossiers sans image | logs de nuits sans aurore perdus | lecture seule |
| Galerie | panique sur un nom de session court (`&s[..8]`) | requête tuée | analyse robuste, test dédié |
| Horloge | synchronisation via l'en-tête `Date` (jamais envoyé par un navigateur) | plage horaire fausse sur un Pi sans RTC | synchronisation heure + fuseau par l'interface |
| USB | `RequiresMountsFor` + clé liée à un UUID | pas de hotspot sans clé ; autre clé jamais montée | montage udev de n'importe quelle clé |
| USB | clé déclarée montée alors qu'absente | écriture silencieuse sur la carte SD | détection réelle du point de montage |
| Presets | preset intégré = retour au mot de passe d'usine, non sauvegardé | perte de réglages | fusion sans le réseau, persistance |
| Systemd | SIGTERM ignoré, watchdog en `Type=simple` | données non synchronisées à l'arrêt | SIGTERM géré, `Type=notify` |
| Caméra | délai `rpicam-still` = pose + 15 s | poses longues coupées | 3 × pose + 20 s |
| Caméra | fichier temporaire d'une image précédente relu en cas d'échec | doublon enregistré | suppression avant chaque capture |
| Enregistrement | mode RAW+JPG : JPEG ré-encodé depuis l'image d'analyse | JPEG dégradé | JPEG d'origine conservé |
| Installation | `log2ram` absent des dépôts, `tr \| head` sous `pipefail` | installation interrompue | corrigés, test d'installeur |
| Alimentation | arrêt sur n'importe quel drapeau historique de `vcgencmd` | extinction après chaque démarrage | sous-tension actuelle, 3 fois de suite |
| Simulation | démarrage hors plage horaire, pixels bruts écrits en `.jpg` | commande `simulate` inutilisable | corrigée |
| Binaire | lié à glibc 2.39 | ne démarre pas sur Bookworm (2.36) | binaire statique musl |

## 4. Couverture par module (cargo llvm-cov)

| Module | Lignes | Fonctions | Commentaire |
|---|---|---|---|
| core/denoise.rs | 100 % | 100 % | |
| core/validate.rs | 100 % | 100 % | |
| core/exposure.rs | 99,6 % | 100 % | |
| core/state_machine.rs | 98,9 % | 100 % | |
| web/security.rs | 98,3 % | 100 % | |
| core/detection.rs | 97,3 % | 100 % | |
| web/gallery.rs | 96,8 % | 100 % | |
| core/jpeg.rs | 95,9 % | 100 % | |
| web/static_files.rs | 95,6 % | 88,9 % | |
| core/config.rs | 91,3 % | 90,6 % | |
| core/session_logger.rs | 91,5 % | 52,9 % | chemins d'erreur d'E/S |
| sys.rs | 90,2 % | 80,0 % | |
| web/mod.rs | 86,5 % | 69,2 % | `start_server` (couvert par le test navigateur) |
| core/orchestrator.rs | 84,3 % | 100 % | chemins d'échec de montage USB |
| web/api.rs | 74,5 % | 70,2 % | appels matériels (`rpicam-still`, helper) impossibles hors Pi |
| core/models.rs | 73,8 % | 82,4 % | affichages `Display` |
| main.rs, cli/* | 0 % | 0 % | exercés par le test navigateur et `simulate`, hors mesure |
| **Total** | **82,4 %** | **83,2 %** | |

Les adaptateurs `adapters/rpi/*` (compilés avec `--features rpi`) ne sont pas mesurés : ils appellent le matériel.
Leur logique a été déplacée au maximum dans `core` (testé) et dans le helper (testé en simulation).

## 5. Optimisations de performance

Mesures `aurion bench` sur PC x86 (image 12,3 Mpx) ; **à relancer sur le Pi** (`/opt/aurion/aurion bench`) :

| Traitement | Temps | Remarque |
|---|---|---|
| Lecture de la miniature EXIF (analyse) | 0,8 ms | nouvelle méthode |
| Décodage JPEG complet | 191,7 ms | ancienne méthode, à chaque image |
| Détection sur miniature | 0,4 ms | nouvelle méthode |
| Détection sur image complète | 85,1 ms | ancienne méthode |
| Encodage JPEG (q92) | 480,4 ms | seulement si correction des pixels chauds |
| Correction des pixels chauds | 79,6 ms | optionnelle |
| Empilement : ajout / résultat (4 images) | 132 / 100 ms | optionnel |

L'analyse d'une image passe d'environ 277 ms à 1,2 ms (**environ 230 fois moins de calcul**), en surveillance comme en capture.

## 6. Dette technique restante

| Point | Gravité | Proposition |
|---|---|---|
| `orchestrator::run` fait environ 400 lignes | Moyenne | découper en étapes (calibration, boucle, arrêt) |
| Images à la racine de la clé, rattachées aux sessions par l'heure | Moyenne | un dossier par nuit (avec migration) |
| Interface : styles en ligne, code dupliqué entre pages | Faible | factoriser dans `common.js` / CSS |
| `CaptureFrame` mélange pixels d'analyse et octets d'origine | Faible | deux types distincts |
| `detection.rs` et `exposure.rs` : constructeurs à 10-13 paramètres | Faible | construire depuis la config uniquement |
