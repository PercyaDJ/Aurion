# Audit de sécurité

## 1. Périmètre et modèle de menace

Aurion est une caméra de terrain : un Raspberry Pi crée son propre Wi-Fi (WPA2), un téléphone s'y connecte et pilote tout
depuis une page web **sans identifiant**. Qui connaît le mot de passe Wi-Fi est donc l'opérateur.

| Acteur | Accès | Ce qu'on doit empêcher |
|---|---|---|
| Passant sur le terrain | ondes Wi-Fi | se connecter (mot de passe faible ou connu) |
| Page web malveillante ouverte sur le téléphone | le navigateur du téléphone | piloter la caméra à distance (CSRF, DNS rebinding) |
| Appareil du réseau de la maison (mode maintenance) | réseau local | lire la config, installer un binaire, éteindre |
| Fichier piégé sur la clé USB | noms de fichiers | injection dans l'interface (XSS) |
| Service Aurion compromis | compte utilisateur du Pi | devenir root |

Surfaces analysées : API HTTP (42 routes, dont 9 de portail captif), pages web, helper root, sudoers, installeur, service systemd, dépendances Rust.

## 2. Failles trouvées et corrigées

| # | Gravité | Faille (avant) | Correction | Test |
|---|---|---|---|---|
| S1 | Critique | sudoers `NOPASSWD` sur `cp`, `mount`, `dnsmasq`, `hostapd`, `killall`, `date`, `ip` : `sudo cp` suffit pour réécrire `/etc/sudoers`, donc root complet | un seul programme root, `aurion-helper`, qui revalide chaque argument ; sudoers limité à ce fichier | `install_test.sh`, `helper_test.sh` |
| S2 | Critique | `POST /api/system/update` installait n'importe quel exécutable via `sudo cp`, déclenchable depuis n'importe quelle page web | anti-CSRF ; contrôle ELF (classe, boutisme, type, architecture) ; exécution d'essai `--version` ; remplacement atomique sans root ; ancienne version conservée ; refus pendant la nuit | `security_tests.rs` (5 tests OTA) |
| S3 | Élevée | Presets : nom `../../x` = écriture/lecture de `.json` arbitraires (dont la config) | noms `[A-Za-z0-9 _-]`, 40 caractères | `preset_names_cannot_escape_the_presets_dir` |
| S4 | Élevée | Injection de lignes dans hostapd / wpa_supplicant via SSID ou mot de passe | validation SSID (1 à 32 octets, sans caractère de contrôle), mot de passe (ASCII imprimable, 8 à 63), canal (1 à 13), en Rust **et** dans le helper ; SSID/PSK en hexadécimal pour wpa_supplicant | `wifi_config_injection_rejected`, `helper_test.sh` |
| S5 | Élevée | Mot de passe Wi-Fi en clair via `GET /api/config/wifi-password` et dans les presets | route supprimée ; mot de passe jamais renvoyé ni stocké dans un preset | `wifi_password_is_never_exposed` |
| S6 | Élevée | Mot de passe d'usine public (`aurora2024` dans le README) identique sur tous les appareils | mot de passe unique de 14 caractères généré à l'installation ; alerte tant que la valeur d'usine est utilisée | `install_test.sh` |
| S7 | Moyenne | Aucune protection CSRF ni DNS rebinding | écritures refusées si `Origin` ≠ `Host` ou `Sec-Fetch-Site: cross-site` ; `Host` limité à IP, `localhost`, `*.local`, nom d'hôte | `csrf_*`, `dns_rebinding_hosts_are_blocked` |
| S8 | Moyenne | Mot de passe Wi-Fi en argument de commande et dans `/tmp` lisible par tous | transmis au helper par l'entrée standard ; fichiers réseau en 0600 dans `/run/aurion` | revue |
| S9 | Moyenne | Noms de fichiers de la clé insérés tels quels dans le HTML et dans des `onclick` | seuls les noms `[A-Za-z0-9._-]` sont listés ; échappement ; plus de chaînes dans les `onclick` | `listing_ignores_unsafe_and_non_images` |
| S10 | Moyenne | Traversée de chemin partielle : suppression acceptant `sous-dossier/x` et fichiers non-images | noms sûrs + extension image obligatoires pour lire, supprimer, zipper | `security_tests.rs` (6 tests) |
| S11 | Faible | Pas d'en-têtes de sécurité, pas de limite de taille | CSP, `nosniff`, `X-Frame-Options: DENY`, `Referrer-Policy` ; corps limité à 1 Mo (128 Mo pour la mise à jour) | `security_headers_present`, `oversized_body_rejected` |
| S12 | Faible | Config écrite en place, lisible par tous | écriture atomique (fsync + renommage), droits 0600 | `test_save_is_atomic_and_private` |
| S13 | Faible | Réglage de l'heure : chaîne libre passée à `date -s` | époque Unix bornée (2024 à 2100), fuseau vérifié dans `/usr/share/zoneinfo` | `time_sync_rules`, `helper_test.sh` |

