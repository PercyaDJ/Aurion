# Plan d'action

Priorités : **P0** avant la prochaine sortie terrain, **P1** prochaine version, **P2** ensuite, **P3** idées.
Effort : S (moins d'une demi-journée), M (1 à 2 jours), L (plus).

## Validation sur le matériel (P0)

| ID | Action | Effort | Critère de réussite |
|---|---|---|---|
| V1 | Premier essai complet sur le Pi : installation avec l'image carte SD (docs/GUIDE_DEMARRAGE.md), hotspot, portail captif iOS et Android, preview, darks, nuit courte en mode minuteur (0,5 h) | S | Wi-Fi visible, page ouverte seule, DNG + JPEG + miniatures sur la clé, `sudo journalctl -u aurion` sans erreur |
| V2 | Vérifier les options `rpicam-still` utilisées (`--thumb`, `--denoise`, `--awb daylight`, `--raw`) sur votre version de Raspberry Pi OS | S | une capture manuelle avec ces options réussit ; `aurion bench` et le journal indiquent « EXIF thumbnail » |
| V3 | Test de coupure : débrancher l'alimentation pendant une capture, rebrancher | S | clé réparée au montage, aucune image tronquée, journal lisible jusqu'à la dernière minute |
| V4 | Recalibrer les seuils de détection (la zone analysée est désormais de 65 % au lieu de 42 %) sur de vraies images d'aurore et de ciel vide | M | aucun faux positif sur une nuit sans aurore, détection d'une aurore faible visible à l'œil |
| V5 | `aurion bench` sur le Pi et mesure de la température CPU pendant une nuit | S | temps réels consignés dans AUDIT_CODE.md § 5 |
| V6 | Reprise après coupure : lancer une nuit en minuteur 1 h, débrancher après 20 min, rebrancher sans téléphone | S | Wi-Fi visible 5 min, puis reprise ; `session.log` montre « Reprise de la nuit interrompue » ; extinction à l'heure prévue |
| V7 | Vérifier l'heure du Pi 4 après une coupure (présence de `fake-hwclock` sur l'image) | S | l'heure au redémarrage est proche de l'heure de la coupure, sinon documenter l'écart |
| V8 | Pi 5 en expédition : réveil par l'alarme RTC après l'extinction, consommation éteint (avec et sans `POWER_OFF_ON_HALT=1` dans l'EEPROM), comportement de la batterie quand la charge devient très faible | S | le Pi se rallume seul à l'heure programmée ; consommation mesurée consignée dans GUIDE_EXPEDITION.md |
| V9 | Pi 4 + module DS3231 (`dtoverlay=i2c-rtc,ds3231`) : `/sys/class/rtc/rtc0` présent, heure juste après une coupure sans téléphone, démarrage automatique en expédition | S | journal « Heure donnée par l'horloge matérielle » ; nuit démarrée seule |
| V11 | Prise directe (`--immediate`) : comparer `capture_ms` et la consommation avec et sans, vérifier la qualité des DNG | S | option activée par défaut si le gain est réel et les DNG identiques |
| V12 | Mise à jour depuis GitHub par le partage de connexion d'un iPhone et d'un Android, retour au Wi-Fi Aurion | S | version affichée après reconnexion, retour arrière fonctionnel |
| V13 | Temps d'allumage : lire « Wi-Fi prêt après l'allumage » (Diagnostics) sur Pi 4 et Pi 5, avec et sans clé ; `systemd-analyze blame` pour repérer les services lents | S | Wi-Fi visible le plus vite possible ; chiffres consignés dans GUIDE_DEMARRAGE.md |
| V14 | Préparer la clé : clé neuve non formatée, clé ext4, clé NTFS ; vérifier qu'elle est montée et utilisée juste après | S | point vert « Clé USB » sans débrancher |
| V15 | Image 1.10.4 allégée : premier allumage, Wi-Fi Aurion, caméra, clé, mise à l'heure du module horloge (`hwclock -r` après une synchronisation par le téléphone) | S | tout fonctionne comme en 1.10.0 |
| V10 | ISO maximal utile pour le RAW : vérifier au-delà de quel ISO le gain devient numérique (sans effet sur le DNG) sur l'IMX477 | S | valeur mesurée ; `iso_max` par défaut ajusté si nécessaire |

## Énergie (P1)

| ID | Action | Effort | Gain attendu |
|---|---|---|---|
| E1 | Mesurer le courant (testeur USB) : repos, surveillance, capture, avec et sans le bloc d'économie d'énergie | S | chiffres réels d'autonomie par capacité de batterie |
| E2 | ~~Fréquence CPU réduite~~ : profil d'énergie de nuit fait en 1.8.0 (processeur au minimum en surveillance) ; reste à mesurer le gain (E1) et, si utile, `arm_freq` en capture | S | moins de consommation et de chauffe |
| E3 | Planification : Pi éteint en journée et réveil programmé (Pi 5 : RTC intégrée + `rtcwake`) | M | autonomie sur plusieurs nuits |
| E4 | Mode « économie maximale » : capture seulement en RAW, galerie générée à la demande | S | quelques % de CPU en moins |

## Qualité photo (P1 / P2)

| ID | Priorité | Action | Effort |
|---|---|---|---|
| Q1 | P1 | Enregistrer la température du capteur et les réglages réellement appliqués par image (`rpicam-still --metadata`, si disponible sur votre version) dans `event.jsonl` | S |
| Q2 | P1 | Rappel automatique « faites vos darks » à la fin de la nuit (notification au prochain allumage), aux réglages moyens de la nuit | S |
| Q3 | P1 | Rampe d'exposition douce au crépuscule (« holy grail ») : variation limitée par image, puis verrou une fois la nuit noire | M |
| Q4 | P2 | Intervalle adaptatif : plus rapide quand le score d'aurore monte, plus lent sinon (place disque et batterie) | M |
| Q5 | P2 | Rafale automatique en RAW sur les pics d'aurore (score au-dessus d'un seuil) pour l'empilement au post-traitement | M |
| Q6 | P2 | Carte des pixels chauds apprise à partir des darks et fournie comme fichier pour le post-traitement | M |
| Q7 | P3 | Détection plus fine (structures en arc / rideau, rejet des nuages éclairés) sur un jeu d'images réelles annotées | L |

