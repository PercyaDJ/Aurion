# Guide expédition : plusieurs nuits sans toucher à la caméra

Version couverte : **1.7.0**. Pour qui : le photographe qui pose Aurion pour une ou deux semaines et veut récupérer
des RAW exploitables pour le timelapse, la retouche des pics d'aurore et l'observation.

Analogie : Aurion devient un piège photographique. On le règle une fois, il travaille chaque nuit, et on relève
les photos quand on repasse.

## 1. Comment se déroule une expédition

| Moment | Raspberry Pi 5 | Raspberry Pi 4 |
|---|---|---|
| Premier soir | réglages depuis le téléphone, **Plusieurs nuits (expédition)** coché, puis on quitte la page | idem |
| Démarrage | la nuit démarre seule 5 min après la dernière utilisation de la page | idem |
| Fin de nuit | photos rangées, `aurores.csv` écrit, **réveil programmé** 10 min avant le début de la nuit suivante (mode plage horaire), extinction | photos rangées, extinction |
| Soir suivant | il se rallume seul, la nuit démarre seule | **rebrancher la batterie** (ou appuyer sur son bouton) : la nuit démarre seule 5 min après |
| Si la nuit est lancée l'après-midi | il s'éteint et se rallume seul avant la nuit (plus de 2 h d'attente) | il attend allumé jusqu'à l'heure de début |
| Coupure pendant la nuit | reprise automatique dans le même dossier, numérotation continue | idem |
| Relève des photos le matin | après le téléchargement, **Éteindre Aurion** : il se rallumera seul le soir | après le téléchargement, **Éteindre Aurion** ; sinon, en quittant la page, il prépare la nuit suivante et attend allumé toute la journée (batterie consommée pour rien) |

Condition indispensable pour démarrer seul : **l'heure doit être juste**.
- Raspberry Pi 5 : horloge intégrée. Sans sa petite pile (ML-2020), elle ne tient l'heure que tant que la batterie
  reste branchée.
- Raspberry Pi 4 : pas d'horloge. L'heure est juste dès qu'un téléphone a ouvert la page depuis l'allumage. Pour
  des soirs sans téléphone, ajouter un module horloge DS3231 (quelques euros, broches I2C) avec
  `dtoverlay=i2c-rtc,ds3231` dans `/boot/firmware/config.txt` : Aurion lit cette horloge au démarrage. Ce montage n'a
  pas encore été validé sur le matériel (PLAN_ACTION.md, V9).

Tant que l'heure n'est pas confirmée, l'accueil l'indique et **la nuit ne démarre pas seule** : mieux vaut aucune
nuit qu'une nuit enregistrée en plein jour.

## 2. Énergie : faire le calcul avec votre mesure

Votre mesure de cet hiver : une batterie de 20 000 mAh finit la nuit à 30 %, donc environ **14 000 mAh par nuit**
dans ces conditions (froid, réglages de l'époque).

| Durée | Énergie nécessaire (même consommation) | Solutions |
|---|---|---|
| 1 nuit | environ 14 000 mAh | une batterie de 20 000 mAh |
| 14 nuits | environ 196 000 mAh | une batterie chargée par nuit (rotation de 2 batteries rechargées le jour), ou recharge solaire |

Aucun réglage logiciel ne fait tenir 14 nuits sur une seule batterie de 20 000 mAh. Ce que fait Aurion :
- **il ne consomme rien le jour** : il s'éteint à la fin de chaque nuit (Pi 5 : il se rallume seul) ;
- la nuit : analyse sur la miniature, aucun ré-encodage, Wi-Fi coupé, Bluetooth, audio et LED coupés ;
- l'arrêt est propre si la batterie faiblit (sous-tension persistante), et la nuit reprend seule quand on la remplace.

À vérifier sur votre matériel : la consommation du Pi 5 éteint en attente de réveil (PLAN_ACTION.md, V8), et si
votre batterie coupe sa sortie quand le Pi s'éteint (beaucoup le font : sur Pi 4 c'est sans conséquence, il faut
de toute façon la rebrancher le soir ; sur Pi 5 il faut une batterie qui garde sa sortie active, ou une
alimentation qui ne se coupe pas).

## 3. Stockage : choisir le format selon la durée

Tailles estimées (remplacées sur l'accueil par les tailles réelles dès la première nuit) : JPEG environ 4 Mo,
DNG environ 24 Mo. À une image toutes les 10 s, au plus 360 images par heure (moins en pratique : le temps de pose
s'ajoute).

| Format | Par heure, au plus | Nuit de 10 h | 14 nuits |
|---|---|---|---|
| RAW + JPG | environ 10 Go | environ 100 Go | environ 1,4 To |
| RAW seul | environ 8,6 Go | environ 86 Go | environ 1,2 To |
| **JPG + RAW des aurores** | environ 1,4 Go, plus 8,6 Go par heure d'aurore | environ 14 Go sans aurore | environ 200 Go, plus les RAW des aurores |
| JPG seul | environ 1,4 Go | environ 14 Go | environ 200 Go |

Pour une expédition, **JPG + RAW des aurores** est le bon compromis :
- le JPEG de toute la nuit sert au timelapse et à l'observation ;
- le RAW est enregistré dès qu'une aurore est détectée et **10 minutes après la dernière détection**, pour garder
  la fin de l'épisode en RAW ;
- chaque RAW a son JPEG jumeau de même nom.

L'accueil affiche l'autonomie de la clé en heures **et en nuits**. Clé conseillée : 256 à 512 Go en exFAT.
Quand la clé est pleine, la nuit s'arrête proprement : aucune photo n'est jamais effacée automatiquement.

## 4. Récupérer les photos : le workflow

Sur la clé USB (ou dans le ZIP d'une nuit), chaque nuit a son dossier :

```text
sessions/2026-01-15_21-00/
    RAW/        les DNG (import Lightroom, LRTimelapse, Darktable)
    JPG/        les JPEG de toute la nuit (timelapse rapide)
    thumbs/     miniatures de la galerie
    aurores.csv les images avec aurore, de la plus forte à la plus faible
    event.jsonl un événement par image : heure, ISO, pose, score
    session.log journal lisible de la nuit
```

- **Toutes les nuits, sans Wi-Fi** : éteindre Aurion, brancher la clé sur l'ordinateur, copier `sessions/`.
  C'est la voie la plus rapide pour des dizaines de Go (le Wi-Fi du Pi est fait pour le contrôle et quelques images).
- **Les plus belles images** : ouvrir `aurores.csv` dans un tableur, la première ligne est l'image la plus forte ;
  les colonnes `jpg` et `raw` donnent les noms des fichiers.
- **Timelapse rapide depuis les JPEG** (les noms commencent par la date et l'heure, l'ordre alphabétique est l'ordre
  chronologique) :

  ```bash
  ffmpeg -framerate 25 -pattern_type glob -i 'JPG/*.jpg' -c:v libx264 -pix_fmt yuv420p nuit.mp4
  ```

- **Depuis le téléphone** : accueil, *Dernière nuit*, **Télécharger les RAW** ; ou *Photos*, *Par session*,
  boutons RAW / JPG / Tout pour chaque nuit.

## 5. Photos bien exposées, bruit minimal en RAW

Réglages conseillés pour une expédition (détail dans [GUIDE_PHOTO.md](GUIDE_PHOTO.md)) :
- **balance des blancs fixe** (daylight par défaut) : aucune variation de teinte d'une image à l'autre ;
- **verrouiller l'exposition** pendant la capture si vous voulez un timelapse sans scintillement (sinon, le lissage
  de l'exposition reste lent en capture : 3 à 5 % par image au plus) ;
- **temps de pose maximal** selon la focale (règle des 500) et la vitesse des aurores : 2 à 8 s pour des aurores
  rapides, jusqu'à 20 à 30 s pour des arcs calmes ;
- **darks** au début ou à la fin du séjour, à la température des nuits ;
- caméra à l'écart du Pi si possible : le capteur froid est le premier levier contre le bruit.

## 6. Check-list avant de partir

1. Carte SD à jour (dernière release), clé USB exFAT vide, 256 Go et plus.
2. Accueil : **Plusieurs nuits (expédition)** coché, format **JPG + RAW des aurores**, plage horaire adaptée à
   la latitude et à la saison.
3. Vérifications de l'accueil sans point rouge ; autonomie de la clé en nuits supérieure à la durée du séjour.
4. Pi 4 : téléphone disponible le soir ou module horloge installé. Pi 5 : pile de l'horloge conseillée.
5. Une nuit d'essai de 30 minutes en conditions réelles avant de partir.
