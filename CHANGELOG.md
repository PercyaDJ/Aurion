# Journal des versions

## 1.9.0

Objectif : tout-en-un. Un fichier à graver (système + application), la caméra prête en quelques minutes, et une
carte SD qu'on peut regraver sans rien perdre.

- Image au nom fixe **aurion-raspios-arm64.img.xz** : lien permanent vers la dernière version
  (`releases/latest/download/aurion-raspios-arm64.img.xz`), page de release centrée sur ce fichier.
- **Réglages gardés sur la clé USB** (`aurion-reglages.json`, écrit à chaque enregistrement) et repris
  automatiquement au premier démarrage d'une carte SD neuve ; la clé est la mémoire de la caméra.
- Au démarrage, la clé est montée avant le Wi-Fi Aurion : le mot de passe repris s'applique tout de suite ; le
  fichier de réinitialisation du mot de passe marche aussi au premier démarrage.
- Guide : système ou application, quoi mettre à jour et comment ; durées de mise en route.

## 1.8.0

Objectif : RAW seul, photos à la suite, le moins de consommation possible, et des cycles essai / correction de
quelques minutes depuis le téléphone.

### Photo et stockage
- **RAW seul par défaut** (DNG), photos **à la suite** : la pause entre deux photos vaut 0 par défaut, la pose
  choisie par l'exposition automatique donne la cadence. Réglable (*Pause entre deux photos*).
- Miniatures de galerie aussi pour les nuits en RAW seul.
- Autonomie de la clé calculée sur le **débit réel** de la dernière nuit ; taille type d'un DNG de nuit ramenée à
  14 Mo (mesure de terrain 12 à 15 Mo). Correction des chiffres de stockage de la 1.7 (surestimés).
- Durée réelle de chaque prise enregistrée (`capture_ms` dans `event.jsonl`).

### Consommation
- Surveillance (*Aurores seulement*) : plus aucun RAW lu ni écrit tant que l'aurore n'est pas confirmée.
- Profil d'énergie de nuit (`aurion-helper power-profile`) : processeur au minimum en surveillance, normal en
  capture, port Ethernet coupé s'il n'y a pas de câble.
- Option expérimentale **Prise directe** (`rpicam-still --immediate`) à comparer sur le terrain.

### Mises à jour sans ordinateur
- **Mettre à jour depuis GitHub** (*Diagnostics*) : canal stable (dernière release) ou développement (pré-release
  `edge` reconstruite à chaque modification de `main`). Le Pi rejoint le partage de connexion du téléphone, télécharge,
  vérifie, installe, puis revient sur son Wi-Fi ; résultat conservé après le redémarrage.
- **Revenir à la version précédente** en un geste.
- Version affichée avec le commit pour les versions de développement (`1.8.0-edge.xxxxxxx`).
- Le binaire nu `aurion-arm64` est joint à chaque release ; `curl` ajouté aux paquets installés.
- Version du programme système (`aurion-helper version`) vérifiée : *Diagnostics* signale quand l'image ou le
  `.deb` doit être réinstallé.

