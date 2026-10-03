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

Risque accepté à l'époque : les versions n'étaient pas signées. Corrigé en 1.12.0 (signature Ed25519, clé principale
et clé de secours) ; en 1.13.0, le paquet de mise à jour est revérifié par le helper root (section 4).

## 4. Le helper root (`scripts/aurion-helper`)

Seul programme exécutable en root par le service. Principes :
- une liste fermée de commandes (`ap-start`, `ap-stop`, `wifi-scan`, `wifi-connect`, `set-time`, `set-timezone`, `shutdown`, `rtc-wake`, `power-profile`, `mount-usb`, `umount-usb`, `usb-add`, `usb-format`, `app-update`, `app-rollback`, `version`) ;
- chaque argument validé par expression régulière avant usage ;
- secrets lus sur l'entrée standard ;
- `PATH` et `LC_ALL` fixés ; `umask 077` ;
- les crochets de test (`AURION_HELPER_DRYRUN`, `AURION_ENV_FILE`) sont ignorés dès qu'il est appelé via sudo.

Depuis la 1.13.0, `app-update <paquet> <signature>` installe un paquet de mise à jour, fichiers système compris. C'est
la seule commande qui écrit du code exécuté en root : elle copie d'abord paquet et signature dans son propre dossier
(`/var/lib/aurion/update`, 0700) pour qu'ils ne puissent plus changer, puis vérifie la signature avec `openssl` et les
clés de `/usr/local/share/aurion/keys` (root, 0644). Le service ne peut ni fournir ni modifier ces clés ; une
interface compromise ne peut donc installer qu'un paquet publié par le projet. Taille limitée à 64 Mo, liens
symboliques refusés, contenu attendu contrôlé, sauvegarde des fichiers remplacés avant installation.
`app-rollback` remet cette sauvegarde (lancé par le helper sauvegardé lorsqu'il existe).

107 cas testés en mode simulation (dont les injections et 25 pour les paquets de mise à jour : paquet altéré, autre
clé, signature tronquée, lien symbolique, paquet incomplet, retour arrière complet). Incident pendant la revue : un test lancé en root a réellement réglé
l'horloge du conteneur de développement ; le test a été remplacé par une commande sans effet réel et le mode simulation
n'écrit plus rien dans `/run`.

## 5. Dépendances (cargo audit, base RustSec du 23/09/2026, 1267 avis)

| Crate | Version | Avis | Statut |
|---|---|---|---|
| anyhow | 1.0.101 → 1.0.104 | RUSTSEC-2026-0190 (unsound `downcast_mut`) | **corrigé** par mise à jour |
| spin | 0.9.8 → 0.9.9 | version retirée (yanked) | **corrigé** par mise à jour |
| rand | 0.8.5 | RUSTSEC-2026-0097 (unsound avec un logger personnalisé) | accepté : utilisé seulement par l'outil de test `axum-test`, absent du binaire embarqué |

Aucune vulnérabilité (« vulnerability ») connue ; aucun avis ne concerne le binaire livré sur le Pi.

### Suivi continu avec Lysbor (workflows `release.yml` et `lysbor.yml`, script `scripts/lysbor-scan.sh`)

Le contrôle ci-dessus est une photo à une date. Le suivi continu passe par Lysbor, sur deux projets, car chaque envoi
remplace l'inventaire du projet :

| Projet Lysbor | Quand | Contenu |
|---|---|---|
| Aurion_Application (`46e8a16d-58a7-4300-a6ae-e0cbac7ee6c1`, secret `LYSBOR_API_KEY`) | chaque release ; à la demande (workflow Lysbor) | SBOM Syft du dépôt (233 crates, actions GitHub, dépendances du test navigateur) ; Semgrep (code) ; Trivy (secrets, configuration) |
| Aurion_Image (`7d095ce7-6d86-4c48-92ef-b9df2fbd3327`, secret `LYSBOR_IMAGE_API_KEY`) | dans le job de la release, juste après la publication ; à la demande, SBOM publié avec la dernière release (sans retélécharger l'image) | SBOM des paquets du système de l'image publiée : 633 paquets Debian (noyau, OpenSSL, dnsmasq, hostapd…), 220 Python, 98 Go |

Pourquoi l'image compte autant que l'application : sur le terrain, le Wi-Fi du Pi est joignable par toute personne à
portée. Le point d'accès, le serveur DHCP et DNS et le noyau font partie de la surface d'attaque au même titre que
l'interface web. Et c'est l'image entière qui est distribuée.

- Envoi direct par l'API documentée (`scripts/lysbor-upload.sh`) : aucun script distant n'est exécuté dans la CI.
  La clé passe par l'entrée standard de curl, jamais par la ligne de commande ; en cas de refus, le code HTTP et
  l'en-tête `Server` sont affichés (jamais la clé) pour distinguer Lysbor d'un pare-feu placé devant.
- SBOM de l'image (`scripts/image-sbom.sh`) : partition système montée en lecture seule, empreinte de l'image
  vérifiée avant, liste des fichiers exclue (sinon 18 Mo pour 2 Mo utiles, limite Lysbor 25 Mo).
- Outils en versions figées (Syft 1.20.0, Trivy 0.70.0, Semgrep 1.177.0), scripts d'installation pris sur le tag.
- Une clé API par projet, en secrets du dépôt : `LYSBOR_API_KEY` (application) et `LYSBOR_IMAGE_API_KEY` (image).
  Une clé divulguée n'expose qu'un projet. Sans clé, les SBOM sont produits en artefacts et rien n'est envoyé.
- Chaque release porte `aurion-sbom.cdx.json` et `aurion-image-sbom.cdx.json`.
- Réduction de la surface d'attaque (1.10.1) : mises à jour de sécurité Debian à la fabrication, puis retrait de
  92 paquets inutiles (rpi-connect, cloud-init, compilateurs, gdb, Bluetooth, SMB, archiveurs et leurs dépendances
  Python et Go). Premier relevé Lysbor sur l'image 1.10.0 : 9 mises à jour possibles, 37 composants sans correctif ;
  une partie disparaît avec ces paquets. Les autres (binutils, rsync, curl…) sont des outils locaux, non joignables
  depuis le Wi-Fi : à trier dans Lysbor en « risque accepté » avec cette justification.
- 1.10.2 : 47 paquets de plus retirés (en-têtes et compilateur, micrologiciels Wi-Fi inutiles, PPP, rpi-update,
  pastebinit, wget, rich et pygments), documentation et traductions retirées. Plan Lysbor du 24/09/2026 :
  pygments 2.18.0 (CVE-2026-4539) traité par retrait du paquet, la mise à jour par pip ne s'appliquant pas à un
  paquet Debian. Composants restants sans correctif : outils locaux (binutils, rsync, curl en client, perl…),
  non joignables depuis le Wi-Fi, à classer en risque accepté.
- Relevé Lysbor du 24/09/2026 (image 1.10.3) : 88 vulnérabilités ouvertes (230 en 1.10.0), aucune mise à jour
  disponible, 22 paquets sans correctif. Registre des décisions ci-dessous.
- Plan Lysbor du 29/09/2026 (image 1.10.4) : 33 vulnérabilités ouvertes, toutes sur rsync 3.4.1+ds1-5+deb13u4,
  corrigées par Debian en 3.5.0+ds1-0+deb13u1 (trixie-security). Image 1.10.5 : correctif installé par la mise à jour
  de sécurité à la fabrication, qui échoue désormais si rsync reste sous cette version.
- Plan Lysbor du 30/09/2026 (image 1.12.1) : 13 vulnérabilités sur openssl 3.5.7-1~deb13u3, annoncées « sans
  version corrective ». Faux positif : les 13 avis Debian (OSV, mis à jour le 30/09/2026 à 08:00 UTC) donnent
  3.5.7-1~deb13u3 comme version corrigée, et une requête OSV sur cette version ne renvoie aucune vulnérabilité (13 sur
  deb13u2). L'image porte donc déjà le correctif ; Lysbor s'appuie sur un état antérieur de la base. À relancer (renvoi
  du SBOM) puis, si l'alerte persiste, à classer en « non affecté » avec cette justification. La fabrication échoue
  désormais si libssl3t64, openssl ou openssl-provider-legacy restent sous 3.5.7-1~deb13u3.
- Plan Lysbor du 30/09/2026 à 15:56 UTC, après envoi du SBOM de l'image 1.13.0 (15:13 UTC) : mêmes 13 alertes.
  Le SBOM est juste : `libssl3t64`, `openssl` et `openssl-provider-legacy` en 3.5.7-1~deb13u3, `distro=debian-13`.
  OSV interrogé avec ces purl exactes : 0 vulnérabilité. Il n'en renvoie 50 que si l'on omet la version de Debian
  (écosystème `Debian` seul), qui mélange Debian 12 et 14, où ces CVE n'ont pas encore de correctif. L'erreur est
  donc côté Lysbor (base non rafraîchie depuis l'ajout du correctif, ou mauvaise lecture de la version corrigée) ;
  aucune action possible dans l'image. Décision : les 13 CVE sont tracées dans Lysbor comme faux positif (non
  affecté, correctif présent), et l'anomalie est signalée à l'éditeur. À réexaminer si une alerte subsiste après
  correction côté Lysbor.
- Relevé Lysbor du 30/09/2026 à 16:25 UTC, base OSV de Lysbor resynchronisée : les 13 alertes openssl ont disparu. Restent
  2 CVE sur dash 0.5.12-12, sans correctif dans aucune version de Debian (12, 13, 14) : risque accepté, voir registre.

#### Registre des risques acceptés (image carte SD)

Exposition mesurée sur l'image : sur le Wi-Fi Aurion, seuls l'interface Aurion, dnsmasq (DHCP et DNS, lancé par
NetworkManager), NetworkManager avec wpa_supplicant (point d'accès) et avahi (mDNS) reçoivent des données du
réseau. SSH est coupé. Les autres paquets ne traitent que des données locales ou venant d'Aurion.

| Paquet | Rôle dans l'image | Joignable depuis le Wi-Fi | Décision |
|---|---|---|---|
| network-manager | gère le point d'accès Wi-Fi | oui, indirectement | accepté (1.11.0) : les deux failles demandent un compte local sur le Pi (AV:L). CVE-2026-10805 (CVSS 3.1 `AV:L/AC:H/PR:L/UI:R`) touche le moteur DHCP dhclient, CVE-2025-9615 (`AV:L/AC:L/PR:L`) la lecture de fichiers d'un autre utilisateur ; aucune n'est déclenchable depuis le Wi-Fi |
| sudo | lance le helper root depuis Aurion | non, mais sert d'escalade si l'interface était compromise | accepté (1.11.0) : CVE-2026-82474 (CVSS 4.0 `AV:L/PR:L`) ne contourne que le mode `intercept` de sudo, jamais activé ici ; la règle n'autorise que `aurion-helper`, qui revalide chaque argument |
| rsync | copie du noyau et du micrologiciel vers `/boot/firmware` (dépendance de `raspi-firmware`) | non | gardé à jour (3.5.0+ds1-0+deb13u1 minimum, contrôlé à la fabrication) ; retrait impossible : `raspi-firmware`, indispensable au démarrage, en dépend |
| openssl (libssl3t64) | TLS de curl (mises à jour depuis GitHub), wpa_supplicant (point d'accès WPA2) | oui, indirectement (wpa_supplicant) | non affecté (30/09/2026) : les 13 CVE Lysbor du 30/09/2026 sont corrigées en 3.5.7-1~deb13u3, version installée (faux positif Lysbor) ; gardé à jour (minimum contrôlé à la fabrication) ; retrait impossible (bibliothèque TLS du système) |
| dash | `/bin/sh` du système (scripts de paquets, `system()` des programmes C) | non | accepté (30/09/2026) : aucun correctif Debian. CVE-2026-102473 (CVSS 5.5 `AV:L/PR:L`, déni de service CPU par un motif `*` répété) : le code vulnérable est bien présent (Debian compile dash avec `--disable-fnmatch`) mais il faut un compte local pour fournir le motif. CVE-2026-102474 (CVSS 4.0 `AV:L`, Debian « unimportant ») : débordement d'un ou deux octets dans `printf` avec un échappement `\u`/`\U` fourni par l'attaquant. Aurion ne passe jamais par `/bin/sh` : il lance directement `iw`, `ip`, `rpicam-*`, `vcgencmd`, `sync`, et ses scripts (`aurion-helper`, premier démarrage, règle udev) sont en bash. Aucune donnée venue du Wi-Fi n'atteint dash. Retrait impossible (paquet Essential). À réexaminer dès qu'un correctif Debian paraît : la mise à jour de sécurité à la fabrication l'installera |
| libxml2 | bibliothèque XML, seul utilisateur : shared-mime-info | non | accepté |
| systemd, util-linux, coreutils, tar, cpio, diffutils, bzip2, apt, kbd | socle du système | non | accepté |
| busybox, initramfs-tools, parted | démarrage et agrandissement de la carte au premier allumage | non | accepté |
| python3.13 | netplan (configuration réseau de NetworkManager), outils apt | non | accepté |
| perl, ack | scripts de paquets, gestion de la mémoire d'échange (rpi-swap) | non | accepté |
| libgcrypt20 | cryptographie locale (journal systemd) | non | accepté |
| nano, net-tools, unzip | maintenance au clavier | non | accepté ; retirables si besoin (gain faible) |

**Décision du 30/09/2026 (1.13.0) : paquets du système non mis à jour boîtier fermé.** La caméra ne rejoint jamais
Internet d'elle-même : elle émet son propre Wi-Fi, ou rejoint le partage de connexion du téléphone de son propriétaire
le temps d'une mise à jour d'Aurion. Les failles des paquets Debian relevées par Lysbor après la gravure de la carte
restent donc en risque accepté sur les caméras déployées : la mise à jour depuis le téléphone ne porte que sur
l'application et ses fichiers système. Chaque nouvelle image les corrige (mise à jour de sécurité à la fabrication) :
elle s'applique en regravant la carte. Une mise à jour complète du système boîtier fermé (deux systèmes A/B) est
étudiée dans `docs/CONCEPTION_AB.md`, reportée.

Réévaluation : à chaque relevé Lysbor (release ou renvoi manuel) ; tout correctif Debian publié entre dans l'image suivante
(mise à jour de sécurité à la fabrication).
- Actions GitHub figées par empreinte de commit (40 caractères) et non par étiquette (`@v4`), qu'un propriétaire
  peut déplacer vers un autre code (cas trivy-action). Dependabot (`.github/dependabot.yml`) propose chaque semaine
  une pull request qui met à jour empreinte et version ensemble. Réponse aux 20 alertes Semgrep
  `github-actions-mutable-action-tag` (CWE-1357).
- Limite actuelle : l'envoi direct n'applique pas le seuil `--fail-on` de l'agent `lysbor-scan`. Le job ne devient
  pas rouge sur une faille ; l'alerte vient de Lysbor.

## 6. Risques résiduels

| Risque | Niveau | Pourquoi il reste | Piste |
|---|---|---|---|
| Image carte SD : mot de passe Wi-Fi d'usine commun (`aurora2024`) jusqu'à ce que l'utilisateur le change | Moyen | choix d'accessibilité pour un public non technique (le mot de passe est imprimé dans le guide). Depuis la 1.6.0, l'accueil propose de le changer dès la première connexion (appliqué aussitôt) et la vérification avant la nuit le signale en orange. L'installation par script ou paquet génère toujours un mot de passe unique | rendre le changement obligatoire avant la première nuit, si le terrain montre que l'encadré est ignoré |
| Réinitialisation du mot de passe par un fichier sur la clé USB | Faible | qui peut brancher une clé et rallumer le Pi a de toute façon l'accès physique (carte SD, photos) ; le fichier n'agit qu'une fois et la remise à zéro est journalisée | aucune |
| Pas d'authentification applicative | Moyen | choix d'ergonomie : le Wi-Fi WPA2 fait office de clé | PIN optionnel (plan d'action A3) |
| Hotspot en WPA2-PSK (pas WPA3) | Faible | compatibilité avec tous les téléphones | option WPA3-SAE |
| Interface en HTTP (pas HTTPS) | Faible | réseau fermé du Pi ; un certificat auto-signé effraie les navigateurs | aucune dans l'immédiat |
| Service sans durcissement systemd poussé (`ProtectSystem`, `NoNewPrivileges`) | Faible | incompatible avec l'appel à sudo ; depuis 1.11.3, les options compatibles sont posées (`ProtectHome`, `ProtectKernelLogs`, `ProtectControlGroups`, `ProtectHostname`, `LockPersonality`, `RestrictRealtime`, `RestrictNamespaces`, `UMask`) : exposition `systemd-analyze security` 8,8 → 7,5 | déléguer les actions root à un service séparé (plan A4) |