## 3. Revue de la 1.6.0

| Point | Analyse | Test |
|---|---|---|
| `GET /api/preflight` | lecture seule ; n'expose ni le mot de passe ni de chemin ; la détection caméra lance `rpicam-hello` au plus toutes les 30 s (pas de déni de service par rafraîchissement) | `preflight_*` |
| `GET /api/night/last` | lecture seule ; nom de session issu d'un dossier filtré par `is_safe_name` | `last_night_summary` |
| `?only=raw\|jpg` sur le ZIP d'une nuit | valeur fermée, toute autre valeur refusée (400) ; nom de session toujours validé | `session_zip_raw_only_or_jpg_only` |
| `POST /api/night/resume/cancel` | écriture, donc soumise à l'anti-CSRF et au contrôle `Host` ; effet limité (annuler une reprise) | `interrupted_night_can_be_cancelled_from_the_phone` |
| `night.json` | écrit en 0600 dans le dossier de config du service, écriture atomique ; un fichier corrompu est ignoré | `marker_roundtrip_and_garbage` |
| Menu généré en JavaScript | libellés statiques insérés par `textContent` (pas de HTML) ; données serveur toujours échappées (`aurionEscape`) | e2e |
| Changement du mot de passe depuis l'accueil | même route et mêmes validations que les réglages avancés (10 à 63 caractères ASCII) ; hotspot redémarré par le helper | e2e « mot de passe au premier démarrage » |

### Revue de la 1.7.0

| Point | Analyse | Test |
|---|---|---|
| `aurion-helper rtc-wake <époque>` | 10 chiffres exactement, entre maintenant + 1 min et + 8 jours, fichier `wakealarm` requis ; en simulation, jamais le vrai `/sys` | `helper_test.sh` (6 cas) |
| `hwclock --systohc` après `set-time` | seulement si `/dev/rtc0` et `hwclock` existent ; aucun argument utilisateur | revue |
| `GET /api/gallery?session=` | nom de nuit validé par `is_safe_name` (400 sinon) ; images retrouvées par nom validé, uniquement dans `sessions/<nuit>/JPG|RAW` ou à la racine | `night_folders_are_listed_served_and_zipped` |
| Démarrage automatique (expédition) | jamais sans heure fiable ni clé présente ; toute requête `/api/` le repousse de 5 min ; les sondes du portail captif ne comptent pas | simulations `expedition_*` |
| Heure lue sur l'horloge matérielle | valeur ignorée si antérieure à 2024 (horloge non réglée) ; appliquée par le helper existant | `rtc_detection` |

### Revue de la 1.8.0

