# Guide photo : bruit sur les RAW, timelapse et retouche

Ce guide explique comment Aurion est réglé pour un usage de photographe d'aurores : timelapse sur toute la nuit,
et retouche unitaire des images les plus fortes à partir des RAW.

## 1. Le principe : le RAW reste brut

Le DNG enregistré est exactement celui du capteur : **aucun traitement d'Aurion ne le modifie**, et le débruitage du
processeur d'image (`--denoise`) ne s'applique qu'au JPEG. Sur un RAW, le bruit se gagne à la prise de vue et se
retire au post-traitement. Aurion agit donc sur quatre leviers.

### Levier 1 : capteur froid
Le bruit thermique (courant d'obscurité) augmente avec la température du capteur, et la caméra HQ est vissée à côté
du processeur du Pi. Aurion garde le CPU quasiment au repos :
- analyse sur la miniature EXIF 320×240 au lieu de l'image 12 Mpx (environ 230 fois moins de calcul) ;
- aucun décodage ni ré-encodage des photos par défaut ;
- Wi-Fi coupé pendant la nuit.

Conseil matériel : écarter la caméra du Pi (nappe plus longue) ou isoler thermiquement les deux.

### Levier 2 : exposition « à droite », sans étoiles filées
Plus de lumière collectée = meilleur rapport signal/bruit. L'exposition automatique allonge d'abord le temps de pose,
et ne monte l'ISO qu'ensuite. Bornez :
- **temps de pose maximal** : règle des 500 (avertissement dans le tableau de bord) ; pour des aurores rapides, 2 à 8 s ;
- **ISO maximal** : 1600 à 3200 sur l'IMX477 ; au-delà le bruit augmente plus vite que le signal.

### Levier 3 : darks
Page *Cadrage (aperçu)*, section *Darks* : bouchon sur l'objectif, la caméra enregistre 10 à 30 poses noires en RAW **aux
réglages de la dernière preview** (même ISO, même temps de pose), dans `darks/` sur la clé. Soustraites au
post-traitement, elles retirent pixels chauds et bruit thermique des DNG. À faire juste avant ou juste après la nuit,
caméra à la même température.

Logiciels qui les utilisent : Sequator (Windows), Siril (Windows, macOS, Linux), PixInsight, Starry Landscape Stacker (macOS).

### Levier 4 : empilement au post-traitement
Pour une image fixe très propre : empiler 4 à 8 RAW consécutifs dans Sequator ou Siril (le bruit baisse d'environ √N).
Aurion peut aussi produire un JPEG empilé (`_STACKn.jpg`, réglage *Empilement JPEG*) pour un aperçu immédiat, avec
rejet des valeurs extrêmes : avions et satellites disparaissent. Désactivé par défaut (calcul et batterie).

## 2. Réglages timelapse

| Réglage | Recommandation | Pourquoi |
|---|---|---|
| Format | RAW seul (défaut) | DNG pour la retouche et le timelapse ; les miniatures de la galerie sont gardées. RAW + JPG si vous voulez aussi des JPEG |
| Balance des blancs | Lumière du jour (défaut) | identique sur toute la séquence : pas de scintillement de couleur, même balance « as shot » dans tous les DNG |
| Verrouiller l'exposition pendant la capture | activé pour une séquence continue | zéro scintillement ; la rampe jour/nuit se fait ensuite (LRTimelapse, Lightroom) |
| Intervalle | 5 à 10 s pour une aurore active, 20 à 30 s sinon | fluidité contre place disque |
| Mode | SAFE (toute la nuit) | FILTER ne commence qu'après détection et peut manquer le début |

Sans verrou, l'exposition s'adapte lentement (3 à 5 % par image au maximum) : acceptable pour un timelapse, avec un
léger lissage en post-traitement. Pour une exposition totalement manuelle : ISO min = ISO max et pose min = pose max.

## 3. Retrouver les aurores les plus fortes

*Photos*, onglet **Meilleures aurores** (ou accueil, *Voir les plus belles*) : les images sont classées par score de détection (force et étendue de la
couleur verte, rouge ou violette dans le ciel). Le bouton **Télécharger les RAW (top)** récupère en ZIP les DNG et JPEG
des 30 meilleures, prêts pour une retouche unitaire.

Le score est aussi enregistré pour chaque image dans `sessions/<nuit>/event.jsonl` (champ `aurora_score`, et
`frame_number` qui correspond au numéro dans le nom du fichier).

## 4. Ce qui reste à valider sur le terrain

Les réglages par défaut reposent sur la documentation de `rpicam-still` et sur des images synthétiques. À confirmer
lors des premières nuits : seuils de détection (la zone analysée est désormais réellement de 65 % du ciel), taille réelle
des DNG, et apport effectif des darks sur votre capteur. Voir PLAN_ACTION.md.
