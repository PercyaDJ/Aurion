# Spécifications fonctionnelles

Version couverte : **1.6.0**. Ce document décrit **ce que fait** Aurion (le comment est dans [DAT.md](DAT.md)).

## 1. Utilisateurs

| Profil | Besoin | Ce qu'Aurion lui offre |
|---|---|---|
| **Débutant** (aucune connaissance informatique) | poser la caméra le soir, avoir des photos d'aurore le matin | image à flasher, un seul bouton « Lancer la nuit », vérifications en langage courant |
| **Photographe chasseur d'aurores** | RAW propres pour timelapse et retouche des pics d'activité | RAW DNG, balance des blancs fixe, exposition verrouillable, darks, classement des aurores, téléchargement RAW seul |
| **Mainteneur** | installer, mettre à jour, diagnostiquer | paquet, scripts, diagnostics, journaux de nuit, DEX |

## 2. Parcours principal

1. Préparer la carte SD (une fois), brancher caméra, carte, clé USB, alimentation.
2. Se connecter au Wi-Fi Aurion : la page s'ouvre seule, l'heure se règle seule.
3. Premier démarrage : choisir son mot de passe Wi-Fi.
4. Cadrer avec l'aperçu.
5. Vérifier la liste « Prêt pour la nuit ? », choisir mode, durée, format, **Lancer la nuit**.
6. Partir. La caméra photographie et s'éteint seule.
7. Le lendemain : rallumer, se connecter, « Dernière nuit », télécharger les RAW ou les plus belles aurores.

## 3. Exigences fonctionnelles

| ID | Exigence | Critère d'acceptation | Vérifié par |
|---|---|---|---|
| F1 | Le téléphone ouvre l'interface sans taper d'adresse | connexion au Wi-Fi `Aurion` : la page s'ouvre (portail captif) ; sinon `http://192.168.4.1:8080` | `captive.rs`, e2e « portail captif » |
| F2 | L'heure du Pi se règle depuis le téléphone | écart d'au moins 60 s corrigé à l'ouverture de la page ; jamais pendant une nuit | `api_tests`, `helper_test.sh` |
| F3 | Vérifications avant la nuit | caméra, clé et autonomie en heures, heure, alimentation, température, mot de passe ; un point rouge bloque le lancement | `preflight_*` (api_tests), e2e « accueil » |
| F4 | Lancer la nuit en un geste | mode, durée et format enregistrés puis nuit lancée ; Wi-Fi coupé 15 s après | e2e « lancement de la nuit » |
| F5 | Mode **Toute la nuit** (SAFE) | une image toutes les `capture_interval_secs` pendant toute la plage ; images avec aurore suffixées `_AURORA` | simulation `safe_mode_*` |
| F6 | Mode **Aurores seulement** (FILTER) | rien n'est enregistré avant N détections consécutives ; ensuite capture jusqu'à la fin | simulation `filter_mode_*` |
| F7 | Durée | plage horaire (défaut 21:00 à 06:00, attente si lancée plus tôt) ou minuteur de N heures à partir du lancement | simulation (plage, minuteur) |
| F8 | Fin autonome | à la fin : journal vidé, `sync`, extinction | simulation (`shutdown_count`) |
| F9 | Reprise après coupure | si la nuit est coupée avant sa fin, au redémarrage : Wi-Fi 5 min avec compte à rebours et boutons « Reprendre maintenant » / « Annuler », puis reprise seule jusqu'à la fin prévue ; 3 reprises au plus | `core/night.rs`, simulation `interrupted_night_*` |
| F10 | Photos RAW | format RAW + JPG, RAW seul ou JPG seul ; DNG natif jamais modifié | simulation RAW |
| F11 | Journal de nuit | `event.jsonl` (un événement par image : exposition, score, numéro) et `session.log` sur la clé | simulation |
| F12 | Dernière nuit | nombre de photos, d'aurores, de RAW, durée, meilleur score ; liens RAW et ZIP | `last_night_summary`, e2e |
| F13 | Téléchargement | ZIP par nuit : tout, RAW seuls, JPEG seuls ; sélection ; meilleures aurores avec leurs RAW | `session_zip_*`, e2e |
| F14 | Meilleures aurores | classement par score décroissant, avec le numéro d'image (retouche unitaire, timelapse) | e2e « meilleures aurores » |
| F15 | Réglages photo | ISO, pose, intervalle, balance des blancs fixe, verrou d'exposition, pixels chauds, empilement, zone analysée, seuils | e2e réglages |
| F16 | Presets | enregistrer, appliquer, supprimer ; le mot de passe Wi-Fi n'y figure jamais | e2e presets, sécurité |
| F17 | Darks | série de darks aux réglages du dernier aperçu | e2e darks |
| F18 | Mot de passe Wi-Fi | proposé au premier démarrage, 10 à 63 caractères, appliqué aussitôt ; réinitialisable par un fichier `aurion-reset-wifi.txt` sur la clé | `test_usb_wifi_reset`, e2e |
| F19 | Mode simple / expert | menu réduit par défaut ; « Mode expert » affiche réglages fins, presets, stockage ; mémorisé sur le téléphone | e2e « menu » |
| F20 | Mise à jour | depuis l'interface, fichier binaire contrôlé, refusée pendant la nuit, retour arrière possible | sécurité OTA |
| F21 | Extinction | bouton « Éteindre Aurion » ; extinction propre | api_tests |
| F22 | Clé pleine | la nuit s'arrête proprement sous le seuil critique (5 % ou 50 Mo libres) | simulation « disque plein » |

## 4. Règles de gestion

| ID | Règle |
|---|---|
| R1 | Nom d'une photo : `aurora_AAAAMMJJ_HHMMSS_NNNNN[_AURORA].jpg` (et `.dng` de même nom) |
| R2 | Une paire RAW + JPEG compte pour une seule aurore |
| R3 | Le mot de passe Wi-Fi n'est jamais renvoyé par l'API (valeur masquée) ; renvoyer la valeur masquée le conserve |
| R4 | Aucun réglage système (heure, Wi-Fi, mise à jour) n'est possible pendant une nuit |
| R5 | Minuteur : une nuit reprise s'arrête à l'heure de fin prévue et ne capture jamais plus que sa durée initiale (si l'horloge du Pi retarde après la coupure, seule la seconde borne s'applique) |
| R6 | En mode plage horaire, une nuit reprend si l'heure est dans la plage ou si elle a été lancée il y a moins de 12 h |
| R7 | Le fichier `aurion-reset-wifi.txt` n'agit qu'une fois (renommé `.done`) |
| R8 | La capacité affichée se base sur la taille moyenne des photos déjà sur la clé, sinon JPEG 4 Mo et DNG 24 Mo |

## 5. Exigences non fonctionnelles

| ID | Exigence |
|---|---|
| NF1 | Fonctionne sans internet, sur le seul Wi-Fi du Pi |
| NF2 | Consommation minimale : analyse sur miniature, Wi-Fi coupé la nuit, matériel inutile désactivé |
| NF3 | Aucune photo tronquée après une coupure de courant ; au plus une minute de journal perdue |
| NF4 | Interface utilisable sur un téléphone à 390 px de large, en français |
| NF5 | Aucune action root hors du helper validé ; protections CSRF et DNS rebinding |
| NF6 | Tout est testable sans matériel (simulateurs, temps virtuel) |
