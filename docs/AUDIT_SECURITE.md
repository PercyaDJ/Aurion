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

Surfaces analysées : API HTTP (39 routes, dont 9 de portail captif), pages web, helper root, sudoers, installeur, service systemd, dépendances Rust.

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

## 3. Le helper root (`scripts/aurion-helper`)

Seul programme exécutable en root par le service. Principes :
- une liste fermée de commandes (`ap-start`, `ap-stop`, `wifi-scan`, `wifi-connect`, `set-time`, `set-timezone`, `shutdown`, `mount-usb`, `umount-usb`, `usb-add`) ;
- chaque argument validé par expression régulière avant usage ;
- secrets lus sur l'entrée standard ;
- `PATH` et `LC_ALL` fixés ; `umask 077` ;
- les crochets de test (`AURION_HELPER_DRYRUN`, `AURION_ENV_FILE`) sont ignorés dès qu'il est appelé via sudo.

42 cas testés en mode simulation (dont les injections). Incident pendant la revue : un test lancé en root a réellement réglé
l'horloge du conteneur de développement ; le test a été remplacé par une commande sans effet réel et le mode simulation
n'écrit plus rien dans `/run`.

## 4. Dépendances (cargo audit, base RustSec du 23/09/2026, 1267 avis)

| Crate | Version | Avis | Statut |
|---|---|---|---|
| anyhow | 1.0.101 → 1.0.104 | RUSTSEC-2026-0190 (unsound `downcast_mut`) | **corrigé** par mise à jour |
| spin | 0.9.8 → 0.9.9 | version retirée (yanked) | **corrigé** par mise à jour |
| rand | 0.8.5 | RUSTSEC-2026-0097 (unsound avec un logger personnalisé) | accepté : utilisé seulement par l'outil de test `axum-test`, absent du binaire embarqué |

Aucune vulnérabilité (« vulnerability ») connue ; aucun avis ne concerne le binaire livré sur le Pi.

## 5. Risques résiduels

| Risque | Niveau | Pourquoi il reste | Piste |
|---|---|---|---|
| Pas d'authentification applicative | Moyen | choix d'ergonomie : le Wi-Fi WPA2 fait office de clé | PIN optionnel (plan d'action A3) |
| `nmcli` reçoit le mot de passe du hotspot en argument pendant une fraction de seconde | Faible | limitation de `nmcli` ; processus root, aucun autre utilisateur sur le Pi | fichier de connexion NetworkManager en 0600 |
| Hotspot en WPA2-PSK (pas WPA3) | Faible | compatibilité avec tous les téléphones | option WPA3-SAE |
| Interface en HTTP (pas HTTPS) | Faible | réseau fermé du Pi ; un certificat auto-signé effraie les navigateurs | aucune dans l'immédiat |
| Mise à jour OTA non signée | Moyen | contrôles de format et d'exécution mais pas de signature | signature Ed25519 des releases (plan A2) |
| Service sans durcissement systemd poussé (`ProtectSystem`, `NoNewPrivileges`) | Faible | incompatible avec l'appel à sudo | déléguer les actions root à un service séparé (plan A4) |