## Sécurité (P1 / P2)

| ID | Priorité | Action | Effort |
|---|---|---|---|
| A1 | P1 | Écrire le mot de passe du hotspot dans un fichier de connexion NetworkManager (0600) plutôt qu'en argument de `nmcli` | S |
| A2 | P1 | Signer les releases (Ed25519) et vérifier la signature avant toute mise à jour OTA ou `get.sh` | M |
| A3 | P2 | Code PIN optionnel pour l'interface (utile en mode maintenance sur un réseau partagé) | M |
| A4 | P2 | Séparer les actions root dans un petit service dédié (socket local) pour durcir `aurion.service` (`ProtectSystem`, `NoNewPrivileges`) | L |
| A5 | P3 | Option WPA3-SAE pour le hotspot | S |

## Expérience utilisateur (P1 / P2)

| ID | Priorité | Action | Effort |
|---|---|---|---|
| U1 | P1 | Mesurer sur le terrain le temps entre l'allumage et le lancement de la nuit (objectif : moins de 2 minutes) | S |
| U2 | P2 | Aperçu plein écran avec loupe pour la mise au point sur une étoile | M |
| U3 | P2 | Aperçu du timelapse de la dernière nuit (miniatures animées) sur l'accueil | M |

## Code et tests (P2)

| ID | Action | Effort |
|---|---|---|
| C1 | Découper `orchestrator::run` en étapes testables séparément | M |
| C2 | ~~Un dossier par nuit sur la clé~~ : fait en 1.7.0 (les anciennes captures restent lisibles à la racine, sans migration) | - |
| C3 | Factoriser le CSS et le JavaScript des pages (menu et rafraîchissement factorisés en 1.6.0 ; reste : styles en ligne de la galerie et des réglages) | M |
| C4 | Tests sur banc matériel : un Pi dédié en CI (runner auto-hébergé) qui lance V1 automatiquement | L |
| C5 | Porter la couverture de `web/api.rs` au-delà de 85 % en simulant `rpicam-still` (script factice dans le `PATH`) | S |

## Déploiement (P2)

| ID | Action | Effort |
|---|---|---|
| D1 | Dépôt APT signé (GitHub Pages) : `sudo apt install aurion` et mises à jour par `apt upgrade` | M |
| D2 | ~~Image carte SD prête à flasher construite par la CI~~ : fait en 1.5.0 (`scripts/build-image.sh`, release automatique) | - |
| D3 | Rendre le dépôt public (téléchargement de l'image sans compte GitHub pour les utilisateurs) | S |
| D4 | ~~Réinitialisation du mot de passe Wi-Fi sans regraver la carte~~ : fait en 1.6.0 (fichier `aurion-reset-wifi.txt` sur la clé) | - |
