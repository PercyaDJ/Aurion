# Journal des versions

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