### Tests
- 220 tests Rust (30 nuits simulées, dont RAW à la suite avec vraie durée de pose, surveillance sans RAW,
  profils d'énergie ; mise à jour contre un faux GitHub, retour arrière), 57 cas du helper, 29 étapes navigateur.

## 1.7.0

Objectif : poser Aurion pour une expédition de plusieurs semaines, dans l'ordre stable, fiable, simple, complet.

### Autonomie sur plusieurs nuits
- **Mode expédition** : à l'allumage, la nuit démarre seule 5 min après la dernière utilisation de l'interface,
  seulement si l'heure est juste (téléphone ou horloge matérielle) et la clé présente.
- **Raspberry Pi 5** : réveil programmé par son horloge (RTC) 10 min avant la nuit suivante ; si une nuit est lancée
  plus de 2 h avant son début, il s'éteint et se rallume seul au lieu d'attendre allumé.
- Horloge matérielle lue au démarrage (Pi 5 ou module DS3231) ; l'heure envoyée par le téléphone met aussi
  l'horloge matérielle à jour.
- Reprise après coupure : même dossier de nuit, numérotation continue, fichiers incomplets nettoyés ; une nuit
  lancée le matin pour le soir reprend correctement.

### Photos et stockage
- **Un dossier par nuit** sur la clé : `sessions/<nuit>/RAW`, `JPG`, `thumbs`, journaux, et **`aurores.csv`**
  (images avec aurore, de la plus forte à la plus faible, noms des JPG et RAW).
- Nouveau format **JPG + RAW des aurores** : JPEG toute la nuit pour le timelapse, RAW pendant les aurores et
  10 min après ; plusieurs nuits tiennent sur une clé.
- ZIP d'une nuit organisé comme le dossier de la clé ; galerie compatible avec les photos des versions précédentes
  (à la racine de la clé).
- Autonomie de la clé affichée en nuits ; les fichiers vides ou tronqués ne faussent plus l'estimation.

### Fiabilité
- La galerie ne liste plus qu'au plus 1000 images récentes par requête (et filtre par nuit) : plus de page figée
  avec des dizaines de milliers de photos.
- L'heure est renvoyée par le téléphone à chaque chargement de page (et de nouveau si le Pi en doute).

### Tests
- 212 tests Rust (27 nuits simulées, dont démarrage automatique, veille et réveil du Pi 5, RAW pendant les aurores,
  reprise dans le même dossier), 48 cas du helper root, 29 étapes navigateur.

## 1.6.0

Objectif : poser Aurion le soir, récupérer ses photos le matin, sans friction.

### Interface
- Nouvel accueil unique : vérifications « Prêt pour la nuit ? » (caméra, clé et autonomie en heures, heure,
  alimentation, température, mot de passe), choix du mode (Toute la nuit / Aurores seulement), de la durée et du
  format, bouton **Lancer la nuit**, écran « Nuit en cours », résumé de la dernière nuit.
- Téléchargement des **RAW seuls** en un geste (accueil) et boutons RAW / JPG / Tout pour chaque nuit (Photos).
- Menu unique, **mode simple** (5 entrées) et **mode expert** (réglages fins, presets, stockage).
- Premier démarrage : choix du mot de passe Wi-Fi depuis l'accueil, appliqué aussitôt.
- L'ancien tableau de bord redirige vers l'accueil. Rafraîchissement suspendu quand l'écran est éteint.

### Autonomie et fiabilité
- **Reprise automatique d'une nuit interrompue** (batterie vide ou changée) : Wi-Fi 5 min avec compte à rebours,
  annulable, puis reprise jusqu'à la fin prévue ; 3 reprises au plus.
- Mot de passe Wi-Fi oublié : fichier `aurion-reset-wifi.txt` sur la clé USB.
- Changement du Wi-Fi appliqué immédiatement (plus besoin de redémarrer).

### API
- `GET /api/preflight`, `GET /api/night/last`, `POST /api/night/resume/cancel`.
- `GET /api/gallery/sessions/:name/download?only=raw|jpg` (filtre inconnu refusé).

### Documentation
- Nouveaux : DAT, DEX, spécifications fonctionnelles, conception UX, ce journal.
- Guide de démarrage, audits, tests et plan d'action mis à jour.

### Tests
- 194 tests Rust (dont 20 nuits simulées en temps virtuel, 4 sur la reprise après coupure), 28 étapes navigateur.

## 1.5.0

- Image carte SD prête à flasher construite et publiée automatiquement, installation au premier démarrage.
- Sécurité : helper root unique, anti-CSRF, anti-DNS-rebinding, mise à jour contrôlée, mot de passe jamais exposé.
- Photo : analyse sur miniature EXIF, balance des blancs fixe, verrou d'exposition, pixels chauds, empilement,
  darks, classement des meilleures aurores.
- Batterie : Bluetooth, audio et LED coupés, journaux en RAM, écritures atomiques, réparation de la clé au montage.
- Tests unitaires, API, sécurité, simulation, non-régression, installeur, helper, image, navigateur.
