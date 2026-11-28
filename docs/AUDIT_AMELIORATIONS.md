# Audit d'amélioration et feuille de route (novembre 2026)

Ce document rassemble quatre études menées le 28/11/2026 sur Aurion 1.14.0 :

- **Annexe A**, audit du code : qualité d'image, détection, énergie, installation. Lecture du code uniquement, rien n'a
  été exécuté sur le matériel.
- **Annexe B**, étude des capteurs : capteurs pour Raspberry Pi, caméras astronomiques USB, boîtiers plein format.
- **Annexe C**, étude optique, embarqué, état de l'art de la détection et du débruitage.
- **Annexe D**, étude chiffrée de la triangulation de l'altitude des aurores depuis la région de Grenoble.

Chaque chiffre des annexes B et C cite sa source. Ce qui n'a pas pu être vérifié est marqué « non vérifié », ce qui
est calculé est marqué « calcul ». Rien n'a encore été mesuré sur une caméra Aurion réelle : les gains annoncés sont des
estimations à confirmer par les validations matérielles (tâches V du [plan d'action](PLAN_ACTION.md)).

---

## 1. Les deux usages visés

| | Photographe | Réseau scientifique (association) |
|---|---|---|
| But | timelapse et photos d'aurores de la meilleure qualité possible | mesurer l'altitude des aurores par triangulation entre plusieurs stations |
| Image | RAW couleur, peu de bruit, dynamique | RAW couleur **calibrée** : position de chaque pixel sur le ciel, réponse du capteur connue |
| Temps | l'heure à la seconde suffit | horodatage précis et commun à toutes les stations |
| Mobilité | transportable, une nuit ou une expédition | posée plusieurs nuits en autonomie, une dizaine de stations (région de Grenoble) |
| Budget | grand public | financement possible par l'association |

Le cœur logiciel reste le même pour les deux : un seul produit, avec des options matérielles par palier.

## 2. Ce qu'il faut retenir

1. **Le logiciel bride aujourd'hui le matériel.** Chaque nuit démarre sous-exposée (départ au minimum, montée de 3 %
   par image), la détection a une hystérésis sans effet, et tout le traitement se fait sur une vignette JPEG 8 bits de
   320×240 : le RAW 12 bits n'est jamais exploité. Corriger cela ne coûte rien et passe avant tout achat (annexe A,
   section 0).
2. **L'objectif est le levier matériel le moins cher.** Pour une source étendue comme une aurore, la lumière reçue par
   pixel varie comme 1/N² (N = nombre d'ouverture) : passer de f/2,5 à f/1,4 donne environ 3,2 fois plus de lumière
   (calcul, annexe C 1.3).
3. **Le filtre infrarouge d'origine de la HQ Camera coupe environ 70 % de la raie rouge à 630 nm** (lecture graphique
   de la courbe officielle, annexe C 1.5). C'est gênant pour les aurores rouges, fréquentes aux latitudes françaises.
4. **Plein format : impossible sur la nappe caméra du Pi, possible en USB.** Le plus grand capteur trouvé en MIPI CSI
   est un 4/3" (carte artisanale). Le Pi peut en revanche piloter en USB une caméra astronomique (ZWO, Player One,
   QHY) ou un boîtier plein format via gphoto2 (annexe B). Le Pi n'est donc pas la limite : c'est l'écosystème des
   capteurs à nappe qui l'est.
