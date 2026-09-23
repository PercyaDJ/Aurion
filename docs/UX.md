# Conception de l'expérience (UX / UI)

Version couverte : **1.6.0**.

## 1. Principe directeur

Aurion doit se comporter comme un appareil, pas comme un ordinateur. Analogie : un réveil. On règle l'heure,
on appuie sur un bouton, on n'y pense plus. Tout ce qui ressemble à de l'informatique (adresses, fichiers,
phases techniques) est caché ou traduit en phrases courantes.

Deux niveaux d'usage coexistent :
- **Simple** : l'accueil suffit pour poser la caméra, lancer la nuit et récupérer les photos.
- **Expert** : un interrupteur dans le menu révèle tous les réglages fins, sans rien retirer au débutant.

## 2. Frictions identifiées et réponses

| Friction (avant 1.6) | Réponse |
|---|---|
| Deux écrans d'entrée (accueil puis tableau de bord) avant de pouvoir lancer | un seul accueil qui contient tout le parcours ; l'ancien tableau de bord redirige |
| Bouton « Déconnexion, lancer la capture » : libellé technique | **Lancer la nuit**, avec le résumé de ce qui va se passer dans la confirmation |
| Alertes techniques (« Clé USB non montée ») sans solution | liste « Prêt pour la nuit ? » : un point vert, orange ou rouge et, sous chaque point, **quoi faire** |
| Aucune idée de la place restante en heures | « environ 4 h 48 de photos avec ces réglages », recalculé selon le format choisi |
| Mode SAFE / FILTER : jargon | « Toute la nuit » (idéal timelapse) et « Aurores seulement » (économise la clé) |
| Changement du mot de passe caché dans les réglages avancés, actif au redémarrage | encadré au premier démarrage, appliqué aussitôt, procédure de secours affichée |
| Mot de passe oublié : regraver la carte | fichier `aurion-reset-wifi.txt` sur la clé USB |
| Récupérer les RAW : galerie, onglet, sélection | « Dernière nuit » sur l'accueil : **Télécharger les RAW** en un geste ; boutons RAW / JPG / Tout sur chaque nuit |
| Nuit perdue si la batterie lâche | reprise automatique, annulable depuis le téléphone |
| Menu de 9 entrées | 5 entrées en mode simple, 8 en mode expert |
| Pendant la nuit, l'écran affichait des phases (`CALIBRATION`, `RUN`) | écran « Nuit en cours » avec trois consignes claires |

## 3. Écrans

| Écran | Contenu | Mode |
|---|---|---|
| Accueil | reprise éventuelle, mot de passe (premier démarrage), vérifications, choix de la nuit, **Lancer la nuit**, dernière nuit, cadrage, extinction | simple |
| Nuit en cours | remplace l'accueil dès que la nuit démarre : consignes, rien à toucher | simple |
| Photos | toutes les images, par nuit (RAW / JPG / Tout), meilleures aurores ; `#best` et `#sessions` ouvrent directement l'onglet | simple |
| Cadrage | aperçu, darks | simple |
| Réglages photo | ISO, pose, intervalle, format | simple |
| Réglages experts | balance des blancs, verrou d'exposition, débruitage, détection, réseau | expert |
| Presets | réglages enregistrés | expert |
| Stockage | espace, état de la clé | expert |
| Diagnostics et mise à jour | version, capteurs, journal, mise à jour, Wi-Fi maison | simple |

## 4. Règles d'interface

- **Une action principale par écran**, visuellement dominante (dégradé aurore). Les actions secondaires sont en contour.
- **Toujours dire quoi faire** : chaque erreur ou alerte est suivie d'une consigne concrète.
- **Pas de cul-de-sac** : un point rouge explique comment le lever ; « Plus tard » existe pour le mot de passe.
- **Confirmation seulement pour l'irréversible** : lancer la nuit (le Wi-Fi va se couper), éteindre, supprimer.
- **Libellés en français courant**, heures au format « 4 h 48 », pas d'unité technique sans explication.
- **Thème sombre** obligatoire : lisible la nuit, n'éblouit pas, ne pollue pas les photos voisines.
- **Téléphone d'abord** : conçu et testé à 390 px de large, gros boutons.
- **Accessibilité** : contrastes du thème sombre, `aria-label` sur le menu, animations coupées si le téléphone
  demande de réduire les mouvements.
- **Sobriété** : le rafraîchissement automatique s'arrête quand l'écran du téléphone s'éteint ou que l'onglet est
  caché (moins de réveils du Pi et du téléphone).

## 5. Implémentation

- Menu unique généré par `js/common.js` (`AURION_MENU`), interrupteur « Mode expert » mémorisé dans le
  navigateur (`localStorage`, clé `aurionExpert`).
- `aurionPoll(fn, ms)` remplace `setInterval` : rafraîchissement suspendu quand la page est cachée.
- L'accueil s'appuie sur `/api/preflight`, `/api/night/last`, `/api/config`, `/api/disconnect`,
  `/api/night/resume/cancel`.
- Tests : `tests/e2e/ui_test.mjs` (accueil, menu simple et expert, redirection, mot de passe, lancement de la nuit,
  téléchargements par nuit).

## 6. Pistes suivantes

- Mesurer sur le terrain le temps entre l'allumage et le lancement de la nuit (objectif : moins de 2 minutes).
- Aperçu en direct plein écran avec aide à la mise au point (zoom sur une étoile).
- Notification « faites vos darks » à la fin d'une nuit (PLAN_ACTION.md, Q2).
