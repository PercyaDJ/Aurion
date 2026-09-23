# Revue complète Aurion (v1.4.13 vers v1.5.0)

## État de départ

Le dépôt **ne compilait pas** : le dernier commit avait supprimé par erreur la fonction `update_config`
(`src/web/api.rs` tronqué). Aucune version installable ne pouvait donc être produite à partir de `main`.

## Failles de sécurité corrigées

| # | Gravité | Problème | Correction |
|---|---|---|---|
| 1 | Critique | **sudoers = root complet** : `NOPASSWD` sur `cp`, `mount`, `dnsmasq`, `hostapd`, `killall`, `date`… (`sudo cp` suffit à réécrire `/etc/sudoers`) | un seul script root, `aurion-helper`, qui revalide chaque argument ; sudoers limité à ce script |
| 2 | Critique | **Mise à jour OTA** : n'importe quelle page web pouvait envoyer un binaire (CSRF), installé via `sudo cp` | protection CSRF, contrôle ELF + architecture, test `--version` avant installation, remplacement atomique sans root, ancienne version gardée |
| 3 | Élevée | **Traversée de chemin sur les presets** : nom `../../x` = écriture/lecture de fichiers `.json` arbitraires (dont la config) | noms validés (`[A-Za-z0-9 _-]`, 40 car.) |
| 4 | Élevée | **Injection dans hostapd / wpa_supplicant** via SSID ou mot de passe (retours ligne, guillemets) | validation SSID (1 à 32 octets, pas de caractère de contrôle), mot de passe (ASCII imprimable, 8 à 63), canal (1 à 13), côté Rust **et** côté helper ; SSID/PSK passés en hexadécimal à wpa_supplicant |
| 5 | Élevée | **Mot de passe Wi-Fi en clair** via `GET /api/config/wifi-password` et dans les fichiers de presets | route supprimée, mot de passe jamais renvoyé ni stocké dans les presets |
| 6 | Élevée | **Mot de passe par défaut public** (`aurora2024` dans le README) sur tous les appareils | mot de passe unique généré à l'installation ; avertissement tant que la valeur d'usine est utilisée |
| 7 | Moyenne | **CSRF / DNS rebinding** : toute page web ouverte sur le téléphone pouvait lancer un arrêt, une suppression, une mise à jour | contrôle `Origin` / `Sec-Fetch-Site` sur les écritures, liste blanche des `Host` (IP, `localhost`, `*.local`, nom d'hôte) |
| 8 | Moyenne | Mot de passe Wi-Fi en argument de commande et dans des fichiers `/tmp` lisibles | l'application transmet les secrets au helper par l'entrée standard ; fichiers de config réseau en 0600 dans `/run/aurion`. Reste : `nmcli` reçoit le mot de passe en argument pendant une fraction de seconde (processus root, sans autre utilisateur sur le Pi) |
| 9 | Moyenne | Noms de fichiers insérés tels quels dans le HTML (XSS possible via un nom sur la clé) | seuls les noms sûrs sont listés ; échappement dans l'interface ; plus d'`onclick` construit avec des chaînes |
| 10 | Faible | Pas d'en-têtes de sécurité, pas de limite de taille de requête | CSP, `nosniff`, `X-Frame-Options`, limite 1 Mo (128 Mo pour la mise à jour) |
| 11 | Faible | Config écrite sans précaution (lisible par tous, corrompue en cas de coupure) | écriture atomique (fichier temporaire, fsync, renommage), droits 0600 |

## Bugs fonctionnels corrigés

| Problème | Effet sur le terrain | Correction |
|---|---|---|
| Mot de passe masqué validé **avant** d'être remplacé | toute sauvegarde depuis *Réglages avancés* échouait | ordre corrigé (test de non-régression) |
| La galerie **supprimait** les dossiers de session sans image | logs perdus pour une nuit sans aurore, et la session en cours pouvait être effacée | lecture seule |
| Double recadrage de la zone d'analyse | seuls 42 % du ciel analysés au lieu de 65 % | le détecteur reçoit l'image complète. **La sensibilité change** : à revérifier sur le terrain |
| Heure jamais synchronisée (le navigateur n'envoie pas d'en-tête `Date`) | Pi sans horloge : plage 21h-6h fausse, nuit manquée | synchronisation automatique heure + fuseau depuis le téléphone à l'ouverture de l'interface |
| Service lié au montage USB (`RequiresMountsFor`) | sans clé au démarrage : pas de hotspot ni d'interface | dépendance retirée, avertissement dans l'interface |
| Clé USB liée à **un** UUID dans fstab | une autre clé n'était jamais montée | montage automatique de n'importe quelle clé (udev + systemd-mount) |
| « Clé montée » alors qu'elle était absente (fichier test écrit sur la carte SD) | fausse sécurité | vraie détection de point de montage |
| Surveillance d'alimentation sur **n'importe quel** drapeau historique | arrêt du Pi peu après chaque démarrage après une seule chute de tension | réaction à la sous-tension actuelle, 3 fois de suite |
| `log2ram` absent des dépôts Raspberry Pi OS | le bootstrap s'arrêtait en erreur | remplacé par journald en RAM |
| Générateur de mot de passe `tr \| head` sous `pipefail` | installation interrompue (trouvé par le test d'installeur) | corrigé |
| Presets intégrés remettaient le mot de passe d'usine et n'étaient pas sauvegardés | perte du mot de passe, retour aux anciens réglages au redémarrage | un preset ne touche plus réseau/stockage ; persistance |
| Bouton *Déconnexion* actif pendant la nuit | phase affichée bloquée sur DISCONNECT | refusé hors phase ARM |
| Sauvegardes sans vérification de la réponse | « Sauvegardé » affiché même en cas de refus | message d'erreur réel |
| Arrêt `systemctl stop` (SIGTERM) non géré | pas de `sync` | SIGTERM géré, `sync` avant sortie |
| Preview pendant la capture | conflit sur la caméra | refusée pendant la nuit, verrou caméra |
| Preview à exposition quasi fixe (limiteur à 15 %) | preview trop sombre ou claire | mesure directe, jusqu'à 4 prises |
| Galerie : miniature 64×48 affichée en plein écran, lecture complète des fichiers en mémoire | image floue, RAM | miniature 320 px puis image complète ; envoi en flux ; décodage limité à 2 en parallèle |
| `simulate` démarrait hors plage horaire et écrivait des pixels bruts en `.jpg` | simulation vide | corrigé |
| Binaire compilé sur PC récent : glibc 2.39 exigée | ne démarre pas sur Bookworm (2.36) | binaire statique musl |
| Polices Google | requêtes bloquées hors ligne, pages plus lentes | supprimées |
| Icônes de 1,5 Mo en 1024 px | chargement lent sur le Wi-Fi du Pi | redimensionnées (400 Ko au total) |

## Déploiement simplifié

- **Un seul binaire** : les pages web sont incluses dedans (`rust-embed`). Mettre à jour le binaire met à jour l'interface.
- **Plus de compilation sur le Pi** : archive précompilée (statique, arm64) construite par GitHub Actions à chaque tag.
- **Une commande** : `sudo ./install.sh` (installe, met à jour, répare une config invalide, désinstalle).
- Envoi depuis Windows (`deploy.ps1`), Linux/macOS (`deploy.sh`) ou directement depuis le Pi (`get.sh`).
- Mise à jour depuis le téléphone, avec retour arrière possible.

## Tests ajoutés

| Suite | Nombre |
|---|---|
| Tests unitaires Rust | 83 |
| API (contrat utilisé par les pages) | 17 |
| Sécurité | 19 |
| Galerie | 9 |
| Simulation de nuits complètes (temps virtuel) | 11 |
| Non-régression (un par bug + valeurs de référence de détection) | 10 |
| Intégration (existants) | 7 |
| Helper root (dry-run) | 39 cas |
| Installeur (fausse racine) | 33 contrôles |
| Interface dans Chromium | 21 |

Le binaire arm64 final a aussi été exécuté sous émulation qemu (pages, API, protection CSRF).

## Ce qui n'a pas pu être vérifié sans le matériel

Ces points reposent sur la documentation des outils et doivent être validés lors du premier essai sur le Pi :

1. **Hotspot via NetworkManager** (`nmcli ... mode ap, ipv4.method shared`) et DNS captif via
   `/etc/NetworkManager/dnsmasq-shared.d/`. L'ancienne méthode hostapd reste utilisée si NetworkManager ne gère pas `wlan0`.
2. **Montage USB par udev + systemd-mount**.
3. **Scan Wi-Fi pendant que le hotspot est actif** : la puce du Pi peut ne renvoyer qu'une liste partielle.
4. **Délai de capture** `rpicam-still` : fixé à 3 × durée d'exposition + 20 s (l'ancien, durée + 15 s, risquait de couper les poses longues).
5. **Seuils de détection** : la zone analysée est maintenant réellement de 65 %, la sensibilité peut différer de l'ancienne version.

Premier essai conseillé : `sudo journalctl -u aurion -f` pendant le démarrage, connexion au hotspot, preview, puis une courte nuit en mode minuteur (0,5 h).

## Pistes suivantes

- Code PIN optionnel pour l'interface (utile si le Pi rejoint souvent un réseau partagé).
- Rangement des images par dossier de session (aujourd'hui à la racine de la clé, conservé pour compatibilité).
- Horloge RTC (Pi 5 : pile) pour ne plus dépendre du téléphone.