5. **La triangulation est une méthode établie** avec des paires de caméras all-sky (réseau MIRACLE en Fennoscandie,
   méthode automatique de Whiter et al. 2013 :
   [gi-2-131-2013](https://gi.copernicus.org/articles/2/131/2013/gi-2-131-2013.pdf)). MIRACLE utilise des filtres
   interférentiels à 630,0, 557,7 et 427,8 nm sur des caméras monochromes, pas un capteur couleur. Elle impose des
   exigences qu'Aurion ne remplit pas encore (section 4).

## 3. Caméra astronomique USB : ce que l'on gagne, ce que l'on perd

Exemple : ZWO ASI585MC (Sony IMX585, 1/1,2", 8,3 Mpx, pixels de 2,9 µm). Gain par pixel à ouverture égale par rapport
à la HQ Camera : environ ×3,5 (rapport des surfaces de pixel, sans tenir compte du rendement quantique, annexe B).

**Ce que l'on gagne**
- RAW pur, sans traitement d'image imposé, avec gain analogique, offset, binning et temps de pose réglés directement
  par le kit de développement (SDK) du fabricant. C'est plus fin et plus propre scientifiquement que `rpicam-still`.
- Bruit de lecture inférieur à 1 électron (annexe B, source ZWO).
- Caméras déjà gérées par les logiciels all-sky libres sur Raspberry Pi (indi-allsky, Allsky).

**Ce que l'on perd ou ce qui change**
- Le SDK ZWO est une bibliothèque fermée (existe en ARM64). Il faut écrire un nouvel adaptateur caméra dans Aurion :
  l'architecture s'y prête, la caméra est déjà derrière une interface (`src/ports/camera.rs`).
- Monture : filetage T2/M42 avec un tirage mécanique court (12,5 ou 17,5 mm selon la bague pour la version non
  refroidie, d'après un revendeur : [Agena](https://agenaastro.com/zwo-asi585mc-cmos-color-astronomy-imaging-camera.html)).
  Un objectif en monture C (tirage 17,526 mm) est donc probablement montable avec la bonne bague, **à vérifier avec
  la documentation ZWO avant achat**.
- Pas d'aperçu ni de balance des blancs automatique : c'est à Aurion de les faire, ce qu'il fait déjà en partie.
- Consommation et tenue au froid : non vérifiées pour ce modèle.

## 4. Exigences propres au réseau scientifique

Ces exigences n'existent pas dans Aurion aujourd'hui. Elles sont listées pour ne pas les rendre impossibles par les
choix des lots 1 à 5.

| Exigence | Pourquoi | Piste |
|---|---|---|
| Horodatage commun précis | comparer la même aurore au même instant depuis plusieurs stations | module GPS avec signal PPS sur le Pi et chrony ; 10 ms ou mieux, cible 1 ms (annexe D 4.2). Aujourd'hui Aurion date l'image au lancement de `rpicam-still`, pas au début réel de la pose : à changer |
| Position de la station | base de la triangulation | GPS du même module |
| Calibration astrométrique | savoir quelle direction du ciel voit chaque pixel | résolution sur les étoiles (« plate solving », par exemple astrometry.net), refaite chaque nuit claire |
| Calibration photométrique | comparer les intensités entre stations | darks, flats, même modèle de capteur et d'objectif partout |
| Format de données | échange entre stations et outils scientifiques | FITS avec métadonnées complètes (heure, position, pose, gain, température) en plus du DNG |
| Prises de vue synchronisées | images simultanées sur toutes les stations | départ de pose calé sur l'heure GPS |
| Collecte centralisée | réunir les images des stations | envoi quand un réseau est disponible, ou relevé des clés USB |
| Géométrie du réseau | précision de l'altitude | **étudiée en annexe D** : une dizaine de stations sur 50 à 100 km autour de Grenoble ne suffit pas pour les aurores basses au nord (environ 10 km d'erreur sur un arc vert à 52°N, 86 km à 55°N, pour 1 mrad de pointage). Il faut une chaîne nord-sud de 250 à 400 km au moins (phase 1 minimale : Grenoble, Dijon ou Besançon, Nancy). Le groupe alpin reste utile au zénith (arcs SAR, STEVE) et contre les nuages |

## 5. Lots de travaux proposés

Ordre conseillé : les lots 1 et 2 d'abord (sans achat), puis les autres selon les priorités de l'association.

**État** : lot 1 réalisé en version 1.15.0 (voir le [journal des versions](../CHANGELOG.md)), sauf la traduction des
erreurs caméra et le QR code Wi-Fi, reportés au lot 5. Lot 2 réalisé en version 1.16.0 pour sa partie simple :
métadonnées réelles de la caméra (QW7), lecture du DNG, écriture FITS, master dark par série avec carte des pixels
chauds (IQ-6, points 1 et 2), empilement en lumière linéaire avec rejet sigma (IQ-7). Lot 2b réalisé en version
1.17.0 : recalage de l'empilement sur les étoiles (IQ-7 ; étoiles cherchées sur l'image pleine résolution binnée 2x2
et non sur la vignette, qui en montre trop peu ; appariement par triangles, transformation rigide estimée par RANSAC,
zone sol jamais tournée), binning 2x2 des RAW par couleur (IQ-6, annexe C 4.3 ; utilisé pour l'aperçu galerie des DNG
sans vignette) et script Siril par nuit `calibrer.ssf` (IQ-6, point 3, syntaxe vérifiée dans le code source de
Siril 1.4.0). Le modèle rigide a été retenu sur calcul (fisheye équidistant supposé) : une affine ne réduit le résidu
que de 5 à 10 %. Restent : la soustraction du master dark sur la vignette d'analyse et le passage de la détection
d'aurore sur le RAW binné (IQ-6, point 4), un modèle de recalage sur la sphère avec calibration de l'objectif si les
résidus réels l'exigent. Points à valider sur le matériel : format réel du DNG, clés de métadonnées, temps de calcul du
master dark sur Pi 4, nombre d'étoiles détectées et résidus du recalage avec l'objectif 2,7 mm, temps de recalage et
mémoire sur Pi 4, rendu des aurores mobiles dans l'image empilée.

| Lot | Contenu | Matériel | Effort |
|---|---|---|---|
| **1. Corrections rapides** | exposition de départ reprise de l'aperçu, verrou d'exposition corrigé, hystérésis et confiance de la détection, plafond du gain analogique, réglages affichés mais inutilisés (annexe A, section 0) | aucun | petit |
| **2. Traitement RAW** | lecture du DNG, binning 2x2, dark maître par température capteur, empilement aligné sur les étoiles avec rejet sigma | aucun | moyen |
| **3. Détection v2** | portes Soleil et Lune (éphémérides), ciel clair par comptage d'étoiles, méthode Yamauchi et Brändström 2023, contexte NOAA (Kp, Bz, OVATION) quand internet est là, collecte de vignettes étiquetées pour un futur petit réseau de neurones | aucun | moyen à grand |
| **4. Énergie** | protocole de mesure, `rpicam-still` persistant ou libcamera direct, sortie JPEG réduite, extinction le jour et réveil au crépuscule sur Pi 5 | testeur USB-C | moyen |
| **5. Installation et ergonomie** | aide à la mise au point (netteté des étoiles), horizon touché sur l'aperçu, QR code Wi-Fi, choix du lieu et horaires calculés, autotest avec vraie pose ; viser 12 actions au lieu de 18 | aucun | moyen |
| **6. Multi-caméras** | adaptateur pour caméra astronomique USB (SDK ZWO ou INDI), puis gphoto2 pour un boîtier plein format | une caméra USB | grand |
| **7. Réseau scientifique** | GPS et PPS, FITS, astrométrie, synchronisation, collecte (section 4) | module GPS | grand |
| **8. Matériel** | fisheye f/1,4, module sans filtre IR avec filtre 420 à 685 nm, chauffage anti-givre de 1 à 2 W régulé, boîtier étanche, batterie au froid | à chiffrer | variable |

## 6. Paliers matériels (à chiffrer)

| Palier | Caméra | Optique | Remarque |
|---|---|---|---|
| Grand public | HQ Camera actuelle | fisheye f/1,4 en monture C | profite à plein des lots 1 à 5 |
| Avancé | ZWO ASI585MC (prix public relevé par le porteur du projet : 579 € TTC) | fisheye adapté au capteur 1/1,2" | lot 6 nécessaire |
| Association | ZWO ASI294MC Pro, ASI6200 avec filtres 557,7 et 630 nm, ou Sony A7S II en caisson chauffé | selon le capteur | lots 6 et 7 ; puissance de calcul à valider pour les gros capteurs |

---

## Annexe A : Audit d'amélioration : qualité d'image, détection, énergie, installation

Dépôt audité : commit `7a9ea29` (version 1.14.0). Lecture seule, aucun fichier modifié.
Toutes les références `fichier:ligne` renvoient à ce commit. Les comportements cités sont ceux du code ; ce qui
dépend du matériel (temps de capture réel, consommation, comportement exact de `rpicam-still`) est marqué
« à mesurer » et renvoie aux tâches de validation de `docs/PLAN_ACTION.md`.

Barème : **Effort** S (moins d'une demi-journée), M (1 à 2 jours), L (plus). **Risque** : faible / moyen / élevé
(risque de régression ou de comportement différent sur le terrain).

---

### 0. Gains rapides (fort gain, petit effort)

| # | Constat (code) | Correction proposée | Gain estimé | Effort | Risque |
|---|---|---|---|---|---|
| QW1 | La nuit démarre toujours à ISO min / pose min (`exposure.rs:52`), la calibration fait 3 images avec `update()` limité à +15 % par image (`orchestrator.rs:356-375`), puis le mode *Toute la nuit* passe en RUN où la hausse est limitée à +3 % par image. Le réglage trouvé par l'aperçu (`state.last_preview`, `api.rs:519`) n'est jamais réutilisé par la nuit. | Initialiser l'exposition de la nuit avec, dans l'ordre : la dernière preview, sinon la moyenne de la nuit précédente (`dark_reminder.json` contient déjà ISO et pose moyens), sinon le milieu géométrique de la plage (comme `capture_preview`, `api.rs:478`). Remplacer `update()` par `jump()` pendant la calibration (jusqu'à 6 images, arrêt dès convergence comme dans `capture_preview`). | Avec les valeurs par défaut (départ ISO 100 / 1 s, cible type ISO 800 / 8 s soit ×64), la calibration n'atteint que ×1,52, puis il faut de l'ordre de 100 à 150 images à +3 % pour rejoindre la cible : la première partie de chaque nuit est sous-exposée. | S | faible |
| QW2 | `lock_in_run` gèle l'exposition dès l'entrée en RUN (`orchestrator.rs:1059`). En mode *Toute la nuit* (défaut, `config.rs:539`), RUN commence juste après les 3 images de calibration : avec le verrou, **toute la nuit est figée vers ISO 100 / 1,5 s** (1 s × 1,15³). `docs/GUIDE_PHOTO.md` § 2 recommande pourtant d'activer ce verrou. Le test `exposure_lock_keeps_the_timelapse_flicker_free` (`tests/simulation_tests.rs:334`) vérifie que l'exposition ne bouge pas, pas qu'elle est correcte. | Ne verrouiller qu'après convergence (erreur log inférieure à 0,1 sur 3 images de suite) ou après QW1 ; ajouter un test qui vérifie la valeur verrouillée sur un ciel simulé dont la cible est connue. | Supprime une nuit entière sous-exposée de 5 à 6 IL quand l'utilisateur suit le guide. | S | faible |
| QW3 | Hystérésis sans effet : le score ne vaut que 0 ou plus de 1, car il n'est calculé que si `green_score > green_threshold` (`detection.rs:183-185`, idem rouge et violet) ; avec `hysteresis_off = 0.7` (`config.rs:542`), le seuil de maintien (`detection.rs:207`) est équivalent au seuil d'entrée. `confidence` vaut toujours 1,0 quand il y a détection (`detection.rs:215-216`). | Calculer un score continu non seuillé (par exemple `green_score / green_threshold` sans condition) et appliquer `hysteresis_on/off` sur ce score ; garder la condition de surface séparément. | Moins de clignotement `_AURORA` / non-`_AURORA`, `JpgAuroraRaw` plus stable, `confidence` exploitable dans la galerie. | S | faible (seuils à recaler, V4) |
| QW4 | Réglages affichés mais inutilisés : `variation_threshold` (`config.rs:136`) et `ev_step_max` (`config.rs:87`) ne sont lus nulle part hors validation et interface (`settings_advanced.html:341-346`, `412-415`). | Les retirer de l'interface (garder la désérialisation pour compatibilité) ou les brancher (QW3 / D-6 pour la variation). | L'utilisateur ne règle plus un curseur sans effet. | S | faible |
| QW5 | En surveillance, chaque photo de contrôle est une capture JPEG pleine résolution 12 Mpx (`camera_rpi.rs:73-112` : aucun `--width`/`--height`) alors que seule la vignette 320×240 est analysée. `docs/ENERGIE_ET_FIABILITE.md` parle pourtant d'une « petite photo JPEG par minute ». | En phase WATCH, demander une sortie réduite (par exemple `--width 1014 --height 760`, vignette conservée) ; à mesurer : mode capteur binné 2028×1520 (meilleur rapport signal/bruit par pixel) ou plein format réduit par l'ISP. | Supprime l'encodage logiciel d'un JPEG 12 Mpx toutes les minutes et raccourcit le temps caméra allumée ; gain à mesurer (E1). | S | faible (seuils de détection calculés sur la vignette, inchangés) |
| QW6 | Mode par défaut `RawDng` (`config.rs:551`) : le JPEG 12 Mpx est quand même produit et encodé à chaque image (`rpicam-still -o x.jpg --raw`), puis jeté (`save_frame`, `orchestrator.rs:735` : `saves_jpg()` faux). | En `RawDng`, forcer le mode capteur plein format pour le RAW et une sortie traitée réduite : `--mode 4056:3040:12:P --width 1014 --height 760` (à valider sur Raspberry Pi OS, comme V2 : le DNG doit rester en 4056×3040). | Un encodage JPEG 12 Mpx de moins par image en mode par défaut ; CPU, chaleur près du capteur et énergie (à mesurer, E1). | S | moyen (vérifier la taille du DNG produit) |
| QW7 | Température « capteur » = température du processeur (`sys.rs:71-76`, utilisée par `dark_reminder.rs:36`). Q1 (métadonnées) est encore ouvert. | Ajouter `--metadata <fichier> --metadata-format json` à `rpicam-still` et lire `SensorTemperature`, `AnalogueGain`, `DigitalGain`, `ExposureTime` réellement appliqués ; les écrire dans `event.jsonl` et dans le rappel des darks. | Darks appariés sur la vraie température ; détection du gain numérique (QW8) ; diagnostic. | S | faible |
| QW8 | ISO converti en gain sans plafond (`exposure.rs:204` : ISO/100), `iso_max = 3200` par défaut (`config.rs:518`) soit `--gain 32.0`. Au-delà du gain analogique maximal de l'IMX477 (environ 22, à confirmer par V10 et QW7), le surplus est numérique : il éclaircit le JPEG mesuré mais pas le DNG. | Plafonner `iso_max` effectif au gain analogique maximal lu dans les métadonnées (QW7), ou à 2200 par défaut pour les formats RAW. | DNG mieux exposés à réglage égal (la mesure n'est plus faussée par un gain qui n'existe pas dans le RAW). | S | faible |
| QW9 | Aucune aide pour rejoindre le Wi-Fi : SSID et mot de passe tapés à la main (`GUIDE_DEMARRAGE.md` § 3). | QR code Wi-Fi standard (`WIFI:T:WPA;S:Aurion;P:...;;`) imprimé dans le guide et affiché par l'interface après un changement de mot de passe (à capturer pour un second téléphone). | Une saisie de mot de passe en moins par téléphone, moins d'erreurs dans le froid. | S | faible |
| QW10 | Messages d'erreur caméra bruts : `stderr` de `rpicam-still` recopié tel quel (`camera_rpi.rs:147-152`, `199-207`). | Table de traduction des erreurs fréquentes (aucune caméra, caméra occupée, délai dépassé, mémoire) en consignes, comme le fait déjà `get_preflight`. | Diagnostic sans terminal. | S | faible |

---

### 1. Qualité d'image et bruit

#### 1.1 Réponses directes

| Question | Réponse (code) |
|---|---|
| Traitement sur le JPEG 8 bits ou sur le RAW 12 bits ? | **Uniquement sur du 8 bits.** Analyse (exposition, détection) sur la vignette EXIF 320×240 qualité 70 du JPEG traité par l'ISP (`camera_rpi.rs:98`, `jpeg.rs:179-192`). Pixels chauds et empilement sur le JPEG pleine résolution décodé en RGB 8 bits (`orchestrator.rs:809-850`). Le DNG n'est jamais lu ni décodé : `capture_raw` renvoie `data: Vec::new()` (`camera_rpi.rs:216-228`). |
| Darks soustraits (master dark), flats, bias ? | **Non.** Les darks sont des DNG enregistrés tels quels dans `darks/` (`api.rs:746-843`) pour un post-traitement externe (Siril, Sequator). Aucun master dark, aucune carte de pixels chauds, aucun flat ni bias dans le code. Q6 (carte des pixels chauds) est ouvert. |
| Empilement aligné (les étoiles tournent) ? | **Non.** `Stacker` additionne pixel à pixel (`denoise.rs:114-129`), rejet du min et du max par pixel dès 4 images (`denoise.rs:132-149`). Aucun recalage. Désactivé par défaut (`stack_frames: 0`). |
| Débruitage ISP utilisé sur le RAW ? | **Non, et c'est voulu** : `--denoise cdn_hq` (`camera_rpi.rs:95`) ne s'applique qu'au JPEG ; le DNG est brut. Les darks sont pris avec `--denoise off` (`api.rs:800`), l'aperçu sans option `--denoise` (`api.rs:455-457`, donc valeur par défaut de l'outil). |
| Arguments `rpicam-still` | `--nopreview -o <jpg> --shutter <µs> --gain <ISO/100> --awb daylight --ev 0 --denoise cdn_hq --thumb 320:240:70 [--raw] (-t 100 | --immediate)` (`camera_rpi.rs:79-111`). Gain et pose fixés à chaque image : le contrôle automatique de l'outil est neutralisé, l'exposition est pilotée par Aurion. Aucun `--width/--height`, `--mode`, `--metadata`, `--tuning-file`. |
| Gain analogique ou numérique ? | Un seul `--gain` dérivé de l'ISO (`exposure.rs:204`). L'outil répartit lui-même analogique puis numérique : au-delà du maximum analogique, le surplus est numérique (voir QW8). |
| Capteur lu deux fois (JPEG + RAW) ? | **Non.** `capture_raw_and_jpg` fait un seul appel `rpicam-still --raw` et relit le JPEG produit à côté du DNG (`camera_rpi.rs:231-260`). En surveillance, aucun RAW (`captures_raw_now`, `orchestrator.rs:1222`). |
| Exposition automatique adaptée aux aurores ? | Mesure P80 de la luminance de la zone ciel sur la vignette sRGB, cible 60/255, rejet au-dessus de 250 (`config.rs:527-530`, `exposure.rs:148-169`). Lissage exponentiel en log, +15 %/-20 % par image en surveillance/calibration, +3 %/-5 % en capture (`exposure.rs:97-131`). Pose d'abord, ISO ensuite (`exposure.rs:172-186`). Pose max 30 s, ISO max 3200 par défaut (`config.rs:517-520`). Défauts : QW1, QW2, QW8, IQ-2 à IQ-4 ci-dessous. |

#### 1.2 Constats détaillés

**IQ-1. Démarrage d'exposition et verrou** : voir QW1 et QW2 (gains rapides). C'est le défaut de qualité le plus
coûteux trouvé : il touche chaque nuit en mode par défaut.

**IQ-2. Pose d'abord dans les deux sens** (`exposure.rs:172-186`)
- Actuel : `apply_multiplier` modifie toujours la pose en premier, l'ISO seulement quand la pose bute sur une borne,
  pour éclaircir comme pour assombrir.
- Problème : quand le ciel s'éclaircit (lune qui se lève, aurore forte), la pose baisse et l'ISO reste haut. À
  exposition égale, une pose plus longue à ISO plus bas collecte plus de photons (meilleur rapport signal/bruit sur un
  ciel limité par le bruit de photons).
- Proposition : asymétrique. Éclaircir : pose jusqu'au plafond, puis ISO. Assombrir : ISO jusqu'au plancher, puis pose.
- Gain : jusqu'à 1 IL de bruit en moins dans les nuits lunaires ou de forte aurore. Effort S. Risque faible (tests
  existants à adapter : `test_bright_image_decreases_exposure`).

**IQ-3. Mesure gamma traitée comme linéaire** (`exposure.rs:114`, `exposure.rs:141`)
- Actuel : l'erreur est `ln(cible / mesure)` et `jump()` multiplie par `cible / mesure`, sur des valeurs sRGB 8 bits.
- Problème : le JPEG est encodé avec une courbe gamma (environ 2,2) : une image à 30 au lieu de 60 demande environ
  ×4,6 de lumière, `jump()` n'applique que ×2. Convergence lente de l'aperçu (jusqu'à 4 prises, `api.rs:484`) et de la
  nuit.
- Proposition : table de linéarisation sRGB (256 entrées) appliquée au percentile avant le calcul du rapport.
- Gain : aperçu en 1 à 2 prises au lieu de 3 à 4 (chaque prise dure 3 fois la pose environ, voir P-3). Effort S. Risque
  faible (constantes `alpha` à revoir).

**IQ-4. Programme d'exposition inadapté aux aurores rapides** (`config.rs:520`, `exposure.rs:172`)
- Actuel : pose max 30 s par défaut et pose prioritaire. `docs/GUIDE_PHOTO.md` § 1 conseille 2 à 8 s pour des aurores
  rapides. `check_star_trail_rule` (`api.rs:93`) ne vérifie que les étoiles (règle des 500 avec facteur 5,6).
- Problème : une aurore active filée sur 30 s perd ses rayons ; le programme ne tient pas compte de la détection.
- Proposition : programme « priorité aurore » : tant que le score d'aurore dépasse un seuil (score continu de QW3),
  plafond de pose dynamique 4 à 8 s et montée en ISO jusqu'au maximum analogique (QW8) ; retour progressif au plafond
  long quand l'aurore faiblit. Pour les étoiles, remplacer la règle des 500 par la règle NPF (plus juste pour des
  pixels de 1,55 µm).
- Gain : structures d'aurore nettes dans les pics, sans perdre le ciel profond hors aurore. Effort M. Risque moyen
  (scintillement de timelapse : le changement doit rester borné par image).

**IQ-5. Rampe crépusculaire absente** (Q3 ouvert)
- Actuel : la nuit démarre à heure fixe (21:00 par défaut, `config.rs:559`), la calibration part du minimum.
- Proposition : coupler QW1 (départ au bon réglage) et D-5 (altitude du Soleil) : démarrer au crépuscule nautique et
  laisser l'exposition suivre avec un taux limité, verrou seulement une fois le Soleil sous -15°.
- Effort M. Risque faible.

**IQ-6. Pas de chaîne RAW : master dark, carte des pixels chauds** (Q6 ouvert)
- Actuel : darks enregistrés, rien n'est calculé (`api.rs:746-843`). Correction des pixels chauds sur le JPEG
  dématricé et débruité par l'ISP (`denoise.rs:29-83`), où un pixel chaud a déjà « coulé » sur ses voisins par le
  dématriçage, ce qui le fait ressembler à une petite étoile que le filtre protège (`denoise.rs:73-76`).
- Proposition :
  1. Bibliothèque de master darks clés par (gain, pose, tranche de 5 °C de `SensorTemperature`, QW7) : médiane pixel à
     pixel de la série en 16 bits, écrite en DNG `darks/master_ISO800_8.0s_T35.dng`.
  2. Carte des pixels chauds dérivée du master dark (pixel au-dessus de médiane + k·σ), fichier texte ou masque
     `hotpixels_ISO800_8s.pgm`, réutilisable par Siril et par Aurion.
  3. Script Siril généré par nuit (`sessions/<nuit>/calibrer.ssf`) pointant vers le bon master dark : la
     calibration devient un double clic au retour.
  4. Option : appliquer le master dark à la vignette d'analyse (soustraction en domaine linéaire) pour la détection.
  Lecture DNG en Rust pur possible (crate `tiff` ou `rawloader`), sans casser le binaire statique musl.
- Gain : pixels chauds et signal thermique retirés de façon fiable, sans retoucher les originaux ; prépare la détection
  sur données linéaires. Effort M (points 1 à 3), L avec le point 4. Risque faible (les DNG d'origine ne sont pas
  modifiés).

**IQ-7. Empilement non aligné, rejet min/max** (`denoise.rs:85-157`, `orchestrator.rs:770-803`)
- Actuel : moyenne de N JPEG 8 bits sans recalage ; dès 4 images, le min et le max de chaque pixel sont retirés.
- Problèmes :
  - Avec l'objectif 2,7 mm (README, configuration testée), un pixel couvre environ 1,55 µm / 2,7 mm ≈ 118" ;
    le ciel tourne de 15"/s à l'équateur céleste : 8 images de 30 s (240 s) donnent un filé allant jusqu'à environ
    30 pixels (moins près du pôle). Les étoiles du `_STACKn.jpg` sont des traits.
  - Le rejet min/max jette toujours 2 images sur N : pour N = 4, il n'en reste que 2 (gain √2 au lieu de 2).
  - Calcul en 8 bits après courbe gamma : la moyenne n'est pas une moyenne de lumière.
- Proposition : soit garder l'empilement embarqué comme simple aperçu et le documenter (le vrai empilement avec
  recalage se fait dans Sequator ou Siril), soit : rejet sigma (moyenne et variance par Welford, rejet à 2,5σ) en
  domaine linéaire, et recalage par rotation autour du pôle estimée à partir des centroïdes d'étoiles de la vignette
  (appariement de triangles). Masquer le sol (zone hors ROI) pour qu'il ne soit pas tourné.
- Gain : aperçu empilé net ; effort L pour le recalage, S pour sigma + linéarisation. Priorité basse (option désactivée
  par défaut). Risque moyen (mémoire : 144 Mo par 12 Mpx en `u16 + 2×u8`, plus avec la variance).

**IQ-8. Vignette d'analyse en qualité 70** (`camera_rpi.rs:98`, `api.rs:457`)
- Actuel : `--thumb 320:240:70`, JPEG 4:2:0 : la chrominance est sous-échantillonnée et quantifiée fort, alors que la
  détection repose entièrement sur des écarts de couleur de 8 à 15 niveaux.
- Proposition : `--thumb 320:240:90` (coût négligeable, quelques Ko par image).
- Gain : moins de bruit de quantification sur les scores de couleur. Effort S. Risque faible (V4 de toute façon).

**IQ-9. Aperçu, darks et nuit ne partagent pas la même commande** (`api.rs:444-465`, `api.rs:796-800`,
`camera_rpi.rs:73-112`)
- Actuel : trois lignes de commande `rpicam-still` écrites séparément ; l'aperçu n'a pas `--denoise`, la nuit a
  `cdn_hq` ; les darks forcent `--awb daylight` quel que soit le réglage.
- Proposition : une seule fonction de construction dans `core` (paramètres : RAW oui/non, sortie réduite oui/non,
  débruitage) utilisée par les trois. L'aperçu reflète alors exactement la nuit.
- Gain : ce que l'utilisateur voit au cadrage est ce qu'il aura ; moins de code dupliqué. Effort S. Risque faible.

**IQ-10. Fichier de réglage capteur** (optionnel)
- Proposition : tester `--tuning-file imx477_scientific.json` (fourni avec libcamera) pour des couleurs plus neutres
  et une matrice couleur plus stable, utile à une détection fondée sur la chromaticité (D-2). À comparer sur une nuit.
- Effort S. Risque moyen (rendu des JPEG différent).

**IQ-11. Thermique**
- Le CPU, voisin du capteur, est la principale source de chaleur pilotable. Chaque encodage JPEG 12 Mpx évité
  (QW5, QW6) et la suppression du redémarrage de la caméra à chaque image (P-3) réduisent la chaleur, donc le courant
  d'obscurité. À mesurer avec `SensorTemperature` (QW7) au lieu de la température CPU.

---

### 2. Détection des aurores

#### 2.1 Algorithme actuel (exactement ce que fait le code)

Entrée : vignette EXIF RGB 8 bits (en pratique 320×240) du JPEG traité par l'ISP, balance « lumière du jour », débruitage
`cdn_hq` (`orchestrator.rs:1054-1066`, `jpeg.rs:179`). Appelée à chaque image, en surveillance comme en capture.

1. **Zone analysée** : les `roi_top_percent` % du haut de l'image (65 % par défaut, borné 50 à 99,
   `detection.rs:94-96`). Hypothèse : horizon en bas, image non inclinée.
2. **Masque lune** (`detection.rs:237-320`) : grille de 16×12 cases sur la zone ; une case est masquée si sa
   luminance moyenne `(r+g+b)/3` dépasse 240 ; dilatation d'une case.
3. **Scores par pixel non masqué** (`detection.rs:121-159`) : pixels de luminance moyenne inférieure à 30
   (`luminosity_threshold`) comptés mais sans score ; sinon `gs = g - (r+b)/2`, `rs = r - (g+b)/2`,
   `bs = b - (r+g)/2`. Sommes des parties positives ; surfaces : `gs > 15`, `rs > 10`, violet si `rs > 5` et `bs > 4`.
4. **Scores globaux** (`detection.rs:165-174`) : moyenne des parties positives sur **tous** les pixels de la zone (y
   compris les pixels sombres), surface en % de la zone.
5. **Couleur dominante** (`detection.rs:177-199`) : une couleur n'est candidate que si score moyen > seuil et
   surface > 1 % ; score = score moyen / seuil (donc toujours supérieur à 1 quand candidat).
6. **Étalement** (`detection.rs:323-364`) : grille 4×4 de la zone ; une case est « lumineuse » si la moyenne de son
   **canal vert** dépasse 60 (constante en dur, `detection.rs:338`) ; il faut au moins 3 cases.
7. **Décision** (`detection.rs:205-212`) : hystérésis sur le score (QW3 : sans effet) et étalement obligatoire.
8. **Confirmation** (mode *Aurores seulement*) : `consecutive_required = 2` détections de suite
   (`orchestrator.rs:1176-1185`), une image par `watch_interval_secs = 60` s plus la durée de capture
   (`orchestrator.rs:619`).

Gestion des cas :
- **Lune** : masque par cases moyennes > 240 seulement ; aucune éphéméride.
- **Lumières de ville, nuages éclairés** : aucun traitement spécifique.
- **Crépuscule** : aucun ; seule la plage horaire fixe (21:00 à 06:00) protège.
- **Airglow** (raie verte 557,7 nm, la même que l'aurore) : aucun traitement.
- **Pollution lumineuse verte** (LED) : aucun traitement.

Jeu de données : **aucune image réelle**. `testdata/watch_sequences/` ne contient qu'un `.gitkeep` ; les tests
utilisent des images synthétiques uniformes (`detection.rs:385-463`) et la caméra simulée
(`adapters/pc/camera_mock.rs:135-187`). Le test `test_hysteresis_stickiness` (`detection.rs:503-520`) ne contient
aucune assertion.

#### 2.2 Défauts

| ID | Constat | Conséquence | Effort | Risque |
|---|---|---|---|---|
| D-1 | Hystérésis et confiance sans effet (QW3). | Bascule détecté / non détecté à chaque image proche du seuil. | S | faible |
| D-2 | Écarts absolus `g - (r+b)/2` sur sRGB 8 bits : ils dépendent de l'exposition, du gamma et de la balance. La cible d'exposition (P80 = 60) fixe de fait l'échelle des scores. | Les mêmes seuils ne valent pas pour une nuit lunaire, un ciel pollué et un ciel noir ; un changement d'exposition change le score. | M | moyen |
| D-3 | Étalement : vert moyen > 60 en dur, valeur égale à la cible d'exposition. | Sur un ciel bien exposé, le test est une pièce lancée ; il ne dépend pas de la couleur (un ciel gris lumineux passe). | S | faible |
| D-4 | Masque lune : la lune (environ 0,5°) fait 1 à 2 pixels sur une vignette de 320 px avec un objectif de 99° à 140° ; une case fait environ 20×13 pixels : sa moyenne ne dépasse jamais 240 sauf grand halo. | Masque quasi jamais actif ; le ciel lunaire (bleu en balance lumière du jour) peut déclencher le violet. | S | faible |
| D-5 | Aucune notion d'altitude du Soleil ni de la Lune. | Crépuscule bleu ou violet, aube : faux positifs violets ; en mars à 69° N, le crépuscule nautique dure longtemps. | M | faible |
| D-6 | Dômes de pollution orange (sodium) et nuages éclairés près de l'horizon : `rs > 10` dans le bas de la zone. | Faux positifs « aurore rouge ». | M | moyen |
| D-7 | Airglow vert : même raie que l'aurore, couleur seule insuffisante. | Faux positifs sur les longues poses en site très noir. | M | moyen |
| D-8 | Score moyenné sur toute la zone (pixels sombres inclus) : une bande à `gs = 40` couvrant 20 % de la zone donne un score de 8, sous le seuil de 15. | Aurores faibles ou basses sur l'horizon manquées (le cas le plus courant hors de l'ovale). | S | moyen |
| D-9 | En mode *Aurores seulement*, les images de surveillance qui ont servi à confirmer ne sont pas gardées ; la capture commence au tour suivant. | Début d'aurore perdu (au moins 2 à 3 min) ; aucun moyen de constituer un jeu de données de surveillance. | S | faible |

#### 2.3 Approche proposée (par étapes, toutes sur la vignette : quelques ms par image)

**Étape A (S à M) : corriger et rendre mesurable**
- D-1, D-3, D-8 : score continu ; score de surface fondé sur les pixels (ou cases) en excès plutôt que sur la moyenne
  globale ; seuil d'étalement relatif au fond de ciel, pas en dur.
- D-9 : **tampon de pré-déclenchement** : garder en `/tmp` les K dernières images de surveillance (JPEG) et les
  écrire sur la clé à la confirmation. Enregistrer aussi les vignettes de surveillance (environ 20 Ko, une par minute,
  soit environ 12 Mo par nuit) et leurs scores dans `event.jsonl` : c'est la matière première du jeu de données.
- Écran *Photos* : étiquetage en un geste (aurore / pas d'aurore / nuages / lune / lumière parasite), écrit dans
  `sessions/<nuit>/labels.csv` sur la clé.

**Étape B (M) : modèle de ciel et chromaticité**
1. Linéariser la vignette (table sRGB inverse) ; option : soustraire le master dark réduit (IQ-6).
2. Chromaticité par case (grille 16×12) : `c_g = G/(R+G+B)`, `c_r`, et luminance linéaire.
3. **Modèle de ciel glissant** : médiane par case de `c_g` et de la luminance sur les N dernières images sans aurore
   (N = 10 à 20), avec sa dispersion σ. Résidus `Δc_g`, `Δlum` par case. Le dôme de pollution, l'airglow stable et le
   gradient d'horizon sont absorbés dans le modèle.
4. Masque d'étoiles et de pixels saturés (pixels au-dessus de 250, composantes connexes) avant le calcul ; masque
   lune géométrique (étape C) au lieu de la moyenne de case.
5. Score de surface : fraction de cases avec `Δc_g > k·σ` et `Δlum > 0`.

**Étape C (M) : éphémérides et contexte**
- Position du Soleil et de la Lune (algorithme de faible précision, quelques dizaines de lignes, aucune dépendance) à
  partir de la latitude/longitude (voir I-5) et de l'heure, déjà fiable grâce au téléphone ou à la RTC.
- Porte crépusculaire : détection désactivée (ou seuils relevés) tant que le Soleil est au-dessus de -12°.
- Lune : altitude, phase et azimut. Seuils relevés selon l'éclairement lunaire ; masque centré sur la position
  projetée quand l'orientation est connue (sinon, masque des composantes saturées).
- Même calcul pour la plage horaire automatique (I-5) et la rampe crépusculaire (IQ-5).

**Étape D (M) : temps et structure** (ce qui sépare l'aurore de l'airglow et des nuages)
- Variation temporelle : énergie de la différence d'image à image du résidu vert, normalisée par le bruit (c'est à
  quoi servait sans doute `variation_threshold`, aujourd'hui inutilisé, QW4). L'airglow varie en dizaines de minutes,
  l'aurore active en secondes à minutes.
- Structure : moments d'ordre 2 du masque vert (allongement, orientation) : arcs allongés est-ouest, rayons verticaux ;
  un voile uniforme (airglow, nuage éclairé) a un allongement faible.
- Décision : combinaison logistique (surface, chromaticité, variation, allongement, porte solaire), hystérésis sur la
  probabilité, paramètres appris sur le jeu de données de l'étape A.

**Étape E (option) : météo spatiale**
- Quand une connexion existe (session de mise à jour par le partage du téléphone, `online_update`), télécharger la
  prévision Kp sur 3 jours de NOAA SWPC (fichier JSON de petite taille) et, si possible, la probabilité OVATION au
  point de la caméra ; la garder sur la carte SD avec sa date.
- Usage : a priori sur les seuils (Kp élevé : seuils abaissés, mode *Aurores seulement* plus sensible) et conseil sur
  l'accueil (« Kp 5 prévu ce soir : mode Toute la nuit conseillé »). Jamais bloquant : sur le terrain, il n'y a
  généralement pas d'internet. Hypothèse à vérifier : le navigateur du téléphone connecté au Wi-Fi Aurion (sans
  internet) peut-il encore joindre NOAA par les données mobiles et transmettre la prévision au Pi ? Le comportement
  varie selon iOS et Android.

**Étape F (L) : classifieur léger**
- Petit CNN (3 à 4 couches de convolution, entrée 96×72 ou 128×96, quelques dizaines de milliers de paramètres) ou
  MobileNetV3-small, exécuté en Rust pur avec `tract-onnx` (garde le binaire statique musl, pas de TFLite à embarquer).
  Ordre de grandeur sur Cortex-A72 : quelques millisecondes par vignette (à mesurer avec `aurion bench`).
- Jeu de données : **aucun dans le dépôt** (`testdata/watch_sequences/` vide). Sources : vignettes et étiquettes de
  l'étape A (domaine exact de la caméra), complétées pour le pré-entraînement par des jeux publics d'imageurs
  plein ciel, par exemple le jeu OATH (Oslo Auroral THEMIS, environ 5 800 images étiquetées : arc, diffus, discret,
  nuageux, lune, ciel clair). Écart de domaine important (imageurs monochromes filtrés à 557,7 nm, optique
  différente) : utile pour la structure, pas pour la couleur. Visée réaliste : quelques centaines de nuits-images
  étiquetées par l'utilisateur avant de remplacer la règle.
- Garder la règle (étapes B à D) comme repli et comme porte (crépuscule, lune).

---

### 3. Énergie et autonomie

#### 3.1 Profil actuel (code et scripts)

| Poste | État | Où |
|---|---|---|
| Processus caméra | un `rpicam-still` lancé **à chaque image** (initialisation libcamera, démarrage capteur, prise, encodage, sortie) | `camera_rpi.rs:79-145` |
| Durée d'une prise | d'après le commentaire du code, avec une pose longue libcamera a besoin de plusieurs images de la durée demandée avant la prise ; délai d'attente 3 × pose + 20 s | `camera_rpi.rs:114-120` ; `capture_ms` déjà journalisé dans `event.jsonl` (`orchestrator.rs:470`) |
| Encodage | JPEG 12 Mpx produit à chaque image, même en surveillance et même en `RawDng` (jeté) | QW5, QW6 |
| Entre deux images | rien (sommeil Tokio) ; RUN dos à dos : pause minimale 1 s (`orchestrator.rs:1242-1250`) ; surveillance : 60 s après la prise | `orchestrator.rs:619` |
| Fréquence CPU | `powersave` en surveillance, `ondemand` en capture | `aurion-helper:404-418` |
| Wi-Fi | point d'accès coupé 15 s après « Lancer la nuit », radio éteinte (`nmcli radio wifi off`) | `orchestrator.rs:157-167`, `aurion-helper` `cmd_ap_stop` |
| Ethernet | lien coupé la nuit sans câble | `aurion-helper:421-427` |
| Bluetooth, audio, LED | coupés dans `config.txt` (`disable-bt`, `audio=off`, LED `trigger=none`) ; services bluetooth, hciuart, ModemManager, triggerhappy désactivés | `install.sh:420-444`, `install.sh:400-412` |
| HDMI | **non désactivé** (documenté comme « non appliqué faute de mesure », `ENERGIE_ET_FIABILITE.md`) | |
| Fichiers temporaires | `/tmp` en RAM (tmpfs 200 Mo), `PrivateTmp=yes` | `install.sh:311-315`, `aurion.service:28` |
| Journaux | en RAM (journald volatile 50 Mo) | `install.sh:291-294` |
| Réveils de fond | `aurion-power-watch` toutes les 20 s (lance `vcgencmd`), `sync` toutes les 2 min, watchdog systemd | `install.sh:320-375` |
| Écritures clé | chaque photo : fichier temporaire, `fsync`, renommage, `fsync` du dossier (DNG de 12 à 24 Mo selon les mesures du dépôt) | `ENERGIE_ET_FIABILITE.md` § 2 |
| Mesure disponible | 14 000 mAh par nuit d'hiver avec une batterie de 20 000 mAh (`GUIDE_EXPEDITION.md` § 2), soit environ 4 à 5 W moyens sur 9 à 10 h en tenant compte des pertes de conversion (estimation) | |

#### 3.2 Constats et propositions

**P-1. Surveillance en pleine résolution** : QW5. Gain à mesurer ; c'est le mode *Aurores seulement*, celui qui doit
consommer le moins.

**P-2. JPEG inutile en mode RAW seul** : QW6.

**P-3. Un processus caméra par image** (`camera_rpi.rs:79-145`)
- Problème : à chaque image, démarrage complet de la caméra et images préliminaires à la pose demandée. En RUN dos à
  dos, si `capture_ms` vaut environ 3 fois la pose (à vérifier dans `event.jsonl` d'une nuit réelle : rapport
  `capture_ms / exposure_us`), le capteur ne collecte des photons enregistrés qu'un tiers du temps : cadence du
  timelapse divisée par 3 et énergie par image multipliée d'autant.
- Propositions, par ordre d'effort :
  1. V11 : mesurer `--immediate` (déjà câblé, `camera_rpi.rs:104-106`) ; l'activer par défaut si les DNG sont
     identiques. Effort S.
  2. Processus `rpicam-still` persistant déclenché par signal (`--signal`, `-t 0`) ou `--timelapse` : la caméra
     reste configurée, plus d'initialisation par image. Le changement de pose impose peut-être un redémarrage : ne
     changer la pose que par paliers (déjà le cas avec le taux de 3 %, et nul avec le verrou). Effort M.
  3. Pipeline libcamera direct (Picamera2 en service Python séparé, ou bindings Rust `libcamera-rs`) en flux continu
     plein format à `FrameDurationLimits = pose` : rapport cyclique proche de 100 %, une image toutes les
     « pose + quelques ms ». Effort L, risque élevé (nouvelle dépendance, maintenance).
- Gain attendu : le plus important de cet audit côté énergie et cadence, à quantifier par la mesure ci-dessous.

**P-4. Fréquence CPU en capture** (E2 partiellement fait)
- Actuel : `ondemand` en capture.
- Proposition : une fois P-1 à P-3 appliqués, il reste peu de calcul ; tester `powersave` aussi en capture, ou un
  plafond `arm_freq=1200` sur Pi 4, contre `ondemand` (course au repos). Choisir par la mesure (Wh par image).
- Effort S. Risque faible.

**P-5. HDMI**
- Proposition : sur Pi 4, essayer `dtoverlay=vc4-kms-v3d,nohdmi` (ou retrait de l'overlay KMS si rien n'utilise le
  GPU) dans le bloc `config.txt` d'`install.sh:428-441`. Gain non garanti (quelques dizaines de mA au mieux) :
  n'appliquer qu'après mesure, comme le dit déjà la documentation.
- Effort S. Risque faible (aucun écran n'est utilisé sur le terrain ; garder une procédure de secours documentée).

**P-6. Réveils de fond**
- `aurion-power-watch` toutes les 20 s lance un processus `vcgencmd` ; pendant la nuit, la boucle d'Aurion pourrait
  lire l'état de sous-tension elle-même entre deux images (une lecture par image). Gain marginal ; à faire seulement
  si la mesure montre un plancher de repos sensible. Effort S. Risque faible.

**P-7. Écritures sur la clé**
- Le `fsync` par photo est justifié par la sûreté (coupures). Ne pas y toucher. Si le débit pose problème (clé lente),
  le test automatique le signale déjà (`selftest.rs:31`, 5 Mo/s).

#### 3.3 Protocole de mesure (E1)

Matériel : testeur USB-C en ligne avec enregistrement (tension, courant, énergie cumulée), entre la batterie et le Pi ;
sur Pi 5, en complément, `vcgencmd pmic_read_adc` donne les courants internes par rail.

Conditions : même Pi, même clé, bouchon sur l'objectif, pièce à température stable (puis une série dehors au froid),
Wi-Fi coupé comme la nuit. Exposition manuelle fixe (ISO min = ISO max = 800, pose min = pose max = 8 s) pour que
toutes les variantes fassent le même travail.

| Scénario | Durée | Relevés |
|---|---|---|
| Repos ARM, point d'accès allumé, aucun client | 15 min | W moyen |
| Repos, point d'accès coupé (référence plancher) | 15 min | W moyen |
| Surveillance actuelle (*Aurores seulement*, 1 image/min) | 30 min | W moyen, `capture_ms` |
| Surveillance avec sortie réduite (QW5) | 30 min | idem |
| Capture RAW seul actuelle (dos à dos) | 30 min | W moyen, images/h, `capture_ms`, température CPU et capteur |
| Capture RAW seul + JPEG réduit (QW6) | 30 min | idem |
| Capture + `--immediate` (V11) | 30 min | idem |
| Capture avec processus persistant (P-3.2) | 30 min | idem |
| Variantes gouverneur et HDMI (P-4, P-5) | 30 min chacune | idem |

Indicateurs : W moyen, **Wh par image enregistrée**, images par heure, rapport `capture_ms / pose`, température du
capteur en fin de série. Deux répétitions par scénario, ordre alterné. Résultat attendu : une ligne par variante dans
`AUDIT_CODE.md` § 5 et `GUIDE_EXPEDITION.md` § 2, et une autonomie en heures par capacité de batterie.

---

### 4. Installation et ergonomie

#### 4.1 Parcours actuel, du carton à la première nuit (image carte SD)

D'après `docs/GUIDE_DEMARRAGE.md`, `scripts/build-image.sh`, `deploy/aurion-firstboot.sh` et `index.html` :

1. Installer Raspberry Pi Imager. 2. Télécharger l'image (600 Mo). 3. Imager : modèle, image personnalisée, carte,
« Non » aux réglages, écriture. 4. Carte dans le Pi. 5. Nappe caméra (deux côtés). 6. Clé USB. 7. Alimentation.
8. Attente du premier démarrage : `install.sh` s'exécute sur le Pi (3 à 5 min, `aurion-firstboot.sh:16-17`,
`TimeoutStartSec=600`), redémarrage possible. 9. Rejoindre le Wi-Fi *Aurion* en tapant `aurora2024`. 10. Page ouverte
par le portail captif (sinon taper l'adresse). 11. Choisir un mot de passe (encadré « Protégez votre caméra »).
12. Se reconnecter avec le nouveau mot de passe. 13. La vérification automatique s'exécute à la première visite
(`index.html:314-324`, environ 1 min). 14. *Cadrer* : preview. 15. Mise au point à la main sur l'objectif, sans aide.
16. Lire la liste *Prêt pour la nuit ?*. 17. Choisir mode, durée, format. 18. *Lancer la nuit* et confirmer.

Soit environ **18 actions**, dont 3 sur ordinateur, 2 saisies de mot de passe, et une mise au point sans outil.
Points forts déjà en place : portail captif, heure et fuseau donnés par le téléphone, autotest complet
(`selftest.rs`), liste de vérification avec consignes, préparation de la clé en exFAT, réglages sauvegardés sur la clé,
reprise après coupure.

#### 4.2 Constats et propositions

| ID | Constat (code) | Proposition | Actions gagnées / gain | Effort | Risque |
|---|---|---|---|---|---|
| I-1 | Saisie manuelle du Wi-Fi (étapes 9 et 12). | QR code Wi-Fi (QW9) : dans le guide et sur un autocollant pour l'usine ; affiché par la page après changement de mot de passe. | 2 saisies supprimées | S | faible |
| I-2 | Installation complète au premier démarrage sur le Pi (étape 8) alors que les paquets sont déjà dans l'image ; l'image n'applique que `--boot-config-only` (`install.sh` `main`). | Exécuter `install.sh` en chroot pendant `build-image.sh` (compte `aurion`, fichiers, service, sudoers, durcissement) et ne garder au premier démarrage que ce qui dépend de l'appareil (empreinte, éventuellement mot de passe). | 3 à 5 min et un redémarrage en moins | M | moyen (services systemd en chroot) |
| I-3 | Aucune aide à la mise au point : aperçu plein cadre 12 Mpx envoyé au téléphone (`api.rs:521-535`), pas de loupe (U2 ouvert). | Mode « Mise au point » : boucle de poses courtes à gain élevé (1 à 2 s, gain 16) sur un recadrage central 1:1 (`--roi 0.4,0.4,0.2,0.2` avec petite sortie), calcul de la demi-largeur de flux (HFR) médiane des étoiles détectées et affichage d'un chiffre et d'une barre « meilleur jusqu'ici ». | Mise au point fiable en 1 à 2 min, sans œil exercé ; c'est la première cause de nuit ratée en astrophotographie | M | faible |
| I-4 | Zone ciel fixée par un pourcentage (`roi_top_percent`, réglage expert, aide en infobulle `settings_advanced.html:32`). L'aperçu montre déjà la zone exclue (`preview.html:184-200`). | Sur l'aperçu, toucher la ligne d'horizon : la page calcule `roi_top_percent` et l'enregistre. | Réglage expert supprimé du parcours ; détection mieux cadrée (D-6) | S | faible |
| I-5 | Plage horaire fixe 21:00 à 06:00 (`config.rs:559-560`), choix de durée à chaque nuit (étape 17) ; aucune position géographique. La géolocalisation du navigateur exige un contexte sécurisé (HTTPS), absent sur `http://192.168.4.1`. | Choisir le lieu une fois (liste de sites d'aurores et saisie de latitude/longitude, pré-rempli d'après le fuseau du téléphone) ; mode par défaut « Du crépuscule à l'aube » calculé par l'altitude du Soleil (-12°) ; sert aussi à D-5, IQ-5 et à l'étape E. | Un choix en moins chaque soir, plage juste à toute saison | M | faible |
| I-6 | Caméra « détectée » si `rpicam-hello --list-cameras` répond (`api.rs:278-293`), sans vérifier le modèle ; facteur de recadrage 5,6 en dur (`api.rs:19`) ; focale 2,7 mm par défaut en réglage expert (`config.rs:552`). | Lire le nom du capteur dans la sortie (`imx477`) et avertir sinon ; dans *Réglages photo*, choix de l'objectif dans la liste du README (2,1 / 2,7 / 3,2 / 6 mm…) qui règle la focale et le plafond de pose (NPF, IQ-4). | Un réglage expert de moins, plafond de pose juste | S | faible |
| I-7 | Erreurs caméra brutes (QW10). | Traduction en consignes. | Diagnostic sans terminal | S | faible |
| I-8 | L'autotest prend une photo de 10 ms à gain 1 (`selftest.rs:173-176`) : il ne mesure ni le temps réel d'une pose de nuit, ni les pixels chauds, ni le modèle de capteur. | Ajouter : une pose aux réglages de nuit (`shutter_max`) pour mesurer `capture_ms` réel et en déduire images/heure ; nombre de pixels chauds sur un dark court (bouchon ou nuit) ; modèle de capteur (I-6) ; gain analogique maximal lu dans les métadonnées (QW7, QW8). | Capacité en heures et cadence annoncées justes dès la première nuit (aujourd'hui hypothèse `CAPTURE_OVERHEAD_SECS = 2.0`, `api.rs:268`) | S à M | faible |
| I-9 | Aperçu transféré en pleine taille (plusieurs Mo par prise) sur le Wi-Fi 2,4 GHz. | Envoyer une version réduite (1280 px) pour l'affichage, pleine résolution seulement pour la loupe (I-3). | Aperçu affiché plus vite, moins de radio | S | faible |
| I-10 | Réglages sans effet visibles (QW4). | Les retirer. | Moins de confusion | S | faible |
| I-11 | `docs/GUIDE_PHOTO.md` § 2 recommande le verrou d'exposition, qui fige aujourd'hui la nuit sous-exposée (QW2) ; `docs/ENERGIE_ET_FIABILITE.md` parle de « petite photo » en surveillance alors que la capture est en 12 Mpx (QW5). | Corriger la documentation avec les correctifs. | Cohérence | S | faible |

Objectif réaliste après I-1, I-2, I-4, I-5, I-6 : environ **12 actions** du carton à la première nuit, aucune saisie
de mot de passe, aucune décision d'horaire, et une mise au point guidée.

---

### 5. État des éléments déjà planifiés (docs/PLAN_ACTION.md, docs/AUDIT_CODE.md)

| ID | État constaté dans le code | Lien avec cet audit |
|---|---|---|
| V1 à V17 | Validations matériel : non vérifiables par lecture du code, toutes à faire avant terrain (P0). | V2 et V11 conditionnent QW5, QW6, P-3 ; V4 conditionne QW3 et la section 2 ; V10 = QW8 |
| E1 | Ouvert (aucune mesure consignée). | Protocole § 3.3 |
| E2 | Fait en partie (profil `watch`/`capture`, `aurion-helper:414-418`) ; `arm_freq` non traité. | P-4 |
| E3 | Code présent (RTC Pi 5, `orchestrator.rs:217-260`, `rtc-wake`) ; validation V8 ouverte. | |
| E4 | Ouvert. | QW6 va dans ce sens |
| Q1 | **Ouvert** : aucun `--metadata` dans les commandes. | QW7 |
| Q2 | Fait (`dark_reminder.rs`, encadré d'accueil). Température = CPU. | QW7 |
| Q3 | **Ouvert** : pas de rampe crépusculaire. | IQ-5, D-5 |
| Q4, Q5 | **Ouverts** : intervalle fixe, pas de rafale. | D-9, IQ-4 |
| Q6 | **Ouvert** : pas de carte des pixels chauds. | IQ-6 |
| Q7 | **Ouvert** : détection par règles de couleur seulement, aucun jeu d'images réelles. | Section 2.3 |
| U1 | Ouvert (mesure terrain). | § 4.1 |
| U2 | **Ouvert** : aucune loupe ni mesure de netteté. | I-3 |
| A2 | Ouvert pour `get.sh` : vérification SHA-256 seulement (`scripts/get.sh:71-77`), pas de signature. | hors périmètre |
| C1, C2 | Faits. | |
| Dette « constructeurs à 10-13 paramètres » | Toujours présente (`detection.rs:30-41`, `exposure.rs:36-50`). | à traiter avec D-2 |

---

### 6. Ordre de réalisation proposé

1. **Avant la prochaine sortie (S, faible risque)** : QW1, QW2, QW3, QW4, QW8, IQ-2, IQ-3, IQ-8, IQ-9, QW10, I-11.
   Puis V2/V11 sur le Pi pour décider QW5, QW6 et `--immediate`.
2. **Mesure** : QW7 (métadonnées) et le protocole E1 (§ 3.3) avec les variantes QW5, QW6, `--immediate`.
3. **Prochaine version** : D-9 (tampon de pré-déclenchement et collecte des vignettes), étiquetage dans la galerie,
   I-3 (mise au point), I-4, I-6, I-8, I-1, IQ-4.
4. **Ensuite** : étapes B à D de la détection, I-5 (lieu, crépuscule automatique, IQ-5), IQ-6 (master darks et
   script Siril), P-3.2 (processus caméra persistant), I-2 (installation dans l'image).
5. **Plus tard** : étape F (classifieur sur le jeu de données collecté), étape E (Kp), P-3.3 (libcamera direct),
   IQ-7 (empilement aligné).

---

## Annexe B : Aurion : étude des capteurs pour l'imagerie d'aurores faibles

Date de recherche : 28 novembre 2026. Chaque chiffre est suivi de sa source. Quand un chiffre n'a pas pu être confirmé, il est marqué "non vérifié". Les prix sont ceux affichés par les vendeurs au moment de la recherche (souvent en USD) et changent vite.

Référence actuelle d'Aurion : Raspberry Pi HQ Camera, Sony IMX477, 4056 x 3040 px, zone image 6,287 x 4,712 mm (7,9 mm de diagonale), pixels de 1,55 µm, pose maximale 670,74 s ([doc Raspberry Pi](https://www.raspberrypi.com/documentation/accessories/camera.html)). Bruit de lecture et QE de l'IMX477 : non publiés par Sony dans les sources consultées, non vérifié.

---

### 1. La physique, expliquée simplement

#### 1.1 L'analogie des seaux sous la pluie

Imaginez que la lumière de l'aurore est une pluie très fine et régulière qui tombe sur un champ. Chaque pixel est un seau.

- **La taille du seau (surface du pixel)** : un seau deux fois plus large en diamètre a une ouverture quatre fois plus grande, donc il recueille quatre fois plus de gouttes dans le même temps. Un pixel de 2,9 µm a une surface 3,5 fois plus grande qu'un pixel de 1,55 µm ((2,9 / 1,55)² = 3,5).
- **L'entonnoir (l'objectif et son nombre f)** : l'objectif est un entonnoir posé au-dessus des seaux. Le nombre f (f/1,2, f/2, f/2,5...) dit à quel point l'entonnoir concentre la pluie. Pour une source étendue comme une aurore, l'éclairement sur le capteur varie comme 1 / N² (N = nombre f). Passer de f/2,5 à f/1,2 multiplie la pluie reçue par chaque millimètre carré de capteur par (2,5 / 1,2)² = 4,3.
- **Le temps d'exposition** : laisser les seaux dehors deux fois plus longtemps double le nombre de gouttes.
- **Le rendement quantique (QE)** : certains seaux ont un fond poreux. Un QE de 80 % signifie que 80 % des gouttes (photons) sont réellement comptées.

**Donc, par pixel : photons reçus ∝ luminance de l'aurore x (taille pixel / N)² x temps x QE.**

#### 1.2 Ce qui brouille la mesure (le bruit)

Quand on compte les gouttes, plusieurs erreurs s'ajoutent :

1. **Le bruit de photon (bruit de grenaille)** : la pluie n'est jamais parfaitement régulière. Si un seau reçoit en moyenne 100 gouttes, il en recevra en pratique entre 90 et 110 environ : l'incertitude vaut la racine carrée du nombre de gouttes (√100 = 10). C'est une loi de la nature, inévitable. Le rapport signal/bruit (SNR) vaut alors S / √S = √S : pour doubler le SNR, il faut quatre fois plus de photons.
2. **Le bruit de lecture** : à chaque fois qu'on vide le seau pour le mesurer, la règle graduée est un peu imprécise (quelques électrons). Cette erreur est la même quelle que soit la quantité d'eau. Elle compte énormément quand les seaux sont presque vides (poses courtes, aurore très faible), et devient négligeable quand ils sont bien remplis.
3. **Le courant d'obscurité (bruit thermique)** : un seau qui fuit à l'envers, c'est à dire qui se remplit tout seul d'un peu d'eau parasite, d'autant plus qu'il fait chaud. Un capteur refroidi (caméra astro "Pro") ou une nuit d'hiver à -15 °C réduisent fortement ce parasite. Exemple publié : 0,0017 e-/s/pixel à 0 °C pour l'ASI6200 ([ZWO](https://www.zwoastro.com/product/asi6200)).
4. **Le fond de ciel** : le ciel n'est jamais noir (pollution lumineuse, lumière zodiacale, airglow, Lune). Ces gouttes là ne sont pas de l'aurore mais elles apportent leur propre bruit de photon. Sous un ciel lumineux, c'est souvent ce fond qui limite, pas le capteur.

Formule simplifiée (par pixel, en électrons) :

SNR = S / √(S + B + D·t + R²)

avec S = signal aurore, B = fond de ciel, D·t = courant d'obscurité accumulé, R = bruit de lecture.

**Conséquence pratique** : avec des poses de plusieurs secondes et une aurore visible, S + B dépasse vite R² et on est "limité par les photons". Dans ce régime, seule la quantité de photons collectés compte, donc l'ouverture, la surface de pixel et le QE. Un bruit de lecture très bas (0,5 à 1 e-) n'apporte un gros gain que pour des poses courtes (cadence vidéo, aurores rapides, pulsantes) ou très faibles.

#### 1.3 Pourquoi de gros pixels ou le binning aident

Regrouper 2 x 2 pixels (binning) revient à fusionner 4 seaux en un seul gros seau : 4 fois plus de signal, SNR multiplié par 2 dans le régime limité par les photons, au prix d'une résolution divisée par 2 en largeur et en hauteur. Avec l'IMX477, un binning logiciel 2 x 2 donne un "pixel virtuel" de 3,1 µm et 3 MP environ. Différence importante : en binning logiciel (après lecture), chaque petit pixel apporte son propre bruit de lecture (le bruit de lecture du super pixel est multiplié par 2). Un vrai gros pixel physique ne paie ce bruit qu'une fois. C'est pour cela qu'un capteur à gros pixels natifs et faible bruit de lecture reste meilleur qu'un petit capteur binné, surtout en poses courtes.

#### 1.4 Le piège : "photons par pixel" contre "photons sur tout le champ"

Le nombre f règle les photons **par unité de surface du capteur**. Mais ce qui fixe la quantité totale de lumière d'une portion de ciel donnée, c'est la **pupille d'entrée** (diamètre D = focale / N) et l'**étendue** (surface de la pupille x angle solide du champ). Pour un même champ de vision (par exemple un fisheye 180°) et un même nombre de pixels, chaque pixel voit le même morceau de ciel, et le signal par pixel est proportionnel à D².

Exemples de calcul :

- Fisheye actuel 2,7 mm f/2,5 sur IMX477 : D = 2,7 / 2,5 = 1,08 mm.
- Objectif 6 mm f/1,2 sur IMX477 : D = 5 mm, mais champ beaucoup plus étroit (ce n'est plus un all-sky).
- Fisheye hypothétique 8 mm f/3,5 sur plein format : D = 2,3 mm. À champ égal, il collecte (2,3 / 1,08)² = 4,5 fois plus de lumière par élément de ciel que le fisheye M12 actuel, alors qu'il est "plus fermé" en nombre f.

**f/1,2 sur petit capteur contre f/2 sur plein format** : par unité de surface de capteur, f/1,2 reçoit (2 / 1,2)² = 2,8 fois plus de lumière. Mais un pixel de 8,4 µm (Sony A7S II) a une surface 29 fois plus grande qu'un pixel de 1,55 µm. Par pixel : (8,4 / 2)² / (1,55 / 1,2)² = 17,6 / 1,67 = 10,5 fois plus de photons pour le plein format à f/2. À même champ de vision, les deux systèmes collectent autant de lumière par pixel **si et seulement si** leurs pupilles d'entrée ont le même diamètre. Le grand capteur n'est pas magique : il permet simplement d'utiliser des objectifs à grande pupille (grande focale ET ouverture correcte) tout en gardant un champ large.

En résumé : **à nombre f égal**, le gain par pixel est le rapport des surfaces de pixel. **À champ et nombre de pixels égaux**, le gain est le rapport des D² des objectifs. Ces deux règles sont utilisées dans le tableau final.

---

### 2. Piste A : capteurs plus grands en MIPI CSI sur Raspberry Pi

Point clé matériel : le Pi 4 n'a qu'un connecteur 15 broches à 2 voies D-PHY, le Pi 5 (et les cartes Compute Module) un connecteur 22 broches à 4 voies ([doc Raspberry Pi](https://www.raspberrypi.com/documentation/accessories/camera.html)). Les cartes de Will Whang exigent 4 voies, donc Pi 5 ou CM4.

#### A1. Sony IMX462 / IMX290 / IMX327 (STARVIS, 1/2,8", 2 MP)

- IMX462 : 1/2,8", 5,6 x 3,2 mm, 1936 x 1096 px, pixels 2,9 µm, bruit de lecture 0,47 à 2,65 e-, puits 11 200 e- (données de la caméra ZWO ASI462MC, [AstroBackyard](https://astrobackyard.com/zwo-asi462mc-review), [Agena](https://agenaastro.com/zwo-asi462mc-2-12-mp-cmos-color-astronomy-camera-with-usb-3-0-asi462mc.html)).
- Support Pi : IMX290 et IMX327 listés comme capteurs tiers supportés par libcamera ([doc logicielle Raspberry Pi](https://raspberrypi.com/documentation/computers/camera_software.html)) ; IMX290 et IMX327 partagent le pilote imx290 ([libcamera-devel 2021](https://lists.libcamera.org/pipermail/libcamera-devel/2021-March/018091.html)) ; série de patchs de novembre 2024 pour des fichiers de tuning IMX327/IMX462 propres ([patchwork libcamera](https://patchwork.libcamera.org/cover/22059/)), statut de fusion non vérifié.
- indi-allsky classe imx462 et imx327 en note "A", avec "bruit élevé à fort gain (+16)" ([wiki indi-allsky](https://github.com/aaronwmorris/indi-allsky/wiki/libcamera-enablement)).
- Module Arducam Pivariety IMX462 : libcamera uniquement, installation d'un pilote noyau par script ; compatibilité Pi 5 annoncée pour les versions récentes (B0444) ([docs Arducam](https://docs.arducam.com/Raspberry-Pi-Camera/Pivariety-Camera/IMX462/), [Arducam B0444](https://www.arducam.com/product/2mp-imx462-color-ultra-low-light-starvis-camera-module-with-141h-wide-angle-m12-lens-for-raspberry-pi/)). L'ISP du Pi ne gère pas le mode HDR du capteur ([forum Arducam](https://forum.arducam.com/t/hdr-image-with-pivariety-camera-imx462/2104)). Prix : non vérifié. Waveshare propose aussi des modules IMX462 et IMX290 ([wiki Waveshare](https://www.waveshare.com/wiki/IMX462_IR-CUT_Camera_(A))).
- Bilan : gain de surface de pixel x3,5 contre IMX477, support logiciel le plus mûr, mais seulement 2 MP sur un très petit capteur.

#### A2. Sony IMX585 (STARVIS 2, 1/1,2", 8 MP) : StarlightEye

- Capteur : 1/1,2", 11,2 x 6,3 mm, 3840 x 2160, pixels 2,9 µm, bruit de lecture 0,7 e-, QE 91 %, puits 40 ke- (couleur), ADC 12 bits (données ZWO ASI585MC, [ZWO](https://www.zwoastro.com/product/asi585mc-mm/)).
- Carte StarlightEye (Will Whang) : Pi 5 et CM4 uniquement (4 voies), Pi 4 non supporté, monture C, capteur de température TMP117, IMU, filtre IR commutable ; v2.0 sans alimentation 5 V externe ; produit retiré de Tindie, renvoi vers shop.willwhang.dev ; pilote "encore en développement", peut exiger une compilation noyau ou DKMS ([Tindie StarlightEye](https://www.tindie.com/products/will123321/starlighteye/)).
- Logiciel : patch libcamera IMX585 + IMX585 mono d'août 2024 co-signé par Raspberry Pi, avec support Pi 5 porté dans l'arbre "downstream" ([libcamera-devel](https://lists.libcamera.org/pipermail/libcamera-devel/2024-August/043882.html), [patchwork](https://patchwork.libcamera.org/patch/20812/)) ; pilote noyau v3 soumis en août 2025, testé sur Pi 4 et Pi 5 ([patchew](https://patchew.org/linux/20250816055432.131912-1-will@willwhang.com)). Statut de fusion dans le noyau principal : non vérifié.
- Alternatives : Leopard Imaging LI-IMX585-MIPI-110H, 3856 x 2176, 439 USD, MIPI mais compatibilité Pi non vérifiée ([Leopard Imaging](https://leopardimaging.com/product/robotics-cameras/cis-2-mipi-modules/i-pex-mipi-camera-modules/rolling-shutter-mipi-cameras/8-39mp-imx585/li-imx585-mipi-110h/)). Arducam vend des IMX585 et IMX678 en **USB 3.0 UVC**, pas en CSI, et dit que libcamera/rpicam ne sont pas la bonne voie pour eux ([forum Arducam](https://forum.arducam.com/t/imx678-and-imx585-with-raspberry-pi/8088)). Soho Enterprise IMX585 : aucun produit trouvé, non vérifié.

#### A3. Sony IMX678 (STARVIS 2, 1/1,8", 8 MP)

- 1/1,8", 3840 x 2160, pixels 2,0 µm, QE crête 83 %, bruit de lecture 0,6 e- (meilleur cas) ([ZWO ASI678MC](https://www.zwoastro.com/product/zwo-asi678mc/), [Adorama](https://www.adorama.com/ZWASI678MC.html)).
- En CSI pour Pi 5 : kit Arducam "PiVistation 5 Darksee" IMX678 ([Arducam](https://arducam.com/product/arducam-pivistation-5-darksee-8-3mp-sony-starvis-2-imx678-ultra-low-light-all-in-one-raspberry-pi-5-camera-kit)), page inaccessible lors de la recherche : interface, prix et support rpicam non vérifiés.
- Pixel à peine plus grand que l'IMX477 (gain de surface x1,7) : intérêt limité pour Aurion, sauf pour le bruit de lecture.

#### A4. Sony IMX283 (1", 20 MP) : OneInchEye

- Capteur : type 1", diagonale 15,86 mm, pixels 2,4 µm, environ 20,3 MP (5496 x 3672 actifs, 5472 x 3648 utiles) ([FRAMOS](https://framos.com/products/sensors/area-sensors/imx283cqj-c-21902/), [Macnica](https://www.macnica.com/eu/atd-europe/en/products-support/suppliers/sony/sony-imx283cqj-c), [Kurokesu](https://www.kurokesu.com/main/2025/08/05/new-20m-imx283-1-csi-2-camera-module/)).
- OneInchEye V2.0 : sortie 12 bits, 179 USD, CM4 (4 voies), "épuisé depuis le 29 mars 2026", pilote en développement ([Tindie OneInchEye](https://www.tindie.com/products/will123321/oneincheye-v20/)) ; compatibilité Pi 5 annoncée par le dépôt en mars 2024 ([OneInchEye](https://will127534.github.io/OneInchEye/)). Kurokesu annonce aussi un module IMX283 CSI-2 ([Kurokesu](https://www.kurokesu.com/main/2025/08/05/new-20m-imx283-1-csi-2-camera-module/)), compatibilité Pi non vérifiée. Bruit de lecture et QE : non vérifié.
- Gain de surface de pixel x2,4, beaucoup de résolution, bon candidat au binning 2 x 2 (pixel virtuel 4,8 µm, 5 MP).

#### A5. Sony IMX294 (4/3", 10,7 MP) : FourThirdsEye

- ZWO ASI294MC Pro (même capteur) : BSI, 4,63 µm, 23,2 mm de diagonale (Sony : 21,63 mm), environ 11,7 MP selon ZWO (10,7 MP selon Sony), bruit de lecture 1,2 e- (moins de 2 e- en HCG), puits 63 700 e-, ADC 14 bits ([ZWO ASI294MC Pro](https://www.zwoastro.com/product/asi294mc-pro/)).
- FourThirdsEye : monture Micro 4/3, Pi 5 et CM4, 4,63 µm, 10,7 MP en photo, matrice Quad Bayer à sortie binnée uniquement ; liaison CSI à 1,78 Gbit/s au delà de la spécification Pi 5 (1,5 Gbit/s), sensibilité à la longueur de nappe (50 cm : pixels défectueux, 20 cm : correct) ; pilote dans un dépôt séparé ([GitHub FourThirdsEye](https://github.com/will127534/FourThirdsEye)) ; fork libcamera requis selon le vendeur, prix 399 USD ([Tindie FourThirdEye](https://www.tindie.com/products/will123321/fourthirdeye-v10), [Hackster](https://www.hackster.io/news/will-whang-launches-the-fourthirdeye-giving-the-raspberry-pi-5-micro-four-thirds-support-for-399-dc0001c607cc)). Pose maximale, puissance : non vérifié.
- C'est le plus gros capteur CSI trouvé pour le Pi : gain de surface de pixel x8,9 contre l'IMX477.

#### A6. Plein format en CSI ?

Aucun module plein format en MIPI CSI compatible Raspberry Pi n'a été trouvé. Sur Pi, le plafond actuel est le 4/3" (FourThirdsEye).

---

### 3. Piste B : caméras astronomiques USB

Toutes ces marques sont gérées par indi-allsky (ZWO, QHY, Starlight Xpress, Svbony, Altair, ToupTek, Player One) ([indi-allsky](https://www.Github.com/aaronwmorris/indi-allsky), [FAQ](https://github.com/aaronwmorris/indi-allsky/wiki/FAQ)). Le logiciel Allsky (créé par Thomas Jacquin, maintenu par AllskyTeam) supporte les ZWO vendues avant octobre 2024 et les caméras Pi, sur Raspberry Pi OS uniquement ([AllskyTeam/allsky](https://github.com/AllskyTeam/allsky)). ZWO fournit des bibliothèques Linux armv8 (64 bits) dans son SDK ([guide Carnegie](https://instrumentation.obs.carnegiescience.edu/Software/ZWO/Setup/setup.html), [forum ZWO](https://bbs.zwoastro.com/d/12315-libasicamera2-for-aarch64?page=2)) ; des soucis sous Pi 5 Bookworm sont signalés sur le forum ZWO ([fil](https://bbs.zwoastro.com/d/17176-problems-with-raspberry-pi5-bookworm-os)).

| Modèle | Capteur | Taille | Pixel | Bruit lecture | QE | ADC | Refroidi | Alim. | Poids | Prix |
|---|---|---|---|---|---|---|---|---|---|---|
| ASI462MC | IMX462 | 1/2,8" | 2,9 µm | 0,47 à 2,65 e- | non vérifié | non vérifié | non | USB, W non vérifié | non vérifié | 249 USD liste ([Highpoint](https://www.highpointscientific.com/zwo-asi462mc-color-planetary-astronomy-camera-asi-462-mc-rfb-ln)) |
| ASI678MC | IMX678 | 1/1,8" | 2,0 µm | 0,6 e- | 83 % | 12 bits | non | USB, W non vérifié | non vérifié | 299 à 349 USD ([ZWO](https://www.zwoastro.com/product/zwo-asi678mc/)) |
| ASI585MC | IMX585 | 1/1,2" | 2,9 µm | 0,7 e- | 91 % | 12 bits | non | USB, W non vérifié | non vérifié | 359 à 449 USD ([ZWO](https://www.zwoastro.com/product/asi585mc-mm/)) |
| ASI294MC Pro | IMX294 | 4/3" | 4,63 µm | 1,2 e- | >75 % (supposé) | 14 bits | oui | 12 V 3 à 5 A recommandé | non vérifié | non vérifié ([ZWO](https://www.zwoastro.com/product/asi294mc-pro/)) |
| ASI2600MC Pro | IMX571 | APS-C 23,5 x 15,7 | 3,76 µm | 1,0 à 3,3 e- selon gain ([FLO](https://firstlightoptics.com/zwo-cameras/zwo-asi-2600mc-pro-usb-30-cooled-colour-camera.html)) | >80 % (estimé) | 16 bits | oui, -35 °C | 12 V 3 A max refroidisseur + 5 W chauffage | 700 g | 1349 à 2868 USD ([ZWO](https://zwoastro.com/product/asi2600)) |
| ASI6200MC Pro | IMX455 | plein format 36 x 24 | 3,76 µm | 0,86 e- | 80 % (MC) | 16 bits | oui, -30 à -35 °C | 11 à 14 V, adaptateur 12 V 3 à 5 A | 700 g | 3599 à 5078 USD ([ZWO](https://www.zwoastro.com/product/asi6200)) |

Notes :

- Les modèles "Pro" refroidis doivent être alimentés en 12 V pour être même reconnus ([ZWO ASI2600](https://zwoastro.com/product/asi2600)). Le refroidisseur peut consommer jusqu'à 12 V x 3 A = 36 W au maximum ([FLO](https://firstlightoptics.com/zwo-cameras/zwo-asi-2600mc-pro-usb-30-cooled-colour-camera.html)) ; en hiver nordique, l'intérêt du refroidissement est moindre car le capteur est déjà froid.
- L'ASI6200MC (61 MP) binné 2 x 2 donne environ 15 MP à 7,5 µm : c'est la réponse "plein format à gros photosites" côté astro, avec 16 bits et refroidissement. Attention au volume de données (fichiers de 120 Mo environ en 16 bits, calcul : 61,2 MP x 2 octets) et à la puissance de calcul du Pi.
- Caméras all-sky intégrées : Starlight Xpress Oculus (CCD ICX205AL, 4,65 µm, 1392 x 1040, fisheye 2,5 mm f/1,2 150°, plus vendue) et Oculus PRO (ICX825AL, 6,45 µm, 1392 x 1040), environ 1340 à 1450 USD chez Astroshop ([B&H](https://www.bhphotovideo.com/c/product/1678071-REG/starlight_xpress_oculus_pro_150_oculus_pro_2_5mm_f_1_2.html), [Astroshop](https://www.astroshop.eu/astronomical-cameras/starlight-xpress-camera-oculus-pro-all-sky-150d-mono/p,86392)). Alcor System OMEA : champ 185°, jusqu'à 2 MP, CCD mono ou couleur, prix historique 3750 à 5600 USD (2013) ([Astronomy.com](https://www.astronomy.com/observing/alcor-systems-omea-all-sky-camera/)) ; prix actuel non vérifié.
- ZWO ne vend pas de "kit all-sky" complet trouvé ; elle vend un objectif 2,1 mm f/2 150° CS pour capteurs 1/3" ([Agena](https://agenaastro.com/zwo-2-1-mm-150-lens-2-1.html)).
- Player One, QHY, ToupTek : mêmes capteurs Sony déclinés ; non détaillés ici faute de vérification.

---

### 4. Piste C : hybrides et reflex plein format pilotés par gphoto2

#### 4.1 Bruit de lecture mesuré (photonstophotos.net)

Valeurs extraites des données du graphique "Input-referred Read Noise" ([photonstophotos RN_e](https://www.photonstophotos.net/Charts/RN_e.htm), données en log2 converties en électrons, gain analogique uniquement) :

| Boîtier | ISO 100 | ISO 800 | ISO 3200 | ISO 6400 |
|---|---|---|---|---|
| Sony A7S (ILCE-7S) | 15,9 e- | 4,9 e- | 1,31 e- | 1,12 e- |
| Sony A7S II (ILCE-7SM2) | 10,1 e- | 4,4 e- | 1,16 e- | 1,05 e- |
| Sony A7S III (ILCE-7SM3) | 13,1 e- | 9,3 e- | 1,06 e- | 0,86 e- |
| Sony A7 III (ILCE-7M3) | 6,4 e- | 1,34 e- | 1,26 e- | 1,19 e- |
| Nikon Z 6 | 6,9 e- | 1,62 e- | 1,27 e- | 1,21 e- |
| Nikon D750 | 5,2 e- | 3,3 e- | 2,6 e- | 2,3 e- |
| Canon EOS R6 | 7,6 e- | 2,1 e- | 1,67 e- | 1,31 e- |
| Canon EOS RP | 36 e- | 5,9 e- | 2,8 e- | 2,6 e- |

À haut ISO, les hybrides plein format récents descendent vers 1 e-, comparable aux caméras astro non refroidies, mais avec des pixels bien plus grands.

#### 4.2 Capteurs et boîtiers

- Sony A7S II : capteur 35,8 x 23,8 mm, 4256 x 2848 px ([DxOMark](https://dxomark.com/Cameras/Sony/A7S-II---Specifications)), soit un pas de pixel calculé de 8,4 µm (35,8 / 4256) ; batterie NP-FW50 ; fonctionnement 0 à 40 °C ; poids environ 627 g avec batterie et carte ; adaptateur secteur AC-PW20 en option ([Imaging Resource](https://www.imaging-resource.com/PRODS/sony-a7s-ii/sony-a7s-iiDAT.HTM)).
- Sony A7S III : environ 699 g, fonctionnement 0 à 40 °C, stockage -20 à 55 °C, USB-C compatible USB Power Delivery pour alimenter la prise de vue, batterie obligatoirement installée, compatibilité PD non garantie avec tous les appareils ([Sony fiche](https://www.sony.co.in/interchangeable-lens-cameras/products/ilce-7sm3/spec), [Sony fonctionnalités](https://www.sony.co.in/interchangeable-lens-cameras/products/ilce-7sm3/feature5)). Définition (12 MP environ) et pas de pixel (environ 8,4 µm) : non vérifiés précisément ici.
- Nikon Z6, D750, Canon R6, RP, Sony A7 III : 24 à 26 MP, pas de pixel autour de 6 µm, non vérifiés ici.
- Plage de fonctionnement 0 °C minimum annoncée par Sony : en conditions aurorales (souvent -10 à -30 °C), on sort de la spécification ; il faut un caisson chauffé et une alimentation externe, car les batteries lithium perdent de la capacité au froid. Comportement exact à basse température : non vérifié.
- Prix d'occasion : non vérifiés (à relever sur MPB, LeBonCoin, eBay au moment de l'achat).

#### 4.3 gphoto2 et la pose longue (bulb)

- Méthode documentée : boîtier en mode M, `shutterspeed=bulb`, puis `bulb=1`, attente, `bulb=0` ; pour certains Canon EOS, la commande `eosremoterelease` ([doc gphoto](https://gphoto.sourceforge.io/doc/remote/)).
- Sony A7 : en 2015, un délai codé en dur de 35 s dans libgphoto2 bloquait les poses bulb de l'A7S ([ticket gphoto 462](https://sourceforge.net/p/gphoto/feature-requests/462)). Un correctif a ensuite permis le bulb sur les A7 ; le projet elysian-a7s l'exploite, mais signale que la pose réelle dépasse la consigne (30 s demandées donnent souvent 36 s d'après l'EXIF) ([elysian-a7s](https://github.com/gschwim/elysian-a7s)). Un utilisateur A7S a eu "The bulb widget is not configurable" avec l'option -B ([ticket gphoto 462](https://sourceforge.net/p/gphoto/feature-requests/462)).
- Nikon : bulb par USB fonctionne sur un D7100 selon un utilisateur, pas sur un D3200 ([free-astro](https://free-astro.org/index.php/Gphoto2_data)). Z6, D750, R6, RP : support bulb par gphoto2 non vérifié. À tester avec `gphoto2 --list-config`.
- Remarque utile : pour l'aurore, les poses utiles dépassent rarement 30 s (au delà, les structures se brouillent). Les vitesses fixes de 1 à 30 s, sans bulb, suffisent dans la plupart des cas, ce qui réduit le risque logiciel.

#### 4.4 Sans Pi : l'approche des chasseurs d'aurores

Beaucoup de chasseurs utilisent simplement un boîtier plein format sur trépied, un objectif grand angle lumineux et un intervallomètre (intégré ou externe) pour faire des timelapses. C'est la solution la plus simple et la plus robuste pour la qualité d'image, mais elle n'est pas autonome (pas de détection, pas de mise en ligne, pas d'exploitation scientifique en continu). Données précises sur les intervallomètres intégrés par modèle : non vérifiées.

---

### 5. Tableau comparatif

Gain par pixel à nombre f égal = (pixel / 1,55 µm)², sans tenir compte des différences de QE (non publiées pour l'IMX477). Pour un binning logiciel 2 x 2 de l'IMX477, le gain est x4 en signal mais avec un bruit de lecture doublé.

| Piste | Exemple | Capteur | Pixel | Gain/pixel vs IMX477 (même f) | Puissance | Poids | Prix | Effort intégration Pi | Risque |
|---|---|---|---|---|---|---|---|---|---|
| Réf. | Pi HQ Camera | IMX477 1/2,3" 12,3 MP | 1,55 µm | x1 (x4 binné 2x2) | non vérifié | non vérifié | non vérifié | nul (actuel) | faible |
| A | Arducam/Waveshare IMX462 | 1/2,8" 2 MP | 2,9 µm | x3,5 | non vérifié | non vérifié | non vérifié | faible (overlay + tuning) | faible à moyen |
| A | StarlightEye | IMX585 1/1,2" 8 MP | 2,9 µm | x3,5 | non vérifié | non vérifié | non disponible | moyen (Pi 5, pilote hors noyau) | élevé (stock, pilote) |
| A | OneInchEye | IMX283 1" 20 MP | 2,4 µm | x2,4 (x9,6 binné 2x2) | non vérifié | non vérifié | 179 USD, épuisé | moyen à fort | élevé |
| A | FourThirdsEye | IMX294 4/3" 10,7 MP | 4,63 µm | x8,9 | non vérifié | non vérifié | 399 USD | fort (fork libcamera, CSI hors spec) | élevé |
| B | ZWO ASI585MC | IMX585 1/1,2" 8 MP | 2,9 µm | x3,5 | USB, non vérifié | non vérifié | 359 à 449 USD | faible (indi-allsky, SDK armv8) | faible |
| B | ZWO ASI294MC Pro | IMX294 4/3" | 4,63 µm | x8,9 | jusqu'à 12 V 3 à 5 A | non vérifié | non vérifié | faible | faible |
| B | ZWO ASI2600MC Pro | IMX571 APS-C 26 MP | 3,76 µm | x5,9 (x23 binné 2x2) | 12 V 3 A max + 5 W | 700 g | 1349 USD et plus | faible | faible |
| B | ZWO ASI6200MC Pro | IMX455 plein format 61 MP | 3,76 µm | x5,9 (x23 binné 2x2) | 11 à 14 V, 12 V 3 à 5 A | 700 g | 3599 à 5078 USD | faible à moyen (données lourdes) | faible |
| B | SX Oculus PRO | ICX825 CCD 1,4 MP | 6,45 µm | x17 | non vérifié | non vérifié | 1340 à 1450 USD | moyen | moyen (CCD ancien) |
| C | Sony A7S II (occasion) | plein format 12 MP | 8,4 µm (calculé) | x29 | NP-FW50 / AC-PW20 | 627 g | non vérifié | moyen à fort (gphoto2, bulb imparfait) | moyen (froid, USB) |
| C | Sony A7S III | plein format 12 MP | env. 8,4 µm (non vérifié) | x29 (non vérifié) | USB-C PD | 699 g | non vérifié | moyen | moyen |

Rappel : ces gains supposent le même nombre f. À champ égal (all-sky), c'est le diamètre de pupille de l'objectif qui décide ; un grand capteur n'apporte son avantage qu'avec un fisheye adapté à sa taille.

---

### 6. Recommandation en trois niveaux

#### Grand public (environ 400 € au maximum)

1. **Garder l'IMX477 mais exploiter le binning 2 x 2 logiciel et l'empilement** : x4 en signal par super pixel, 3 MP restants, coût nul. C'est le premier gain à prendre.
2. **Ou passer à un capteur STARVIS 2,9 µm** : module CSI IMX462 (support libcamera le plus mûr, mais 2 MP) ou caméra USB ZWO ASI585MC (8 MP, 0,7 e-, QE 91 %, 359 à 449 USD) gérée nativement par indi-allsky et Allsky. L'ASI585MC est le meilleur rapport sensibilité/risque, mais dépasse un peu le budget avec le Pi et l'objectif.
3. À éviter pour un produit grand public : les cartes de Will Whang (stocks épuisés, pilotes hors noyau).

#### Avancé (environ 400 à 1500 €)

1. **ZWO ASI294MC Pro** (4/3", 4,63 µm, 14 bits, refroidie) avec un fisheye monture C ou M4/3 adapté au 4/3 : x8,9 par pixel contre l'IMX477, intégration Pi facile. Prix exact non vérifié.
2. **Sony A7S II d'occasion + fisheye plein format** piloté par gphoto2 : le seul "plein format 12 MP à gros photosites" accessible, x29 par pixel, environ 1 e- à ISO 3200 et plus. Inconvénients : bulb imparfait, 0 °C minimum annoncé, alimentation à prévoir. À réserver à un prototype.
3. **FourThirdsEye** (399 USD) pour une piste 100 % CSI open source, en connaissant le risque logiciel.

#### Association / scientifique (sans limite)

1. **ZWO ASI6200MC Pro** (plein format, 3,76 µm, 0,86 e-, 16 bits, refroidissement -30 à -35 °C, 700 g, 3599 à 5078 USD) avec un fisheye circulaire plein format ; binning 2 x 2 pour un "plein format 15 MP à 7,5 µm". Version mono ASI6200MM (QE 91 %) + filtres raies aurorales (557,7 nm et 630 nm) si l'objectif devient scientifique. Prévoir un ordinateur plus puissant qu'un Pi si l'on traite 61 MP en continu.
2. Alternative plus économique : ASI2600MC Pro (APS-C, mêmes pixels, 1349 USD et plus).
3. Alternative non refroidie et plus légère : Sony A7S III (USB-C PD, environ 1 e- à haut ISO), en caisson chauffé.
4. Dans tous les cas : caisson chauffant anti-givre, alimentation 12 V stable, et calibration (darks, flats) pour un usage photométrique.

**Réponse directe à la question du propriétaire** : oui, un plein format à gros photosites est possible, mais pas en CSI sur Raspberry Pi. Deux voies : un hybride Sony A7S (12 MP natifs, 8,4 µm) piloté par USB/gphoto2, ou une caméra astro plein format (ASI6200) binnée. Le Pi peut rester le "cerveau" (planification, détection, mise en ligne) dans les deux cas, via indi-allsky ou gphoto2.

---

## Annexe C : Aurion : étude optique, embarqué, détection d'aurore et traitement du bruit

Date de la recherche : 28 novembre 2026. Contexte : Raspberry Pi 4/5 + HQ Camera (Sony IMX477, 1/2.3"), objectifs actuels M12 2,7 mm f/2,5 fisheye et C/CS 6 mm f/1,2, batterie externe USB-C 25 000 mAh, usage hivernal jusqu'à -20/-30 °C, interface web sur téléphone.

Conventions :
- Chaque fait chiffré est suivi de sa source (URL).
- "non vérifié" signale une information que je n'ai pas pu confirmer par une source primaire.
- "calcul" signale un chiffre que j'ai déduit moi-même à partir de données sourcées.

---

### 0. Données de référence du capteur

HQ Camera, d'après la documentation officielle Raspberry Pi (https://www.raspberrypi.com/documentation/accessories/camera.html) :
- capteur Sony IMX477, 4056 x 3040 pixels, pixels de 1,55 µm ;
- surface image 6,287 x 4,712 mm (diagonale 7,9 mm), format 1/2.3" ;
- montures C/CS ou M12 selon la version ;
- exposition maximale 670,74 s ;
- pas de version NoIR officielle ;
- filtre IR collé, retrait possible mais irréversible (la colle ne tient pas une seconde fois, le verre d'environ 1,1 mm peut se fendre) et annulant la garantie.

Conséquence pratique (calcul) : un fisheye dont le cercle image est inférieur ou égal à 4,7 mm donne un cercle complet sur l'IMX477. Entre 4,7 et 7,86 mm, le cercle est rogné en haut et en bas.

Rappel physique (calcul, optique classique) : pour une source étendue comme une aurore, l'éclairement par pixel ne dépend que du nombre d'ouverture N (proportionnel à 1/N²), pas de la focale. Passer de f/2,5 à f/2,0 apporte x1,56 en signal. Passer à f/1,4 apporte x3,19, soit environ +1,7 IL. C'est le levier le plus direct sur le bruit.

---

### 1. Optique

#### 1.1 Fisheyes et grands-angles rapides compatibles petits capteurs

| Objectif | Focale / ouverture | Champ, cercle image | Monture | Adéquation IMX477 | Source |
|---|---|---|---|---|---|
| Fujinon FE185C046HA-1 | 1,4 mm, f/1,4 à f/16 | 185° x 185° sur 1/2" | C, 140 g, MOD 0,1 m | Cercle complet probable. Modèle absent du catalogue Fujifilm actuel, sans doute arrêté (non vérifié) | https://www.securityinformed.com/amp/fujinon-fe185c046ha-1-cctv-camera-lens-technical-details.html |
| Fujinon FE185C057HA-1 | 1,8 mm, f/1,4 à f/16 | cercle 5,7 mm ; 185° x 154,1° sur 1/2" | C | Cercle légèrement rogné en haut et en bas ; excellente qualité ; actuellement au catalogue | https://www.fujifilm.com/us/en/business/optical-devices/mvlens/fe185-series |
| Fujinon FE185C086HA-1 | 2,7 mm, f/1,8 | cercle 8,6 mm (format 1") | C | Pour capteur 1" (champ très rogné sur IMX477) | même page |
| Fujinon YV2.2x1.4A-SA2 | 1,4 à 3,1 mm, f/1,4 (une source indique f/1,3) | 185° au grand-angle, cercle d'environ 3,45 mm | CS, 80 g, format 1/3" | Petit cercle sur l'IMX477. Attention : version SA2 à diaphragme automatique DC, qui se ferme hors tension. Il faut la version manuelle YV2.2x1.4A-2 | https://asset.fujifilm.com/master/jp/files/2020-01/1d3e6a178a8dd5384c691be23f00a466/FUJINON_cctv_fish_eye_yv2214asa2.pdf, https://www.securityinformed.com/fujinon-yv2-2x1-4a-2-technical-details.html |
| ACF12FM014IRCMM (cité par le guide Allsky) | 1,4 mm, f/1,4 | 182°, cercle 4,59 mm | C | Cercle complet sur l'IMX477 | https://alex-developer.github.io/docs/allsky_guide/hardware/lens.html |
| ACTCS25IR8MPF (même guide) | 2,5 mm, f/1,6 | 190°, cercle 6,4 mm | CS | Rogné en haut et en bas | même page |
| ACF12F0155IRMM (même guide) | 1,5 mm, f/2,0 | 185°, cercle 4,6 mm | CS | Cercle complet | même page |
| Commonlands CIL216 | 1,6 mm, f/2,0 | 195°, cercle 4,7 mm, conçu pour 1/2.3" | M12 | Cercle complet exactement à la hauteur du capteur | https://commonlands.com/products/195degree-fisheye-m12-lens-cil216 |
| Commonlands CIL227 | 2,7 mm, f/2,5 | 185°, cercle 7,8 mm, environ 140° horizontal sur 1/2.3" | M12 | Correspond très probablement à l'objectif actuel d'Aurion (non vérifié) | https://www.automate.org/products/commonlands-llc/cil227 |
| Entaniya M12 220 | f/2,0, 220°, équidistant | cercle 5,1 mm, 10 lentilles dont 1 ED | M12 P0,5, 52 g, titane | Légèrement rogné ; voit sous l'horizon | https://e-products.entaniya.co.jp/en/list/m12-smount-lens/m12-p0-5-s-mount-lenses/rp-l220/ |
| Arducam M25156H18 | 1,56 mm, f/2,0, 1/2.3" | 180° horizontal annoncé sur HQ | M12 | Conçu pour la HQ | https://www.arducam.com/product/arducam-180-degree-fisheye-1-2-3-m12-mount-with-lens-adapter-for-raspberry-pi-high-quality-camera/ |
| Sunex DSL215 | fisheye 185° pour 1/4" à 1/2" | voir fiche | M12 | Les variantes Framos sont souvent pour 1/4" ou 1/3", à vérifier | https://www.optics-online.com/OOL/DSL/DSL215.PDF |
| Sunex DSL216 | 1,3 mm, f/2,0, jusqu'à 1/3" | plus de 180° | M12 | Trop petit pour l'IMX477 | https://findlight.net/optics/lenses/machine-vision/dsl216-high-resolution-miniature-superfisheye-lens |
| Omegon 2,5 mm f/1,2 | 2,5 mm, f/1,2 | champ non publié dans les extraits consultés (non vérifié) | CS | Vendu pour vues générales du ciel et météores | https://www.astroshop.eu/miscellaneous/omegon-cs-mount-lense-2-5mm-f-1-2/p,74476 |
| ZWO 2,5 mm 170° | f/1,2 selon un revendeur, f/2 selon d'autres (contradiction) | 170° diagonale, format 1/3" | CS | Contradiction de sources, à vérifier | https://agenaastro.com/zwo-2-5-170-lens-2-5.html |
| ZWO 2,1 mm 150° | f/2 | 150°, 1/3" | CS | | https://agenaastro.com/zwo-2-1-mm-150-lens-2-1.html |
| Starlight Xpress Oculus | 2,5 mm f/1,2 | 150° | intégré | Caméra all-sky complète | https://www.bhphotovideo.com/c/product/1417162-REG/starlight_xpress_oculus_all_sky_ccd.html |

Lensation et Arecont : aucune fiche avec ouverture publiée trouvée lors de cette recherche (non vérifié).

Pour capteurs plus grands (1", 4/3) :
- Fujinon FE185C086HA-1, 2,7 mm f/1,8, cercle 8,6 mm pour 1" (source Fujifilm ci-dessus).
- Meike 6,5 mm f/2,0, fisheye circulaire 190°, monture Micro 4/3 : https://www.adorama.com/mk652mft.html
- Côté capteurs pour Pi, Will Whang a publié des cartes open source :
  - StarlightEye (IMX585), compatible Pi 5 et CM4 en 4 voies : https://www.tindie.com/products/will123321/starlighteye/ (annonce marquée "plus en vente") ;
  - OneInchEye (IMX283, 1") : https://will127534.github.io/OneInchEye/ ;
  - FourThirdsEye (IMX294, 4/3), 399 $ : https://www.cnx-software.com/2025/03/27/fourthirdseye-is-an-open-source-hardware-10-7mp-imx294-camera-module-for-the-raspberry-pi-5-cm4/
  - Prise en charge libcamera non standard (pilotes à compiler), donc à réserver à une phase R&D.

#### 1.2 Réseaux de référence : ce qu'ils utilisent

Global Meteor Network (GMN, logiciel RMS) :
- caméra IP à capteur IMX291 de préférence, IMX307 en alternative : https://github.com/markmac99/RMS (miroir du dépôt RMS) ;
- objectif 4 mm f/0,95 (parfois noté 3,6 mm) d'environ 88° x 45° : https://www.popastro.com/meteor/?p=4069, https://globalmeteornetwork.org/wp-content/uploads/2022/03/CJENIK_2022_Mar_15.pdf ;
- 8 mm conseillé en ville ; le filtre IR du support d'objectif est retiré ;
- ce n'est donc pas un "all-sky" mais un champ moyen très lumineux. L'hypothèse "IMX291 + 4 mm f/0,95" de la demande est confirmée, avec l'ambiguïté 3,6/4 mm entre sources.

Allsky (Thomas Jacquin, maintenu par l'Allsky Team) : le guide matériel cite des focales de 1,5 à 2,5 mm et des ouvertures de f/1,4 à f/2,8 (https://alex-developer.github.io/docs/allsky_guide/hardware/lens.html).

ZWO : les ASI224MC ont été livrées avec un 2,5 mm all-sky 170° (https://agenaastro.com/zwo-asi224mc-cmos-color-astronomy-imaging-camera.html).

Alcor : non couvert par cette recherche (non vérifié).

#### 1.3 Recommandation optique pour Aurion

1. Meilleur gain par euro : passer d'un M12 f/2,5 à un fisheye f/1,4 à 1,4 à 1,8 mm en monture C sur la HQ en version C/CS. Les meilleures options sont le Fujinon FE185C057HA-1 (actuel, f/1,4) ou un clone 1,4 mm f/1,4 à cercle de 4,6 mm (cercle complet). Gain d'environ x3 sur le signal (calcul).
2. En M12 compact : Commonlands CIL216 (f/2,0, cercle 4,7 mm, cercle complet) ou Entaniya 220 (f/2,0, qualité supérieure). Gain d'environ x1,56 (calcul).
3. Le 6 mm f/1,2 reste le plus lumineux, mais ne couvre qu'environ 55° en horizontal sur l'IMX477 (calcul : 2 x atan(6,287 / 12)). C'est le bon choix pour un mode "aurore vers le nord".
4. À éviter : les objectifs CCTV à diaphragme automatique DC (ils se ferment sans signal), et les ouvertures annoncées sans fiche du fabricant (ZWO 2,5 mm : contradictions).

#### 1.4 Buée et givre

Mécanisme : refroidissement radiatif du dôme par nuit claire (https://britastro.org/node/25816).

Puissances de chauffage relevées :
- bandes USB 5 V pour objectifs : 3 W (Vixen 360 III, 5 V 0,6 A), 6 W (Starfield DH-3), 7,25 à 9,75 W (Svbony SV172), 9 à 9,25 W (Neewer HW-17B). Sources : https://neewer.com/products/neewer-usb-lens-heater-for-dslr-camera-and-telescope-66602321, https://www.telescopescanada.ca/products/svbony-sv172-dew-heater-strip-for-telescopes-and-cameras, https://crkphotoimaging.com.au/products/vx35418/vixen-lens-heater-360-iii-dew-and-fog-eliminator ;
- chauffage de dôme all-sky Moonglow : faible puissance contre la buée, 7 W pour faire fondre la glace (https://www.astroshop.eu/miscellaneous/moonglow-all-sky-cam-dome-heater/p,46465) ;
- bande anti-buée ZWO : environ 4,2 W en 12 V 0,35 A (https://us.zwoastro.com/products/zwo-anti-dew) ;
- montage amateur Raspberry Pi : résistance de 150 Ω sur 12 V, environ 0,96 W, résistance de 2 W par sécurité (https://www.instructables.com/Raspberry-Pi-Dew-Heater-for-All-sky-Camera) ;
- test amateur : un dôme ventilé avec seulement 1 W en 5 V protège nettement de la rosée (https://www.printables.com/article/unventilated-allsky-camera-housing-lets-experiment-VNGbOrZ) ;
- projet de chauffage basse consommation pour caméra Pi : https://github.com/hdiessner/Allskycam-heating ;
- le logiciel Allsky gère un chauffage anti-buée piloté.

Dôme acrylique ou verre :
- l'acrylique varie en qualité et en transparence, et se raye en quelques semaines dehors (témoignages : https://britastro.org/?p=629705, https://hackaday.com/2023/07/01/an-all-sky-camera-to-watch-the-night-sky) ;
- le verre est plus dur et plus stable optiquement (non mesuré pour cet usage précis) ;
- aucune mesure indépendante de transmission comparée trouvée.

Revêtements :
- hydrophobe type Rain-X : retours anecdotiques positifs, mais durée de quelques semaines par application ; un revêtement imparfait peut dégrader la transmission (https://www.uniquephoto.com/community/qa/how-can-i-reduce-cleaning-and-contamination-on-an-outdoor-all-sky-camera-dome) ;
- l'approche hydrophile (film d'eau plutôt que gouttes) existe dans des brevets de capots de caméra (https://image-ppubs.uspto.gov/dirsearch-public/print/downloadPdf/9712729).

Dessiccant : pas de source chiffrée trouvée. Usage courant d'un sachet de silice dans le boîtier fermé pour la condensation interne (non vérifié, pratique répandue).

Recommandation Aurion :
- chauffage du dôme régulé de 1 à 2 W en régime normal (PWM selon le point de rosée mesuré par un capteur de température et d'humidité extérieur) ;
- capacité de 5 à 7 W ponctuellement pour dégivrer ;
- dôme en verre si le budget le permet ;
- bonne étanchéité, et sachet de silice changé à chaque saison.

#### 1.5 Filtre IR et raies aurorales

Raies, d'après le tableau de https://www.swsc-journal.org/articles/swsc/full_html/2020/01/swsc200009/T1.html :
- 557,7 nm : oxygène atomique O(1S vers 1D), vers 110 km ;
- 630,0 nm : oxygène atomique O(1D vers 3P), vers 220 à 250 km ;
- 427,8 nm et 391,4 nm : N2+ premier système négatif, vers 80 à 110 km ;
- le rapport 630,0 / 557,7 supérieur à 1 caractérise l'aurore de cornet, électrons d'environ 0,5 keV (https://angeo.copernicus.org/articles/39/849/2021/).

Courbe officielle du filtre IR-cut de la HQ et de la GS (PDF Raspberry Pi : https://pip-assets.raspberrypi.com/categories/810-raspberry-pi-global-shutter-camera/documents/RP-008148-DS-1-camera-extended-spectral-sensitivity.pdf). Lecture graphique, approximative :
- environ 90 % de transmission vers 500 nm ;
- environ 50 % vers 600 nm ;
- environ 25 à 30 % vers 630 nm ;
- quasi 0 au-delà de 700 nm ;
- environ 85 % à 427,8 nm et environ 80 % à 557,7 nm.

Conséquence : le filtre d'origine coupe environ 70 % de la raie rouge à 630 nm. Les revendeurs le désignent comme un Hoya CM500 (https://www.crowdsupply.com/stereopi/stereopi-v2/updates/bees-and-birds-can-see-the-uv-spectrum-with-hq-cameras-now-you-can-too) et Arducam annonce une coupure à 650 nm. La courbe officielle montre une coupure à mi-hauteur plus précoce, vers 600 nm.

Même PDF, HQ sans filtre : le canal rouge culmine vers 600 nm à environ 0,62 (sensibilité normalisée), vaut environ 0,5 à 630 nm, et reste à 0,2 à 0,3 entre 700 et 800 nm. Le vert culmine vers 520 nm.

Options :
- (a) Garder le filtre. Couleurs naturelles, rouge sous-estimé.
- (b) Retirer le filtre. Le rouge à 630 nm est multiplié par environ 3 (calcul à partir des lectures graphiques), mais le proche IR passe aussi : fond de ciel, pollution lumineuse, mise au point légèrement décalée, dérive de la balance des blancs.
- (c) Option recommandée pour la qualité : retirer le filtre d'origine et mettre devant l'objectif (ou dans la monture C) un filtre UV/IR de bande plus large. Exemple : Baader UV/IR-Cut L CMOS, 420 à 685 nm à 98 % selon les revendeurs (https://agenaastro.com/baader-cmos-optimized-uv-ir-cut-l-filter-fcuvir.html). On récupère 630 nm sans le proche IR. Attention, sur un fisheye, un filtre frontal est impossible : il faut un filtre arrière ou un filtre dans la monture C (non vérifié pour chaque objectif).
- (d) Prévoir une seconde caméra HQ sans filtre pour l'option (b) ou (c) : le retrait est irréversible selon Raspberry Pi.

---

### 2. Miniaturisation et produit embarqué

#### 2.1 Cartes

Consommations (sources officielles et mesures) :

| Carte | Valeur | Source |
|---|---|---|
| Pi 4 | courant typique carte nue active 600 mA, alimentation recommandée 3 A | https://www.raspberrypi.com/documentation/computers/raspberry-pi.html |
| Pi 5 | 800 mA typique, alimentation recommandée 5 A | même page |
| Zero 2 W | 350 mA typique, alimentation recommandée 2 A | même page |
| Pi 4 B (mesure) | repos 540 mA (2,7 W), stress 4 cœurs 1280 mA (6,4 W) | https://www.pidramble.com/wiki/benchmarks/power-consumption |
| Zero 2 W (mesure, HDMI et LED éteints) | 100 mA (0,6 W) sans Wi-Fi, 120 mA (0,7 W) avec Wi-Fi | même page |
| Pi 5 (Jeff Geerling) | repos mesuré à 1,8 W avec l'alimentation officielle, jusqu'à 3 W avec une autre | https://www.jeffgeerling.com/blog/2023/answering-some-questions-about-raspberry-pi-5/ |
| Pi 5 éteint, réglage par défaut | 1,2 à 1,6 W | https://www.jeffgeerling.com/blog/2023/reducing-raspberry-pi-5s-power-consumption-140x/ |
| Pi 5 éteint avec POWER_OFF_ON_HALT=1 | environ 0,01 W | même page |
| CM5 sur carte IO | repos 2,3 W à la prise, presque 2x le CM4 en pleine charge | https://jeffgeerling.com/node/3422 |

Autres pistes :
- Compute Module 5 :
  - 2 entrées MIPI CSI 4 voies ;
  - plage de fonctionnement de -20 °C à +85 °C ;
  - fichiers PCB de la carte IO publiés pour concevoir un porteur sur mesure.
  - Sources : https://pip-assets.raspberrypi.com/categories/944-raspberry-pi-compute-module-5/documents/RP-008181-DS-10-cm5-product-brief.pdf, https://www.raspberrypi.com/documentation/hardware/computemodule/
  - Il existe des CM4 industriels de -40 °C à +85 °C (https://www.raspberrypi.com/app/uploads/2025/03/Industrial-Customer-Presentation_Compute-Module.pdf).
  - Pour le Pi 5 en carte, aucune plage officielle d'ambiance trouvée (on lit souvent 0 à 50 °C, non vérifié).
- Zero 2 W + HQ :
  - capteur pris en charge par libcamera, connecteur 22 broches, câble adapté nécessaire (https://www.raspberrypi.com/documentation/accessories/camera-software.html) ;
  - limite mémoire : une image RGBA 12 MP occupe 48 Mo, avec une zone CMA par défaut de 256 Mo sur 512 Mo (rapport ancien : https://github.com/raspberrypi/picamera2/issues/46). Le traitement plein format (empilement, détection) est serré.
  - Aurion a déjà une étude docs/ETUDE_PI_ZERO.md.
- Rockchip (Radxa ROCK 5, RK3588) : l'IMX477 fonctionne avec des correctifs noyau communautaires et un câble adaptateur. Le réglage ISP libcamera RK3588 était encore immature dans les fils consultés (https://forum.radxa.com/t/imx477-imx577-imx766-on-rock-5/23689/12). Déconseillé pour un produit open source simple.
- Luckfox Pico (RV1106) : ISP intégré, MIPI CSI 2 voies, 5 MP 30 i/s maximum (https://waveshare.com/luckfox-pico-pro.htm?sku=25717). Aucune mesure de consommation fiable trouvée. Un retour isolé fait état d'une veille d'au moins 70 mA sur des cartes RV1103 (https://community.element14.com/technologies/internet-of-things/f/forum/56442/has-anyone-built-a-truly-low-power-camera-iot-device-on-an-off-the-shelf-som/231907). Pas de pilote IMX477 connu (non vérifié).

#### 2.2 Astuces basse consommation (Pi)

- Pi 5, coupure franche et réveil horodaté (documentation officielle, page ci-dessus) :
  - `sudo rpi-eeprom-config --edit`, puis `POWER_OFF_ON_HALT=1` et `WAKE_ON_GPIO=0` ;
  - réveil : `echo +600 | sudo tee /sys/class/rtc/rtc0/wakealarm` puis `sudo halt` ;
  - environ 3 mA dans cet état, redémarrage au bout de 10 minutes ;
  - une valeur absolue (horodatage Unix) est aussi acceptée ;
  - pile RTC rechargeable officielle ; charge activée par `dtparam=rtc_bbat_vchg=3000000`.
  - Usage pour Aurion : éteindre le Pi le jour et le réveiller à l'heure du crépuscule nautique.
- Ventilateur officiel : environ 60 mA (0,3 W) à couper l'hiver (même page).
- LED, Bluetooth, HDMI, sous-cadencement :
  - pidramble cite "Disable HDMI / onboard LEDs / minimize accessories" sans chiffrer le gain sur Pi 4 (https://www.pidramble.com/wiki/benchmarks/power-consumption) ;
  - mécanismes usuels dans config.txt : `dtparam=act_led_trigger=none`, `dtoverlay=disable-bt`, `arm_freq` réduit, gouverneur `powersave` (gains non vérifiés, à mesurer au wattmètre USB) ;
  - avec le pilote KMS, HDMI sans écran consomme peu, mais la commande `tvservice` n'existe plus (non vérifié).
- Wi-Fi en point d'accès : pas de mesure sourcée trouvée. Sur Zero 2 W, le Wi-Fi coûte environ 20 mA de plus (mesure pidramble ci-dessus).

#### 2.3 Bilan énergétique (calcul)

- 25 000 mAh à 3,6-3,7 V nominal représente environ 90 à 92 Wh de cellule, soit environ 75 à 80 Wh utiles en 5 V avec un rendement de 85 % (hypothèse non vérifiée pour le modèle utilisé).
- Pi 4 + HQ en acquisition (environ 3 à 4 W) et chauffage de 1,5 W : environ 5 W au total, soit environ 15 h à 20 °C.
- À -20 °C, la capacité des Li-ion peut chuter jusqu'à 50 % (source fabricant, indicative : https://www.evlithium.com/Blog/low-temperature-lifepo4-battery-performance.html). Il reste donc environ 7 à 8 h, soit une nuit d'hiver tout au plus.

#### 2.4 Batterie par grand froid

- Ne jamais recharger un Li-ion ou un LiFePO4 sous 0 °C : risque de dépôt de lithium, avec perte de capacité définitive. La plage de -30 à +80 °C souvent citée ne concerne que la décharge (https://www.redarcelectronics.com/us/resources/chargers-isolators-faqs/do-not-charge-lithium-battery-below-32-degrees/).
- Le dépôt est réversible à -10 °C mais irréversible à -20 °C dans une étude de 2025 (https://discovery.ucl.ac.uk/id/eprint/10215263/1/Robinson_1-s2.0-S0378775325024012-main.pdf). Pas de dépôt à C/10 mais dépôt sévère à C/3 et au-delà à basse température sur LiFePO4 (https://link.springer.com/article/10.1007/s11581-017-2032-y).
- Stratégie :
  - batterie dans le boîtier isolé (mousse XPS ou PIR) avec la chaleur perdue du Pi ;
  - recharge uniquement à l'intérieur ;
  - éventuellement batterie à chauffage intégré, ou mesure de température batterie avant d'autoriser la recharge ;
  - pour une installation fixe, alimentation secteur 5 V ou PoE avec régulation thermique ;
  - le LTO (titanate) tolère mieux le froid, mais je n'ai pas de source dans cette recherche (non vérifié).

#### 2.5 Boîtier

- IP65 : un boîtier de dérivation électrique en polycarbonate avec presse-étoupes et membrane respirante (évent PTFE) limite la condensation (pratique courante, non vérifié par source).
- ASA imprimé : bonne tenue aux UV (non vérifié par source dans cette recherche).
- Dissipateur aluminium : utile l'été. L'hiver, il vaut mieux garder la chaleur pour la batterie et le dôme.
- Retour d'expérience de boîtier : https://allskykamera.space/kamerabau_gehaeuse.php

---

### 3. Détection d'aurore : état de l'art

#### 3.1 Projets existants

indi-allsky (https://github.com/aaronwmorris/indi-allsky) :
- prédiction d'aurore par OVATION : score agrégé dans un rayon d'environ 500 miles (grille de 5° x 5°) ;
- Kp, Bt, Bz, puissance hémisphérique, vitesse et densité du vent solaire ;
- mise à jour toutes les 60 minutes ;
- comptage d'étoiles par correspondance de motifs OpenCV, pour juger de l'état du ciel ;
- météores par Canny et transformée de Hough ;
- "Moon mode" : baisse du gain quand la Lune est haute ;
- darks maîtres automatiques (sigma clipping ou moyenne) et empilement (max, min, moyenne) ;
- keogrammes temps réel et longue durée ;
- dans `testing/image/mlCloudDetect.py`, un classifieur Keras adapté de https://github.com/gordtulloch/mlCloudDetect, avec les classes Aurora, Clear, Cloudy, AuroraMoon, Frost, Overcast, Snow, Partly Cloudy ;
- le code des URL NOAA est dans `indi_allsky/aurora.py`.

Allsky (Thomas Jacquin) : timelapse, keogrammes, chauffage anti-buée. Pas de détection d'aurore native trouvée (non vérifié).

GMN (RMS) : détection de météores, pas d'aurore.

Aurorasaurus :
- plateforme NASA de signalements citoyens et réseaux sociaux, contrôlés qualité (https://ui.adsabs.harvard.edu/abs/2015SpWea..13..548M) ;
- une API existe pour la recherche (https://ntrs.nasa.gov/citations/20190002362), points d'accès non trouvés (non vérifié) ;
- jeu de données sur Zenodo (https://zenodo.org/records/1255196) ;
- biais : les signalements suivent la population.

#### 3.2 Méthodes publiées

Classification par réseaux de neurones :
- Clausen et Nickisch 2018, JGR Space Physics 123, 5640-5647 (doi 10.1029/2018JA025274) :
  - jeu OATH de 5824 images THEMIS étiquetées en 6 classes (arc, discrète, diffuse, nuageux, Lune, ciel clair sans aurore) ;
  - descripteur de 1001 dimensions issu d'un réseau préentraîné, puis classifieur ridge ;
  - environ 82 % d'exactitude ;
  - données publiques sur tid.uio.no.
  - Source secondaire : https://www.aminer.org/pub/5c757572f56def97989f31c3/
- Sado et al. 2022 : apprentissage par transfert sur 80 réseaux, 73 % sur 6 classes et 91 % en binaire aurore ou non (https://www.mdpi.com/2218-1997/9/2/96/html). Pour Aurion, le binaire est ce qui compte.
- Kvammen et al. 2020, JGR 125, e2020JA027808 :
  - caméra all-sky couleur près de Kiruna, 7 classes ;
  - ResNet-50 le meilleur, environ 92 % de précision ;
  - images ambiguës, lunaires, crépusculaires et nuageuses exclues de l'entraînement ;
  - données sur DataverseNO, doi 10.18710/SSA38J (https://munin.uit.no/handle/10037/19703).
- Apprentissage contrastif SimCLR sur OATH (https://arxiv.org/pdf/2109.13899). Apprentissage multivue multi-longueurs d'onde (https://arxiv.org/pdf/2311.02947).

Méthode couleur légère, Yamauchi et Brändström 2023, GI 12, 71-90 (https://gi.copernicus.org/articles/12/71/2023/) :
- JPEG all-sky de 482 x 482 pixels ;
- classification pixel par pixel en RGB et teinte ;
- pixel aurore si R < G et B < G (teinte entre 0,167 et 0,5) ;
- catégories : aurore forte, arc vert, diffuse visible, nuage, Lune, lumière artificielle ;
- masque de 14 fois le rayon lunaire autour de la Lune ;
- indice publié chaque minute ;
- alerte "niveau 6" si au moins 2 des 3 conditions : %arc ≥ 3 %, %fort ≥ 0,2 %, L3 ≥ 8 ;
- environ 90 % des intensifications détectées en moins de 10 minutes, 4 nuits de faux positifs sur environ 45 (hors Lune et crépuscule).
- C'est la référence la plus directement transposable sur un Pi : sans réseau de neurones, explicable, et sur des images couleur grand public.

#### 3.3 Modèles embarqués

- MobileNetV2 en TFLite : environ 23,5 ms sur Pi 5 et 112,6 ms sur Pi 4 (tableau communautaire, peut-être ancien : https://github.com/aallan/benchmarking-ml-on-the-edge).
- TFLite était le plus rapide sur Pi 4B face à ONNX et PyTorch dans une étude de type MLPerf (https://arxiv.org/pdf/2303.05016).
- Avec une image toutes les 15 à 60 s, une inférence de quelques centaines de ms est négligeable, même sur Zero 2 W (calcul, non mesuré).
- Piste : affiner un MobileNetV2 ou un EfficientNet-Lite en binaire (aurore, pas d'aurore, nuage, Lune) sur OATH, puis sur des images Aurion étiquetées via l'interface web. Il faut garder la règle couleur de Yamauchi comme garde-fou explicable.

#### 3.4 Données spatiales NOAA SWPC (vérifiées en direct le 28/11/2026)

- Kp 1 minute (estimé) : https://services.swpc.noaa.gov/json/planetary_k_index_1m.json
  - format : `[{"time_tag","kp_index","estimated_kp","kp"}]`.
- Kp 3 h, observé sur 7 jours : https://services.swpc.noaa.gov/products/noaa-planetary-k-index.json
  - format objet actuel : `{"time_tag","Kp","a_running","station_count"}`.
  - La notice SCN 26-21 annonce le passage de l'ancien format "première ligne = en-têtes" à des objets JSON : https://www.weather.gov/media/notification/pdf_2026/scn26-21_Data_Format_Changes_Impacting_SWPC_Products.pdf
- Prévision Kp : https://services.swpc.noaa.gov/products/noaa-planetary-k-index-forecast.json
  - format : `{"time_tag","kp","observed","noaa_scale"}`.
- Champ magnétique du vent solaire, 1 min : https://services.swpc.noaa.gov/json/rtsw/rtsw_mag_1m.json
  - champs `bt`, `bz_gsm`, `source` (SOLAR1, ACE…) et `active` : filtrer sur `active: true`.
- Plasma du vent solaire, 1 min : https://services.swpc.noaa.gov/json/rtsw/rtsw_wind_1m.json
  - champs `proton_speed`, `proton_density`.
- Résumés rapides :
  - https://services.swpc.noaa.gov/products/summary/solar-wind-mag-field.json, format `{"bt","bz_gsm","time_tag"}` ;
  - https://services.swpc.noaa.gov/products/summary/solar-wind-speed.json.
- OVATION : https://services.swpc.noaa.gov/json/ovation_aurora_latest.json
  - environ 900 ko, `"coordinates": [[lon, lat, proba], ...]` ;
  - champs "Observation Time" et "Forecast Time".
- Puissance hémisphérique : https://services.swpc.noaa.gov/text/aurora-nowcast-hemi-power.txt
- Attention : les anciennes URL `products/solar-wind/plasma-2-hour.json` et `mag-2-hour.json` répondent 404 au 28/11/2026 (testé).
- Ces fichiers sont ceux qu'utilise indi-allsky (`indi_allsky/aurora.py`).

#### 3.5 Garde-fous : Soleil, Lune, nuages

- Hauteur du Soleil :
  - opérer sous -12° (crépuscule nautique) ou -18° (astronomique) ;
  - Yamauchi note que le crépuscule rend la détection très difficile au printemps à Kiruna ;
  - les seuils de -12° et -18° sont des définitions standard, choisir selon la latitude (non vérifié comme recommandation d'un projet précis).
- Lune :
  - masque autour du disque (14 fois le rayon chez Yamauchi) ;
  - réduction du gain (Moon mode d'indi-allsky) ;
  - classe "AuroraMoon" dans mlCloudDetect.
- Nuages par comptage d'étoiles :
  - détection de taches LoG (OpenCV ou scikit-image) et comparaison au nombre d'étoiles attendu d'après un catalogue ;
  - image nuageuse si moins de 75 % des étoiles attendues (https://arxiv.org/abs/2507.21711) ;
  - CLOWN/PASO (https://arxiv.org/pdf/2409.04245) ;
  - ASTRI, classes clair, nuageux, indéterminé (https://arxiv.org/pdf/1502.02241) ;
  - calibration astrométrique d'un fisheye par traces d'étoiles (https://arxiv.org/pdf/2508.17146) ;
  - apprentissage automatique sur environ 2000 images (https://ar5iv.arxiv.org/html/2003.11109).
- Logique proposée pour Aurion :
  - Soleil sous le seuil ;
  - puis ciel suffisamment clair (comptage d'étoiles dans un masque) ;
  - puis contexte spatial favorable (OVATION local, Kp, Bz négatif soutenu) ;
  - puis indice couleur (pixels G dominants au-dessus du fond, et rapport R/G pour le rouge) ;
  - puis éventuellement un classifieur TFLite ;
  - alerte si l'indice image dépasse le seuil sur 2 ou 3 images consécutives.
  - Les données NOAA servent de probabilité a priori, jamais de preuve.

---

### 4. Réduction du bruit pour le timelapse nocturne

#### 4.1 Acquisition

- Pour les longues poses, couper l'AE/AGC et l'AWB, puis fixer le temps de pose et le gain (https://www.raspberrypi.com/documentation/computers/camera_software.html).
- `rpicam-still --raw` produit un DNG à côté du JPEG.
- Le débruitage ISP de Raspberry Pi gère le spatial, le CDN (couleur) et le temporel, avec un mode "off" distinct (https://git.libcamera.org/libcamera/libcamera.git/tree/src/ipa/rpi/controller/rpi/denoise.cpp). Syntaxe exacte de l'option à vérifier avec `rpicam-still --help` (non vérifié).
- indi-allsky signale que la correction des pixels défectueux (DPC) de l'IMX477 est active par défaut et doit être désactivée pour les longues poses (https://github.com/aaronwmorris/indi-allsky/wiki/Raspberry-PI-HQ-Camera).
- Température capteur : le helper IMX477 de libcamera lit la température dans les données embarquées, bornée de -20 à 80 °C (https://patchwork.libcamera.org/patch/16424). Cela permet d'indexer une bibliothèque de darks par température. Nom exact de la clé picamera2 à vérifier (probablement "SensorTemperature", non vérifié).

#### 4.2 Calibration et empilement

- Bibliothèque de darks par (temps de pose, gain, température capteur) : indi-allsky automatise les darks maîtres en sigma clipping ou en moyenne.
- Siril :
  - `siril-cli`, et un mode "pipe" sans interface (https://www.mankier.com/1/siril) ;
  - `calibrate -dark=` avec correction cosmétique `-cc=dark` ;
  - empilement avec rejet sigma ou sigma winsorisé, ce dernier conseillé au-delà de 50 images (https://siril.readthedocs.io/en/1.2/preprocessing/stacking.html).
  - Disponible en paquet Debian (https://sources.debian.org/data/main/s/siril/1.4.2-1/commands_reference/commands_reference.html), donc en principe sur Raspberry Pi OS arm64 (non vérifié en pratique).
- Python :
  - astroalign aligne par triangles d'étoiles, sans WCS, et accepte NumPy, NDData et CCDData (https://pypi.org/project/astroalign/). Limite : peu d'étoiles ou champ très dense.
  - ccdproc et astropy pour combiner avec sigma clipping ; rawpy (LibRaw) pour lire les DNG. Options non vérifiées dans cette recherche.
- Fisheye et rotation du ciel :
  - la rotation est autour du pôle céleste ;
  - pour empiler 3 à 8 images courtes, on peut tourner autour du pôle mesuré par calibration astrométrique ;
  - sinon on empile par fenêtre glissante courte, sans alignement, en acceptant un léger filé.
  - Pour une aurore qui bouge en secondes, il faut limiter la fenêtre (2 à 4 images) ou empiler seulement le fond via un masque. C'est mon analyse, non vérifiée.
- Ressources :
  - indi-allsky stocke les RAW de l'empilement en mémoire : environ 25 Mo par image 4056 x 3040, 100 Mo pour 4 images ;
  - il recommande un Pi 4 avec au moins 2 Go pour empiler en 4K ;
  - une image 1920 x 1080 est calibrée, dématricée et compressée en moins de 0,5 s sur un Pi 3.

#### 4.3 Ce qui tourne sur un Pi

| Étape | Pi 4/5 | Zero 2 W |
|---|---|---|
| Soustraction de dark + rejet de pixels chauds (NumPy/OpenCV, domaine RAW) | oui | oui en binning 2x2 |
| Empilement 3 à 8 images avec sigma clipping | oui | limite mémoire en plein format |
| Alignement astroalign | oui (quelques s, non mesuré) | lent |
| Siril (calibration et empilement en lot) | oui pour le post-traitement nocturne | déconseillé |
| Débruitage neuronal | à éviter en temps réel | non |

Recommandation :
- dark maître par température, soustrait en RAW avant dématriçage ;
- binning 2x2 en RAW pour l'aperçu web et la détection (le signal par pixel est multiplié par 4) ;
- empilement court en moyenne avec rejet sigma pour le timelapse ;
- Siril la nuit ou au matin pour l'image de synthèse.

---

### 5. Synthèse des recommandations

1. Optique : fisheye f/1,4 en monture C de 1,4 à 1,8 mm (Fujinon FE185C057HA-1 ou clone 1,4 mm à cercle de 4,6 mm), soit environ x3 de signal par rapport au f/2,5 actuel. En M12 : CIL216 ou Entaniya 220 en f/2,0.
2. Filtre : l'IR-cut d'origine coupe environ 70 % de la raie à 630 nm. Option qualité : retirer le filtre sur un second module et ajouter un filtre 420 à 685 nm.
3. Buée : dôme en verre, chauffage PWM de 1 à 2 W régulé sur le point de rosée, 5 à 7 W pour dégivrer.
4. Énergie : Pi 5 avec POWER_OFF_ON_HALT et wakealarm pour dormir le jour (environ 3 mA). Batterie dans un boîtier isolé, jamais rechargée sous 0 °C. Une batterie de 25 Ah suffit à peine pour une nuit à -20 °C avec chauffage.
5. Détection : chaîne Soleil, puis nuages (étoiles), puis NOAA (OVATION, Kp, Bz), puis indice couleur à la Yamauchi, puis classifieur TFLite optionnel entraîné sur OATH puis sur les données Aurion.
6. Bruit : darks indexés par température capteur, DPC coupé, empilement court avec rejet sigma, Siril en post-traitement.

---

## Annexe D : Étude chiffrée : triangulation de l'altitude des aurores avec un réseau Aurion autour de Grenoble

Date : 28/11/2026. Document d'étude, aucun fichier du dépôt Aurion n'a été modifié.

Conventions :
- **[calcul]** : résultat obtenu par calcul dans cette étude (script Python reproductible : `/tmp/claude-0/-home-user-Aurion/b3b66ec5-0ade-59f0-9c66-ee5e0426ba07/scratchpad/calc.py` et `calc2.py`).
- **[source]** : chiffre lu dans une source, URL donnée.
- **[non vérifié]** : ordre de grandeur courant, sans source vérifiée pendant cette étude.

Hypothèses communes : Terre sphérique de rayon R = 6371 km, Grenoble à 45,19°N 5,72°E, aurore placée au méridien de Grenoble (5,72°E), stations au niveau de la mer (l'altitude réelle des stations, 200 à 2000 m, change les résultats de moins de 1 %).

---

### 0. Synthèse

1. Vue de Grenoble, une aurore verte (110 km) au-dessus de 50°N n'apparaît qu'à 9° d'élévation, à 52°N à 5°, à 55°N à moins de 1° : elle est pratiquement sur l'horizon, à 550 à 1100 km de distance oblique. Une aurore rouge à 300 km reste à 10 à 26° d'élévation. [calcul]
2. Un pixel IMX477 vaut 0,57 mrad (1,97 arcmin) avec le 2,7 mm, 0,97 mrad (3,3 arcmin) avec un 1,6 mm ; le double en binning 2x2. La calibration astrométrique sur étoiles atteint 0,5 à 1 arcmin (0,15 à 0,3 mrad) dans les réseaux météores (GMN, FRIPON). [calcul, source]
3. Mais pour une aurore, l'erreur dominante n'est pas l'astrométrie : c'est l'identification d'une même structure dans deux images (1 mrad pour un bord d'arc net ou un rai, 3 à 5 mrad ou plus pour une lueur rouge diffuse). [estimation argumentée]
4. Résultat central [calcul] : l'aurore étant au nord et les arcs étant allongés est-ouest, **seule une ligne de base nord-sud mesure l'altitude d'un arc**. Une ligne de base est-ouest ne voit l'arc que glisser le long de lui-même et ne donne rien (sauf structures ponctuelles : rais, piquets de STEVE, taches pulsantes).
5. Erreur d'altitude d'un arc vert à 110 km situé à 52°N, pour 1 mrad d'erreur de pointage : 96 km (base nord-sud 20 km), 36 km (50 km), 16 km (100 km), 6 km (200 km), 1,5 km (500 km). [calcul]
6. Un groupe d'une dizaine de stations dans un rayon de 50 à 100 km autour de Grenoble **ne suffit pas** pour les aurores basses sur l'horizon nord (erreur 10 km par mrad à 52°N, 86 km par mrad à 55°N). Il est en revanche **efficace pour les arcs rouges SAR et STEVE proches du zénith** (1,5 à 2 km par mrad à 400 km). [calcul]
7. Configuration minimale utile : Grenoble + une station 250 à 400 km plus au nord (Dijon, Besançon, Nancy). Grenoble-Nancy (391 km) donne 2,2 km par mrad pour l'arc vert à 52°N et 2,1 km par mrad pour le rouge à 300 km. Lyon n'apporte presque rien pour les arcs (base surtout est-ouest). [calcul]
8. Horodatage : 0,1 s d'erreur suffit pour la plupart des formes ; viser 1 à 10 ms avec un GPS à sortie PPS, ce qui coûte peu. Le vrai point dur est de déclencher toutes les poses sur une grille UTC commune et de dater le début réel de la pose (pas l'heure de lancement de `rpicam-still`). [calcul]

Recommandation : réseau hybride de 10 stations, 3 à 4 autour de Grenoble (zénith, redondance nuages) et 6 à 7 en chaîne nord-sud jusqu'en Lorraine ou en Alsace (Dijon ou Besançon, Nancy ou Strasbourg, et si possible Reims ou Lille par partenariat), toutes équipées d'un GPS PPS, poses synchronisées, images RAW en FITS avec calibration astrométrique ; la triangulation se fait ensuite sur un serveur, pas sur le Pi.

---

### 1. Géométrie d'observation depuis Grenoble

#### 1.1 Formules [calcul]

Soit θ l'angle au centre de la Terre entre Grenoble (45,2°N) et le pied de l'aurore (latitude L, même méridien) : θ = (L − 45,2°) en radians.

- Distance au sol : D = R θ.
- Élévation apparente géométrique e (sans réfraction) d'un point à l'altitude h :
  tan e = (cos θ − R / (R + h)) / sin θ.
- Distance oblique : s = √(R² + (R + h)² − 2 R (R + h) cos θ).
- Portée maximale pour une élévation minimale e_min : θ_max = arccos(R cos e_min / (R + h)) − e_min.

#### 1.2 Résultats [calcul]

| Altitude h | Latitude L | Distance au sol | Élévation vue de Grenoble | Distance oblique |
|---|---|---|---|---|
| 110 km | 50°N | 534 km | 9,1° | 549 km |
| 110 km | 52°N | 756 km | 4,8° | 770 km |
| 110 km | 55°N | 1090 km | 0,8° | 1103 km |
| 200 km | 50°N | 534 km | 17,8° | 578 km |
| 200 km | 52°N | 756 km | 11,2° | 793 km |
| 200 km | 55°N | 1090 km | 5,3° | 1123 km |
| 300 km | 50°N | 534 km | 26,4° | 623 km |
| 300 km | 52°N | 756 km | 17,8° | 829 km |
| 300 km | 55°N | 1090 km | 10,1° | 1153 km |

Latitude la plus au nord visible au-dessus d'une élévation donnée [calcul] :

| h | élévation ≥ 0° | ≥ 5° | ≥ 10° |
|---|---|---|---|
| 110 km | 55,8°N (1175 km) | 51,9°N (743 km) | 49,7°N (502 km) |
| 200 km | 59,4°N (1576 km) | 55,2°N (1113 km) | 52,5°N (810 km) |
| 300 km | 62,4°N (1918 km) | 58,1°N (1439 km) | 55,1°N (1096 km) |

Conséquences :
- La partie verte (base des rais vers 100 à 120 km) d'une aurore centrée vers 52 à 55°N est sous 5° d'élévation, donc dans la zone où le relief (Chartreuse, Vercors, Bauges au nord de Grenoble), la brume et la pollution lumineuse de l'agglomération dominent. Une station grenobloise doit être en altitude avec un horizon nord dégagé.
- Le sommet rouge des rais (200 à 400 km) monte à 10 à 25°. C'est souvent la seule partie visible depuis la France, ce qui est cohérent avec les aurores « rouges » rapportées en mai 2024 [source : Grandin et al., arXiv 2610.19929, https://arxiv.org/abs/2610.19929, couleurs rapportées majoritairement rouges et roses].

#### 1.3 Réfraction et extinction près de l'horizon [calcul]

Réfraction (formule de Bennett, R' en arcmin = 1 / tan(e + 7,31 / (e + 4,4)), e en degrés) et masse d'air (Kasten-Young) :

| Élévation | Réfraction | en mrad | Masse d'air | Extinction (0,25 mag par masse d'air, hypothèse) |
|---|---|---|---|---|
| 1° | 24' | 7,1 | 26 | 6,6 mag |
| 3° | 14' | 4,2 | 15 | 3,8 mag |
| 5° | 10' | 2,9 | 10 | 2,6 mag |
| 10° | 5,4' | 1,6 | 5,6 | 1,4 mag |
| 20° | 2,7' | 0,8 | 2,9 | 0,7 mag |

La réfraction à 5° (2,9 mrad) est plus grande que toute l'erreur de pointage visée : elle doit être corrigée. Une calibration sur étoiles faite jusqu'à basse élévation l'absorbe en grande partie (les étoiles sont réfractées comme l'aurore) ; le résidu dépend de la température et de la pression (quelques % de la valeur, soit environ 0,1 mrad à 5°). Sous 3°, l'extinction (4 à 7 magnitudes) rend la plupart des aurores invisibles ; on retient **e ≥ 3 à 5°** comme limite pratique.

---

### 2. Résolution angulaire et précision de pointage

#### 2.1 Échelle pixel [calcul]

Projection équidistante (fisheye) : r = f θ, donc un pixel de taille p vaut Δθ = p / f (constant du centre au bord, dans l'approximation).

| Objectif | mrad/pixel | arcmin/pixel | deg/pixel | binning 2x2 | rayon image pour 90° |
|---|---|---|---|---|---|
| 2,7 mm | 0,574 | 1,97' | 0,033° | 3,95' | 4,24 mm |
| 1,8 mm | 0,861 | 2,96' | 0,049° | 5,92' | 2,83 mm |
| 1,6 mm | 0,969 | 3,33' | 0,056° | 6,66' | 2,51 mm |
| 1,4 mm | 1,107 | 3,81' | 0,063° | 7,61' | 2,20 mm |

Capteur IMX477 : 6,29 x 4,71 mm (demi-dimensions 3,14 et 2,36 mm, demi-diagonale 3,93 mm) [calcul à partir de 4056 x 3040 x 1,55 µm]. Avec le 2,7 mm, le champ couvert est d'environ 133° sur le grand axe et 100° sur le petit axe [calcul, approximation équidistante] : le « 140° » est atteint en diagonale seulement. Un 1,6 mm donne un cercle de 180° de rayon 2,51 mm, légèrement coupé en haut et en bas (2,36 mm) ; un 1,4 mm tient entièrement.

Taille d'un pixel projetée sur l'aurore (distance oblique x Δθ) [calcul] :

| Distance oblique | 2,7 mm | 2,7 mm binning 2x2 | 1,6 mm |
|---|---|---|---|
| 700 km | 0,40 km | 0,80 km | 0,68 km |
| 1000 km | 0,57 km | 1,15 km | 0,97 km |
| 1500 km | 0,86 km | 1,72 km | 1,45 km |

L'échantillonnage n'est donc pas le facteur limitant : un pixel couvre moins d'un kilomètre alors que les arcs font 5 à 35 km de large [source : gamme de largeurs utilisée par Whiter et al. 2013, https://gi.copernicus.org/articles/2/131/2013/gi-2-131-2013.pdf].

#### 2.2 Précision de pointage après calibration astrométrique [source]

- Global Meteor Network (GMN) : résidus moyens de l'ajustement astrométrique de 0,19 pixel et 0,78 arcmin avec le modèle de distorsion radiale (termes impairs jusqu'à l'ordre 7) ; sur des images all-sky haute résolution, précision angulaire moyenne de 0,5 arcmin, limitée par la déformation de la PSF au bord du champ. Vida et al. 2021, MNRAS 506, 5046, https://arxiv.org/abs/2107.12335. Pour les objectifs 3,6 mm courants du GMN, environ 1 arcmin, https://arxiv.org/pdf/2206.11365.
- FRIPON (réseau français de caméras fisheye) : modèle astrométrique par caméra précis à environ 2 arcmin (1σ), erreur externe 0,8 arcmin, et mesures possibles à 5° d'élévation avec 1 arcmin soit 0,1 pixel. Jeanne et al. 2019, A&A 627, A78, https://www.aanda.org/articles/aa/abs/2019/07/aa34990-18/aa34990-18.html.
- MIRACLE : je n'ai pas trouvé de chiffre de précision astrométrique publié [non vérifié].

Transposition à l'IMX477 + 2,7 mm [calcul] : 0,1 à 0,2 pixel = 0,06 à 0,11 mrad (0,2 à 0,4 arcmin) en théorie ; en pratique on retient **0,2 à 0,3 mrad (0,7 à 1 arcmin)** comme réaliste après calibration, et environ deux fois plus avec le 1,6 mm ou en binning 2x2.

#### 2.3 Erreur totale de pointage sur une aurore (budget) [estimation argumentée]

| Contribution | Structure fine (bord inférieur d'arc net, rai, piquet STEVE) | Lueur diffuse (rouge, SAR) |
|---|---|---|
| Astrométrie | 0,2 à 0,3 mrad | 0,2 à 0,3 mrad |
| Réfraction résiduelle (e ≈ 5°) | 0,1 mrad | 0,1 mrad |
| Localisation de la structure dans l'image (0,5 à 1 px binning 2x2 pour un bord net) | 0,5 à 1 mrad | 3 à 10 mrad (bord flou sur des dizaines de km) |
| Désynchronisation et flou de pose (voir section 4) | 0,1 à 1 mrad | négligeable (structure lente) |
| **Total retenu** | **≈ 1 mrad** | **≈ 3 à 5 mrad** |

Les tables de la section 3 sont données pour 1 mrad ; elles sont linéaires : multiplier par 3 à 5 pour une lueur diffuse.

---

### 3. Précision d'altitude selon la ligne de base

#### 3.1 Méthode [calcul]

- Chaque station donne une direction de visée (vecteur unitaire u, distance oblique s). Une erreur angulaire σ se traduit par une erreur de position σ s dans les deux directions perpendiculaires à u.
- **Cas « point »** (structure identifiable en deux dimensions : rai, piquet, tache) : chaque station apporte 2 équations ; on résout par moindres carrés pondérés l'inconnue 3D, covariance C = (Aᵀ W A)⁻¹, et on projette C sur la verticale locale pour obtenir σ_h.
- **Cas « arc »** (arc allongé est-ouest, position le long de l'arc inconnue) : chaque station n'apporte qu'une équation, la composante perpendiculaire à l'arc dans le plan image ; les inconnues sont la position nord-sud et l'altitude.
- Base nord-sud : seconde station B km au nord de Grenoble. Base est-ouest : deux stations à ±B/2 de part et d'autre de Grenoble.

#### 3.2 Résultats pour σ = 1 mrad (cas « point ») [calcul]

σ_h en km :

| h, L | B = 20 km NS / EO | 50 km | 100 km | 200 km | 500 km |
|---|---|---|---|---|---|
| 110 km, 50°N | 31 / 5,2 | 11,5 / 2,1 | 5,1 / 1,1 | 2,1 / 0,7 | 0,6 / 0,5 |
| 110 km, 52°N | 96 / 8,5 | 36 / 3,4 | 15,7 / 1,8 | 6,2 / 1,0 | 1,5 / 0,7 |
| 110 km, 55°N | 1005 / 16 | 335 / 6,4 | 128 / 3,3 | 41 / 1,8 | 6,3 / 1,0 |
| 200 km, 50°N | 29 / 9,1 | 10,8 / 3,7 | 4,9 / 1,9 | 2,1 / 1,0 | 0,7 / 0,6 |
| 200 km, 52°N | 69 / 13,8 | 26 / 5,5 | 11,9 / 2,8 | 5,0 / 1,5 | 1,4 / 0,8 |
| 200 km, 55°N | 242 / 23 | 91 / 9,4 | 41 / 4,7 | 17,1 / 2,5 | 4,2 / 1,3 |
| 300 km, 50°N | 31 / 14,2 | 11,8 / 5,7 | 5,5 / 2,9 | 2,4 / 1,5 | 0,8 / 0,8 |
| 300 km, 52°N | 65 / 20 | 25 / 8,1 | 11,4 / 4,1 | 5,0 / 2,1 | 1,5 / 1,1 |
| 300 km, 55°N | 178 / 32 | 68 / 12,9 | 32 / 6,5 | 13,7 / 3,3 | 3,8 / 1,6 |

#### 3.3 Résultats pour σ = 1 mrad (cas « arc est-ouest ») [calcul]

- Base nord-sud : valeurs identiques à la colonne NS ci-dessus.
- Base est-ouest : **erreur infinie** (système singulier) pour toutes les bases, car les deux stations voient l'arc se décaler le long de lui-même.

Lecture : la colonne EO du cas « point » paraît meilleure parce qu'elle exploite la parallaxe en azimut, perpendiculaire à la ligne de visée ; mais cette parallaxe n'est mesurable que sur des structures ponctuelles (rais, piquets, extrémités d'arc, plis). Comme la majorité des arcs sont orientés est-ouest [source : Whiter et al. 2013, citant Syrjäsuo et Donovan 2005], **la base nord-sud est la base de dimensionnement**, la base est-ouest est un complément pour les rais.

Ordre de grandeur utile : pour une base nord-sud, σ_h varie à peu près comme σ s² / B (s : distance oblique, B : base) ; doubler la distance de l'aurore impose de quadrupler la base pour garder la même précision [calcul, approximation de parallaxe].

#### 3.4 Une seule station avec une hauteur supposée [calcul]

C'est ce que fait une caméra isolée qui projette son image sur une carte. Erreur de position au sol pour 10 km d'erreur sur la hauteur supposée :

| h supposée | L = 50°N | 52°N | 55°N |
|---|---|---|---|
| 110 km | 39 km | 47 km | 51 km |
| 200 km | 23 km | 30 km | 35 km |
| 300 km | 16 km | 21 km | 26 km |

Pour l'aurore basse au nord, une incertitude de 30 km sur la hauteur (fréquente entre 100 et 130 km pour la raie verte) déplace l'aurore de 120 à 150 km vers le nord ou le sud sur une carte : c'est l'argument principal pour trianguler.

#### 3.5 Configurations réelles [calcul]

Distances depuis Grenoble : Chambéry 45 km, Lyon 93 km, Genève 117 km, Besançon 229 km, Dijon 243 km, Nancy 391 km, Strasbourg 407 km, Paris 481 km, Lille 636 km.

σ_h en km pour σ = 1 mrad (multiplier par 3 à 5 pour une lueur diffuse). « Réseau local 5 » = Grenoble, Chambéry, Lyon, Valence, Genève.

| Cible | G+Chambéry | G+Lyon | G+Dijon | G+Nancy | G+Lille | Réseau local 5 | G+Lyon+Dijon+Nancy |
|---|---|---|---|---|---|---|---|
| Arc vert 110 km, 52°N (e = 4,8°) | 43 | 25 | 4,8 | 2,2 | 1,1 | 10,3 | (point 0,8) |
| Arc vert 110 km, 55°N (e = 0,8°) | 404 | 187 | 29 | 10,9 | 3,8 | 86 | (point 1,8) |
| Arc rouge 300 km, 50°N (e = 26°) | 14 | 9,0 | 2,0 | 1,1 | 0,7 | 3,5 | (point 0,7) |
| Arc rouge 300 km, 52°N (e = 18°) | 29 | 18,6 | 4,0 | 2,1 | 1,2 | 7,4 | (point 1,3) |
| Arc SAR 400 km au zénith (45,2°N) | 5,4 | n.c. | 1,2 | n.c. | n.c. | 1,5 | (point 2,5 avec G+Lyon) |
| Arc SAR 400 km à 48°N | 8,7 | n.c. | 1,3 | n.c. | n.c. | 2,2 | (point 3,3 avec G+Lyon) |
| STEVE 250 km au-dessus de 46,5°N | 2,7 | n.c. | 0,5 | n.c. | n.c. | 0,7 | (point 1,1 avec G+Lyon) |

(n.c. : non calculé.) Lyon se trouve surtout à l'ouest de Grenoble (0,57° de latitude d'écart seulement) : la paire Grenoble-Lyon est presque est-ouest, donc mauvaise pour les arcs.

#### 3.6 Conclusion de la section 3

- Une dizaine de stations réparties sur 50 à 100 km autour de Grenoble donne, pour un arc vert à 52°N, σ_h ≈ 10 km par mrad, soit 30 à 50 km pour une structure réelle : **insuffisant** pour distinguer 110 de 130 km. À 55°N, inutilisable.
- Ce même groupe local est **adapté aux phénomènes de haute altitude proches du zénith** (SAR, STEVE, aurore rouge débordant jusqu'à 45 à 48°N lors des grandes tempêtes, comme en mai 2024) : 1,5 à 2 km par mrad.
- **Configuration minimale pour l'aurore au nord** : Grenoble + une station 250 à 400 km au nord (Dijon ou Besançon, idéalement Nancy). Avec trois stations Grenoble, Dijon, Nancy, l'erreur est de 2 à 5 km par mrad pour la raie verte à 52°N et environ 2 km par mrad pour la raie rouge. Une station à Lille (636 km) ou en Belgique ramène l'erreur sous 1 à 2 km par mrad, mais l'aurore y est alors vue sous un angle très différent, ce qui complique l'appariement.
- Le précédent historique va dans le même sens : Romick et Belon (1967) utilisaient deux photomètres séparés de 226 km, et MIRACLE travaille sur des paires de 108 km (Abisko-Kilpisjärvi) à 291 km (Kilpisjärvi-Sodankylä) [source pour 226 km et les coordonnées : Whiter et al. 2013 ; distances : calcul], mais avec l'aurore entre les stations, cas bien plus favorable que le nôtre.

---

### 4. Contraintes de mise en œuvre

#### 4.1 Vitesses des structures [source]

- Aurores pulsantes : période de quelques secondes à quelques dizaines de secondes (de moins de 1 s à plusieurs dizaines de s selon la définition), taches de 10 à 200 km, dérive de quelques centaines de m/s à 1 à 5 km/s selon le type et le secteur. Synthèse de recherche, sources : https://eprints.soton.ac.uk/437950/1/ISSIbook_pulsating_and_diffuse_aurora_revise3_nohighlight.pdf et https://arxiv.org/pdf/1806.09428.
- Arcs actifs, rais, ondulations en sous-orage : quelques km/s le long de l'arc [non vérifié pour les valeurs exactes en sous-orage ; ordre de grandeur cohérent avec les 3 km/s d'un cas cité dans la synthèse ci-dessus].
- SAR arcs : structures quasi stationnaires sur des minutes à heures [non vérifié].

#### 4.2 Précision d'horodatage requise [calcul]

Condition : le déplacement de la structure pendant l'écart d'horloge entre deux stations doit rester sous 0,5 mrad, soit Δt_max = 0,5 mrad x s / v.

| Distance oblique | v = 0,3 km/s | 1 km/s | 3 km/s | 5 km/s |
|---|---|---|---|---|
| 550 km | 920 ms | 275 ms | 92 ms | 55 ms |
| 800 km | 1330 ms | 400 ms | 133 ms | 80 ms |
| 1100 km | 1830 ms | 550 ms | 183 ms | 110 ms |

Pour les aurores pulsantes (allumage et extinction en une fraction de seconde), il faut aussi que les poses des différentes stations se recouvrent : décalage des débuts de pose ≪ durée de pose.

Exigence retenue : **écart entre stations ≤ 10 ms**, avec une cible de 1 ms, soit un facteur 5 à 50 de marge. NTP sur Internet donne quelques ms quand il fonctionne, et se dégrade sans réseau [source : https://manual.raspberryshake.org/ntp.html] ; un GPS avec sortie PPS sur un GPIO du Pi, avec chrony, donne une précision de l'ordre de la microseconde [source : https://austinsnerdythings.com/2021/04/19/microsecond-accurate-ntp-with-a-raspberry-pi-and-pps-gps/]. Un GPS USB sans PPS plafonne vers 0,1 ms selon un commentaire non relu [source faible : https://hackaday.com/2022/01/20/ntp-server-gets-time-from-space]. Le GPS est donc recommandé moins pour la précision que pour l'autonomie (sites sans Internet fiable) et la traçabilité.

Point dur logiciel : aujourd'hui Aurion lance `rpicam-still` pour chaque image (`src/adapters/rpi/camera_rpi.rs`) ; l'heure de lancement du processus n'est pas l'heure du début de la pose (délai de démarrage de libcamera, de plusieurs centaines de ms à quelques secondes en pose longue) [non vérifié en mesure, à mesurer sur Pi]. Il faut dater chaque image avec l'horodatage capteur fourni par libcamera (métadonnée SensorTimestamp) converti en UTC, ou déclencher la pose par le signal matériel de déclenchement externe que la documentation Raspberry Pi décrit pour la HQ Camera [non vérifié pendant cette étude, à confirmer]. Le capteur IMX477 est à obturateur déroulant : le décalage entre la première et la dernière ligne, de l'ordre de 0,1 s en pleine définition [non vérifié], est faible devant des poses de 1 à 5 s mais doit être noté dans les métadonnées.

#### 4.3 Durée de pose et flou de mouvement [calcul]

Flou angulaire = v x T_pose / s. Pour une pose de 2 s :

| Distance oblique | v = 0,3 km/s | 1 km/s | 3 km/s | 5 km/s |
|---|---|---|---|---|
| 550 km | 1,1 mrad (1,9 px) | 3,6 mrad (6,3 px) | 10,9 mrad (19 px) | 18 mrad (32 px) |
| 800 km | 0,8 mrad (1,3 px) | 2,5 mrad (4,4 px) | 7,5 mrad (13 px) | 12,5 mrad (22 px) |
| 1100 km | 0,5 mrad (1,0 px) | 1,8 mrad (3,2 px) | 5,5 mrad (9,5 px) | 9,1 mrad (16 px) |

(px : pixels de l'objectif 2,7 mm sans binning.) Le flou est plus gênant que la désynchronisation. Conséquences :
- Poses de **1 à 2 s maximum** pendant une aurore active, 5 à 10 s seulement pour les SAR arcs et lueurs rouges lentes. MIRACLE utilisait 1 s pour le vert et 2 s pour le rouge et le bleu, avec une séquence synchronisée entre stations et une image verte au moins toutes les 20 s [source : Whiter et al. 2013].
- Un objectif plus ouvert (f/1,4 à f/1,2) et le binning 2x2 permettent de raccourcir les poses ; le binning double le pixel (0,57 à 1,15 mrad), ce qui reste en dessous du flou.
- Le flou étant symétrique autour du milieu de pose si la vitesse est constante, l'heure de référence d'une image doit être **le milieu de la pose**, avec la même convention partout.

#### 4.4 Mise en correspondance des images

Méthode de Whiter et al. 2013 [source : https://gi.copernicus.org/articles/2/131/2013/gi-2-131-2013.pdf, FMI, réseau MIRACLE] :
- **Méthode 1, plan horizontal** (reprise d'Ashrafi et al. 2005) : on projette les deux images sur un plan horizontal à l'altitude h, on calcule le coefficient de corrélation de Pearson 2D entre les deux projections, on teste h de 80 à 270 km par pas de 10 km puis par pas de 1 km autour du maximum ; la hauteur retenue est celle qui maximise la corrélation. Biais d'environ 2 km (écart type 1,3 km) sur images synthétiques.
- **Méthode 2, lignes de champ** (nouvelle) : on projette les deux images sur des lignes de champ géomagnétique (IGRF) tirées au hasard (espacement d'environ 5 km à 100 km), on garde les lignes où les deux profils d'émission sont corrélés (r > 0,5 puis r > 0,7 et écart des barycentres < 1 km), on moyenne les profils et on prend le pic. Correcte en moyenne, écart type 4,8 km ; 0,7 km (méthode 1) et 2,9 km (méthode 2) si on exclut les arcs orientés le long de la base (angles 15 à 75°).
- **Hypothèse de hauteur unique par image** (méthode 1) : une seule valeur de hauteur par paire d'images. Les auteurs recommandent la méthode 1 pour les arcs proches de l'horizon ou dans le zénith magnétique d'une station, et pour les aurores larges et plates ; la méthode 2 pour les arcs entre les stations. Ils notent aussi qu'il n'est pas possible d'analyser des arcs situés loin au nord ou au sud des deux stations, et que les échecs correspondent aux arcs alignés avec la base (ambiguïté hauteur-longueur).
- Temps de calcul : 5 à 10 s par paire sur un ordinateur de 2012, compatible avec un traitement serveur.

Application au cas grenoblois [analyse] :
- Notre cas (aurore 500 à 1000 km au nord, basse sur l'horizon, hors de la zone entre les stations) est exactement celui que Whiter et al. jugent difficile. La méthode 1 (plan horizontal, hauteur unique) est la seule transposable, à condition d'une base nord-sud de plusieurs centaines de km ; pour un arc rouge, la corrélation portera sur le bord inférieur ou les rais, pas sur la lueur.
- Pour les structures fines (rais, piquets de STEVE, extrémités d'arcs), la triangulation de points appariés à la main ou par détection de caractéristiques suffit, comme l'ont fait les citoyens pour STEVE (piquets 95 à 150 km, STEVE 130 à 270 km, photos orientées sur les étoiles) [source : Archer et al. 2019, GRL, https://ui.adsabs.harvard.edu/abs/2019GeoRL..4610719A] et pour les « dunes » (couche fine vers 100 km, deux photographes à environ 120 km l'un de l'autre, images prises à la même seconde) [sources : https://doaj.org/article/761ca33eaebb4cec952da0c322a5fa30 et https://www.npr.org/2020/01/30/801195065/a-new-form-of-northern-lights-discovered-in-finland-by-amateur-sky-watchers].
- L'hypothèse de hauteur unique est fausse dans le détail (un rai s'étend de 100 à 300 km et plus) ; il faut publier un résultat par raie (vert : canal G, rouge : canal R du capteur Bayer, avec la contamination croisée connue) et par structure, avec la corrélation obtenue comme indicateur de qualité.

---

### 5. Existant à mutualiser

| Réseau, projet | Ce que c'est | Intérêt pour l'association | Source |
|---|---|---|---|
| MIRACLE (FMI) | 5 à 8 caméras all-sky en Fennoscandie et au Svalbard depuis 1996, filtres 557,7, 630, 427,8 nm, séquences synchronisées ; environ 4 x 10⁵ mesures de hauteur 1996-2007 | Méthode de référence (Whiter 2013), contact scientifique, comparaison | https://gi.copernicus.org/articles/2/131/2013/gi-2-131-2013.pdf ; https://en.ilmatieteenlaitos.fi/news/1216749 |
| ALIS_4D (IRF, Suède) | 5 caméras sensibles espacées d'environ 50 km autour de Kiruna, tomographie, couplage EISCAT_3D | Exemple de conception multi-stations, données d'étalonnage | https://www.irf.se/en/forskning/kago/alis_4d/ |
| FRIPON (France, Europe) | Plus de 100 caméras fisheye, distance médiane 80 km entre caméras, conçu pour les bolides ; calibration fisheye 1 à 2 arcmin | Réseau déjà installé sur toute la France, hébergeurs et calibration réutilisables ; un article de 2026 (Katz et al., Space Weather and Space Climate) l'a utilisé pour suivre l'ovale et un SAR arc en mai 2024 sur l'Europe du Sud | https://www.fripon.org/optic/ ; https://www.aanda.org/articles/aa/abs/2019/07/aa34990-18/aa34990-18.html ; https://www.swsc-journal.org/articles/swsc/abs/2026/01/swsc250042/swsc250042.html (texte complet non lu, accès refusé à l'outil) |
| Global Meteor Network | Caméras Raspberry Pi bon marché, logiciel RMS ouvert, calibration 0,5 à 1 arcmin, triangulation multi-stations | Logiciel et modèle de distorsion directement réutilisables, culture de réseau citoyen | https://arxiv.org/abs/2107.12335 |
| Aurorasaurus (NASA) | Signalements et photos citoyennes, plus de 5000 rapports validés de plus de 50 pays en mai 2024 ; reconstructions stéréoscopiques évoquées | Validation, diffusion grand public | https://meetingorganizer.copernicus.org/EGU26/EGU26-16003.html |
| Skywarden (Taivaanvahti, Ursa, Finlande) | Catalogue d'observations citoyennes utilisé par la recherche (dunes, tempête de mai 2024) | Modèle de base de données citoyenne | https://angeo.copernicus.org/articles/44/855/2026/ ; https://arxiv.org/abs/2610.19929 |
| AuroraWatch UK, BGS | Caméras all-sky Raspberry Pi au Royaume-Uni | Partenaires potentiels pour une base nord-sud très longue | https://aurorawatch.lancs.ac.uk/build ; https://geomag.bgs.ac.uk/education/aurora_cam.html |
| ARCTICS (UNIS) | Coordination de la science citoyenne aurorale | Réseau de contacts | https://www.unis.no/project/arctics-auroral-research-coordination-towards-internationalised-citizen-science/ |
| allskykamera.space | Réseau d'amateurs germanophones de caméras all-sky Raspberry Pi | Stations existantes au nord-est (Allemagne) | https://allskykamera.space/machmit.php?lang=en |
| Triangulation citoyenne STEVE (M. Hunnekuhl) | Méthodes robustes de triangulation de photos citoyennes (étoiles, météores, repères) | Méthode pour photos non synchronisées | https://ui.adsabs.harvard.edu/abs/2021AGUFMSA35F1957H |

Je n'ai pas trouvé de projet amateur européen de triangulation systématique de l'altitude des aurores aux latitudes françaises : le créneau semble libre [constat de recherche, non exhaustif].

---

### 6. Recommandations pour l'association

#### 6.1 Nombre et position des stations

Réseau cible de 10 stations, en deux groupes :

| Rôle | Nombre | Positions types (indicatives) | Justification chiffrée |
|---|---|---|---|
| Groupe zénithal alpin | 3 à 4 | Grenoble (site en altitude, horizon nord dégagé), Chambéry ou Bauges, Valence ou Vercors, Genève ou Annecy | SAR, STEVE, aurore rouge au zénith : 0,5 à 2 km par mrad ; redondance contre les nuages ; vitrine grand public |
| Chaîne nord-sud | 4 à 5 | Lyon nord ou Bourg-en-Bresse, Besançon ou Dijon (230 à 245 km), Nancy ou Strasbourg (390 à 410 km), Reims ou Metz | Aurore au nord : 2 à 5 km par mrad (vert 52°N), environ 2 km par mrad (rouge 300 km) |
| Station lointaine en partenariat | 1 à 2 | Lille, Belgique, Royaume-Uni (AuroraWatch, BGS) ou stations FRIPON existantes | Base 600 km et plus : 1 km par mrad ; sert aussi d'étalon |

Configuration minimale de départ (phase 1) : Grenoble + Dijon ou Besançon + Nancy, soit 3 stations, alignées à peu près nord-sud. Ajouter ensuite le groupe alpin et la station lointaine.

Règles d'implantation : horizon nord dégagé jusqu'à 2 à 3°, faible pollution lumineuse vers le nord, alimentation et réseau stables, coordonnées GPS au mètre.

#### 6.2 Matériel minimum par station (palier « association »)

- Raspberry Pi 5 (ou 4) + HQ Camera IMX477, comme Aurion aujourd'hui.
- **GPS avec sortie PPS** câblée sur un GPIO, antenne extérieure, chrony : horodatage à la microseconde, autonomie sans Internet.
- **Objectif** : deux options.
  - Objectif all-sky 1,4 à 1,8 mm f/1,4 pointé au zénith (champ complet, 0,86 à 1,1 mrad par pixel) : indispensable pour SAR et STEVE au zénith, acceptable au nord.
  - Pour les stations de la chaîne nord-sud, un 2,7 mm (0,57 mrad par pixel) incliné vers le nord avec l'axe vers 30 à 35° d'élévation, pour que l'horizon nord tombe à 55 à 60° de l'axe, loin du bord où la PSF se dégrade (limite signalée par le GMN). Champ de 133° x 100° [calcul].
- Dôme acrylique ou verre, chauffage anti-rosée, mesure température et pression (correction de réfraction).
- Option palier scientifique : filtres interférentiels 557,7 et 630 nm (comme MIRACLE) ou deuxième caméra, pour séparer proprement les raies ; caméra refroidie si le budget de 20 à 50 k€ le permet.

#### 6.3 Format de données

- **Images RAW 12 bits** (pas de JPEG pour la science), en FITS, une extension par canal ou image Bayer brute.
- En-tête FITS minimal : `DATE-OBS` (UTC, début de pose, à la microseconde), `DATE-AVG` (milieu de pose), `EXPTIME`, `GAIN`, `CCD-TEMP`, `SITELAT`, `SITELONG`, `SITEELEV` (WGS84), identifiant de station, objectif, orientation, `TIMESYNC` (source : PPS, NTP, aucune) et écart d'horloge mesuré par chrony, version logicielle, convention obturateur déroulant.
- Fichier de calibration astrométrique par station et par nuit (JSON) : modèle de distorsion radiale impaire (type GMN ou FRIPON), résidus RMS, nombre d'étoiles, date, masque d'horizon.
- Événements : paquet (images ± quelques minutes + métadonnées) envoyé à un serveur commun, plus des aperçus JPEG pour le public.

#### 6.4 Ce qu'Aurion doit implémenter (sur le Pi)

Par ordre de priorité, en gardant l'installation simple pour le grand public (fonctions activables) :
1. **Horodatage fiable** : support chrony + GPS PPS, détection automatique, statut de synchronisation dans l'interface et dans chaque image ; datation par l'horodatage capteur libcamera (ou déclenchement externe), pas par l'heure de lancement de `rpicam-still`.
2. **Poses synchronisées sur une grille UTC** commune (par exemple début de pose à chaque multiple de 5 ou 10 s), identiques sur toutes les stations du réseau, avec un mode « tempête » à poses courtes (1 à 2 s, binning 2x2).
3. **Capture RAW en continu** (pas seulement à la demande) et écriture FITS avec l'en-tête ci-dessus ; sans doute via un pipeline libcamera persistant plutôt qu'un processus par image, ce qui réduit aussi la latence.
4. **Calibration astrométrique automatique** sur étoiles par nuit claire (réutiliser le modèle et le code de RMS du GMN ou d'astrometry.net), avec correction de réfraction à partir de la température et de la pression.
5. **Export réseau** : envoi des paquets d'événements vers un serveur de l'association, configuration de la station (coordonnées, orientation, masque d'horizon).

Côté serveur (pas sur le Pi) : appariement multi-stations, triangulation de points (rais, piquets), méthode 1 de Whiter et al. adaptée (corrélation sur plan horizontal, balayage de 80 à 450 km pour inclure le rouge), estimation d'erreur par la propagation de la section 3, publication d'un catalogue de hauteurs par raie.

#### 6.5 Pièges à anticiper

- Les tempêtes visibles depuis la France sont rares (quelques nuits par cycle solaire) : il faut que tout le réseau fonctionne sans intervention le soir venu ; prévoir des tests avec les météores et les satellites, qui servent aussi à valider la synchronisation (un bolide FRIPON triangulé par deux stations Aurion donne une vérification indépendante).
- La couverture nuageuse rend rare le ciel clair simultané sur Grenoble et Nancy : c'est une raison de plus d'avoir plusieurs stations par zone.
- La partie verte basse de l'aurore est souvent masquée par le relief ou l'extinction : les résultats porteront surtout sur la partie rouge et le sommet des rais.

---

### Annexe : hypothèses et limites des calculs

- Terre sphérique, pas d'ellipsoïde ni de géoïde ; erreur relative de l'ordre de 0,3 % sur les distances.
- Erreurs angulaires gaussiennes, indépendantes entre stations et entre axes ; les erreurs systématiques (orientation de la caméra, réfraction) ne se moyennent pas avec le nombre d'images.
- Le modèle « arc » suppose un arc parfaitement est-ouest au méridien de Grenoble ; un arc incliné rend une partie de la base est-ouest utile.
- Les aurores sont des volumes émissifs transparents, pas des surfaces : la précision réelle d'un « pic d'émission » est limitée par l'épaisseur de la couche (dizaines de km), comme le soulignent Whiter et al.
- Réfraction : formule de Bennett à 10 °C et 1010 hPa ; extinction 0,25 mag par masse d'air choisie comme hypothèse.
