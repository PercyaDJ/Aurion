# Guide expédition : plusieurs nuits sans toucher à la caméra

Version couverte : **1.8.0**. Pour qui : le photographe qui pose Aurion pour une ou deux semaines et veut récupérer
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

### Ce qui consomme, et ce qu'Aurion coupe

Un Raspberry Pi n'est pas un appareil photo à microcontrôleur : **il ne sait pas s'endormir entre deux photos**.
Processeur, mémoire, contrôleur USB et régulateurs restent alimentés ; au repos il consomme donc toujours une part
fixe (à mesurer sur votre montage, tâche E1 du plan d'action). Entre deux photos, Aurion ne fait **rien** : aucun
calcul, aucune écriture. Ce qui est coupé :

| Poste | Mesure |
|---|---|
| Wi-Fi | coupé toute la nuit |
| Bluetooth, audio, LED | désactivés à l'installation |
| Port Ethernet | coupé la nuit si aucun câble n'est branché (profil d'énergie) |
| Processeur en surveillance (*Aurores seulement*) | fréquence minimale ; une petite photo JPEG par minute, **jamais de RAW** tant que l'aurore n'est pas confirmée (2 détections de suite) |
| Processeur en capture | fréquence normale (l'outil caméra encode chaque image) |
| Journée | Pi éteint après la nuit (Pi 5 : rallumé seul le soir) |

Aucun réglage logiciel ne fait tenir 14 nuits sur une seule batterie de 20 000 mAh.

À vérifier sur votre matériel : la consommation en surveillance et en capture (E1), celle du Pi 5 éteint en attente
de réveil (V8), et si votre batterie coupe sa sortie quand le Pi s'éteint (beaucoup le font : sur Pi 4 c'est sans
conséquence, il faut de toute façon la rebrancher le soir ; sur Pi 5 il faut une batterie qui garde sa sortie
active).

## 3. Stockage : RAW seul, photos à la suite

Par défaut, Aurion enregistre **uniquement le RAW** (DNG) et prend les photos **à la suite** : chaque pose dure le
temps choisi par l'exposition automatique, puis la suivante démarre (pas de rafale, pas de pause). La cadence dépend
donc de la lumière : poses courtes pendant une aurore forte, longues par ciel sombre.

Correction : dans la version 1.7, j'avais annoncé environ 100 Go par nuit. C'était faux. J'étais parti d'un DNG
de 24 Mo, d'un JPEG en plus et d'une photo toutes les 10 s sans compter le temps de pose. Avec votre mesure (DNG de
nuit de 12 à 15 Mo, 14 Mo retenus) et environ 2 s de délai de l'outil caméra par prise (hypothèse, mesurée
maintenant à chaque photo, champ `capture_ms`) :

| Temps de pose | Photos par heure | Par heure (DNG 14 Mo) | Nuit de 10 h |
|---|---|---|---|
| 20 s (ciel sombre) | environ 160 | environ 2,3 Go | environ 23 Go |
| 15 s | environ 210 | environ 3 Go | environ 30 Go |
| 5 s (aurore) | environ 510 | environ 7,2 Go | - |
| 2 s (aurore forte) | environ 900 | environ 12,6 Go | - |

Pour 14 nuits en mode *Toute la nuit*, compter quelques centaines de Go (plus s'il y a beaucoup d'aurores fortes) :
clé ou disque USB de 512 Go conseillé. En mode *Aurores seulement*, l'enregistrement ne commence qu'à la première
aurore confirmée (puis continue jusqu'à la fin de la nuit) : rien n'est écrit les nuits sans aurore.

Dès la première nuit, l'accueil calcule l'autonomie de la clé **sur le débit réel** de la dernière nuit (tailles et
cadence mesurées), en heures et en nuits. Quand la clé est pleine, la nuit s'arrête proprement : aucune photo n'est
jamais effacée automatiquement.

Pour espacer les photos (moins de fichiers), *Réglages photo*, **Pause entre deux photos**. D'autres formats
restent disponibles (*Réglages photo*) : RAW + JPG, JPG seul, JPG + RAW des aurores.

## 4. Récupérer les photos : le workflow

Sur la clé USB (ou dans le ZIP d'une nuit), chaque nuit a son dossier :

```text
sessions/2026-01-15_21-00/
    RAW/        les DNG (import Lightroom, LRTimelapse, Darktable)
    JPG/        les JPEG, si un format avec JPEG est choisi
    thumbs/     miniatures de la galerie
    aurores.csv les images avec aurore, de la plus forte à la plus faible
    event.jsonl un événement par image : heure, ISO, pose, score
    session.log journal lisible de la nuit
```

- **Toutes les nuits, sans Wi-Fi** : éteindre Aurion, brancher la clé sur l'ordinateur, copier `sessions/`.
  C'est la voie la plus rapide pour des dizaines de Go (le Wi-Fi du Pi est fait pour le contrôle et quelques images).
- **Les plus belles images** : ouvrir `aurores.csv` dans un tableur, la première ligne est l'image la plus forte ;
  les colonnes `jpg` et `raw` donnent les noms des fichiers.
- **Timelapse rapide depuis les JPEG** (formats avec JPEG ; les noms commencent par la date et l'heure, l'ordre
  alphabétique est l'ordre chronologique) :

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

1. Carte SD à jour (dernière release), clé ou disque USB exFAT vide, 512 Go conseillés.
2. Accueil : **Plusieurs nuits (expédition)** coché, format **RAW**, plage horaire adaptée à la latitude et à la
   saison.
3. Vérifications de l'accueil sans point rouge ; autonomie de la clé en nuits supérieure à la durée du séjour.
4. Pi 4 : téléphone disponible le soir ou module horloge installé. Pi 5 : pile de l'horloge conseillée.
5. Une nuit d'essai de 30 minutes en conditions réelles avant de partir.
