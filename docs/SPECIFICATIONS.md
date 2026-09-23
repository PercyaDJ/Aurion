# Spécifications fonctionnelles

Version couverte : **1.9.0**. Ce document décrit **ce que fait** Aurion (le comment est dans [DAT.md](DAT.md)).

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
| F22 | Clé pleine | la nuit s'arrête proprement sous le seuil critique (5 % ou 50 Mo libres) ; aucune photo effacée automatiquement | simulation « disque plein » |
| F23 | Un dossier par nuit | `sessions/<nuit>/RAW`, `JPG`, `thumbs`, journaux ; ZIP d'une nuit organisé de même ; anciennes photos (racine) toujours visibles | `each_night_has_its_folder_*`, `night_folders_*` |
| F24 | Index des aurores | `aurores.csv` en fin de nuit : une ligne par image avec aurore, de la plus forte à la plus faible, avec noms JPG et RAW | `aurora_index_lists_strongest_first` |
| F25 | JPG + RAW des aurores | JPEG toute la nuit ; RAW à partir de la première détection et jusqu'à 10 min après la dernière ; chaque RAW a son JPEG de même nom | `raw_only_during_auroras_saves_the_key` |
| F26 | Mode expédition | départ automatique 5 min après la dernière requête à l'interface, seulement avec heure fiable et clé présente ; raison affichée sinon | `expedition_*` |
| F27 | Réveil programmé (Pi 5) | fin de nuit en expédition : réveil 10 min avant la plage suivante ; nuit lancée plus de 2 h avant son début : extinction et réveil, puis reprise | `pi5_*` |
| F28 | Horloge matérielle | lue au démarrage si présente et valide ; mise à jour quand le téléphone donne l'heure ; source affichée | `rtc_detection`, api_tests |
| F30 | RAW seul, à la suite | format RAW par défaut ; aucune pause entre deux photos par défaut (la pose donne la cadence) ; pause réglable | `raw_only_back_to_back_follows_the_exposure_time` |
| F31 | Surveillance sobre | en surveillance, aucune lecture ni écriture de RAW ; profil d'énergie selon la phase | `watching_the_sky_never_captures_raw`, `night_power_profiles_follow_the_phase` |
| F32 | Mise à jour depuis GitHub | canal stable ou développement, via partage de connexion ou connexion actuelle, retour automatique au Wi-Fi Aurion, résultat affiché | `update_tests.rs` |
| F33 | Retour arrière | un geste rétablit la version précédente (et inversement) | `rollback_swaps_current_and_previous` |
| F34 | Tout-en-un | une image (système + application) sous un nom fixe, lien permanent vers la dernière version ; caméra prête 3 à 5 min après le premier allumage, sans internet | `image_test.sh` |
| F35 | Réglages sur la clé | chaque enregistrement copie les réglages sur la clé ; une carte SD neuve les reprend au premier démarrage ; le mot de passe du partage de connexion n'est jamais copié | `settings_survive_a_new_sd_card`, `saved_settings_are_copied_on_the_usb_key` |
| F29 | Autonomie en nuits | l'accueil affiche le nombre de nuits que la clé peut contenir (mode expédition) | `preflight_in_expedition_mode_*` |

## 4. Règles de gestion

| ID | Règle |
|---|---|
| R1 | Nom d'une photo : `aurora_AAAAMMJJ_HHMMSS_NNNNN[_AURORA].jpg` (et `.dng` de même nom) |
| R2 | Une paire RAW + JPEG compte pour une seule aurore |
| R3 | Le mot de passe Wi-Fi n'est jamais renvoyé par l'API (valeur masquée) ; renvoyer la valeur masquée le conserve |
| R4 | Aucun réglage système (heure, Wi-Fi, mise à jour) n'est possible pendant une nuit |
| R5 | Minuteur : une nuit reprise s'arrête à l'heure de fin prévue et ne capture jamais plus que sa durée initiale (si l'horloge du Pi retarde après la coupure, seule la seconde borne s'applique) |
| R7 | Le fichier `aurion-reset-wifi.txt` n'agit qu'une fois (renommé `.done`) |
| R8 | La capacité affichée se base sur le débit réel de la dernière nuit (au moins 20 images sur 15 min) ; à défaut, sur la taille moyenne des photos (fichiers de moins de 200 ko ignorés) ou JPEG 4 Mo et DNG 14 Mo, avec la pose maximale plus 2 s par prise |
| R9 | Une nuit reprise continue dans son dossier, avec la numérotation qui suit la dernière image enregistrée |
| R10 | En plage horaire, une nuit lancée à l'avance reprend tant que la fin de cette nuit-là n'est pas passée |
| R11 | Le départ automatique n'a jamais lieu tant que l'heure n'est pas fiable (téléphone ou horloge matérielle) |

## 5. Exigences non fonctionnelles

| ID | Exigence |
|---|---|
| NF1 | Fonctionne sans internet, sur le seul Wi-Fi du Pi |
| NF2 | Consommation minimale : analyse sur miniature, Wi-Fi coupé la nuit, matériel inutile désactivé |
| NF3 | Aucune photo tronquée après une coupure de courant ; au plus une minute de journal perdue |
| NF4 | Interface utilisable sur un téléphone à 390 px de large, en français |
| NF5 | Aucune action root hors du helper validé ; protections CSRF et DNS rebinding |
| NF6 | Tout est testable sans matériel (simulateurs, temps virtuel) |