| Point | Analyse | Test |
|---|---|---|
| Mise à jour depuis GitHub | adresse d'API fixe (dépôt du projet), téléchargement accepté seulement s'il commence par `https://github.com/PercyaDJ/Aurion/releases/download/`, taille plafonnée, puis mêmes contrôles que l'envoi manuel (ELF arm64, exécution d'essai, remplacement atomique, ancienne version conservée) ; aucune action root ; refusée pendant une nuit | `update_tests.rs`, `asset_selection_and_origin_check` |
| Le helper root n'est jamais mis à jour par le téléphone | un fichier téléchargé par le compte du service ne doit pas devenir un programme root ; la version attendue est vérifiée et signalée | revue, `helper_test.sh` (`version`) |
| Mot de passe du partage de connexion | validé comme les autres (SSID, WPA 8 à 63), stocké dans la config (0600), jamais renvoyé (masqué), transmis au helper par l'entrée standard | `online_update_rejects_bad_requests` |
| `aurion-helper power-profile` | 3 valeurs fixes ; en simulation, jamais le vrai `/sys` ; Ethernet laissé actif si un câble est branché | `helper_test.sh` (8 cas) |
| Retour arrière | échange de deux fichiers du compte du service, refusé pendant une nuit | `rollback_swaps_current_and_previous` |

Préparer la clé (`aurion-helper usb-format`, 1.10.0) : commande destructrice, donc la plus encadrée. Nom `sdX` exact
(pas de partition, pas d'autre type de disque), chemin sysfs passant par un contrôleur USB (jamais `mmcblk`, NVMe,
SATA), refus si le disque porte la racine du système, refus si plusieurs clés sont branchées. Côté API : confirmation
`EFFACER`, phase ARM uniquement, anti-CSRF. Tests : `helper_test.sh` (7 cas), `prepare_usb_key_is_guarded`.

Copie des réglages sur la clé (`aurion-reglages.json`, 1.9.0) : elle contient le mot de passe du Wi-Fi Aurion, pas
celui du partage de connexion du téléphone. Qui possède la clé a déjà l'accès physique (réinitialisation du mot de
passe par fichier, photos) : risque jugé faible. La copie n'est reprise que sur une carte SD sans réglages
enregistrés, et seulement si elle est valide.

Risque accepté : les versions ne sont pas signées. Le téléchargement passe en HTTPS depuis le dépôt du projet ; qui
peut modifier le dépôt peut donc livrer une version (plan A2 : signature Ed25519).

## 4. Le helper root (`scripts/aurion-helper`)

Seul programme exécutable en root par le service. Principes :
- une liste fermée de commandes (`ap-start`, `ap-stop`, `wifi-scan`, `wifi-connect`, `set-time`, `set-timezone`, `shutdown`, `mount-usb`, `umount-usb`, `usb-add`) ;
- chaque argument validé par expression régulière avant usage ;
- secrets lus sur l'entrée standard ;
- `PATH` et `LC_ALL` fixés ; `umask 077` ;
- les crochets de test (`AURION_HELPER_DRYRUN`, `AURION_ENV_FILE`) sont ignorés dès qu'il est appelé via sudo.

42 cas testés en mode simulation (dont les injections). Incident pendant la revue : un test lancé en root a réellement réglé
l'horloge du conteneur de développement ; le test a été remplacé par une commande sans effet réel et le mode simulation
n'écrit plus rien dans `/run`.

## 5. Dépendances (cargo audit, base RustSec du 23/09/2026, 1267 avis)

| Crate | Version | Avis | Statut |
|---|---|---|---|
| anyhow | 1.0.101 → 1.0.104 | RUSTSEC-2026-0190 (unsound `downcast_mut`) | **corrigé** par mise à jour |
| spin | 0.9.8 → 0.9.9 | version retirée (yanked) | **corrigé** par mise à jour |
| rand | 0.8.5 | RUSTSEC-2026-0097 (unsound avec un logger personnalisé) | accepté : utilisé seulement par l'outil de test `axum-test`, absent du binaire embarqué |

Aucune vulnérabilité (« vulnerability ») connue ; aucun avis ne concerne le binaire livré sur le Pi.

