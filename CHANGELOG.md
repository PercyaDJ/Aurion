# Journal des versions

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