### Suivi continu avec ARBOR (workflow `.github/workflows/arbor.yml`)

Le contrôle ci-dessus est une photo à une date. Le suivi continu passe par ARBOR (projet
`15991eb6-98e7-4685-8ace-8ed0c0b791f3`), à chaque push sur `main` et chaque lundi (les failles publiées sans
nouveau commit sont vues dans la semaine) :

| Étape | Outil | Ce qui est analysé |
|---|---|---|
| SBOM CycloneDX | Syft 1.20.0 | `Cargo.lock` (233 crates), actions GitHub utilisées, dépendances du test navigateur ; confronté à OSV par ARBOR |
| Code | Semgrep 1.177.0 | injections, crypto faible, secrets codés en dur |
| Configuration | Trivy 0.70.0 | secrets commis, fichiers de configuration |

- Le job échoue (code 2) dès qu'un résultat de niveau **élevé** ou **critique** reste ouvert ; une décision prise
  dans ARBOR (faux positif, risque accepté) le débloque sans commit. Il ne bloque ni les tests ni la publication.
- Chaîne d'approvisionnement : scripts d'installation pris sur le tag de la version (jamais `main`), versions
  figées ; l'agent `arbor-scan.sh` est téléchargé dans un fichier, jamais passé directement au shell, et son
  empreinte SHA-256 est affichée. La variable de dépôt `ARBOR_SCAN_SHA256` fige une version relue : tout
  changement du script arrête alors le job.
- La clé `ARBOR_API_KEY` est un secret du dépôt, jamais écrit dans le code ni dans les journaux. Sans elle, le SBOM
  est produit (artefact `aurion-sbom`) et rien n'est envoyé.
- Chaque release porte aussi son inventaire `aurion-sbom.cdx.json`.
- Limite : les paquets Debian de l'image carte SD ne sont pas dans ce SBOM. Comme chaque envoi remplace
  l'inventaire du projet ARBOR, les suivre demande un second projet ARBOR dédié à l'image.

## 6. Risques résiduels

| Risque | Niveau | Pourquoi il reste | Piste |
|---|---|---|---|
| Image carte SD : mot de passe Wi-Fi d'usine commun (`aurora2024`) jusqu'à ce que l'utilisateur le change | Moyen | choix d'accessibilité pour un public non technique (le mot de passe est imprimé dans le guide). Depuis la 1.6.0, l'accueil propose de le changer dès la première connexion (appliqué aussitôt) et la vérification avant la nuit le signale en orange. L'installation par script ou paquet génère toujours un mot de passe unique | rendre le changement obligatoire avant la première nuit, si le terrain montre que l'encadré est ignoré |
| Réinitialisation du mot de passe par un fichier sur la clé USB | Faible | qui peut brancher une clé et rallumer le Pi a de toute façon l'accès physique (carte SD, photos) ; le fichier n'agit qu'une fois et la remise à zéro est journalisée | aucune |
| Pas d'authentification applicative | Moyen | choix d'ergonomie : le Wi-Fi WPA2 fait office de clé | PIN optionnel (plan d'action A3) |
| `nmcli` reçoit le mot de passe du hotspot en argument pendant une fraction de seconde | Faible | limitation de `nmcli` ; processus root, aucun autre utilisateur sur le Pi | fichier de connexion NetworkManager en 0600 |
| Hotspot en WPA2-PSK (pas WPA3) | Faible | compatibilité avec tous les téléphones | option WPA3-SAE |
| Interface en HTTP (pas HTTPS) | Faible | réseau fermé du Pi ; un certificat auto-signé effraie les navigateurs | aucune dans l'immédiat |
| Mise à jour OTA non signée | Moyen | contrôles de format et d'exécution mais pas de signature | signature Ed25519 des releases (plan A2) |
| Service sans durcissement systemd poussé (`ProtectSystem`, `NoNewPrivileges`) | Faible | incompatible avec l'appel à sudo | déléguer les actions root à un service séparé (plan A4) |
