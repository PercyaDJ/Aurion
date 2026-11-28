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

**Rappel automatique après la nuit.** En fin de nuit, Aurion calcule les réglages moyens des photos enregistrées
(ISO, temps de pose et température) et les écrit dans `session.log`. La température est celle du capteur, lue dans
les métadonnées de chaque photo (`SensorTemperature`, depuis la version 1.16.0) ; si la caméra ne la donne pas, Aurion
prend celle du processeur, voisine, et l'encadré l'indique (*capteur* ou *processeur*). Au prochain allumage, l'accueil affiche l'encadré **Faites vos darks** avec ces valeurs :
bouchon sur l'objectif, le bouton *Lancer 10 darks* les enregistre dans `darks/` à ces réglages exacts. L'encadré
disparaît dès que la série est terminée, ou avec *Ignorer*. Une série faite depuis la page *Cadrage* à d'autres
réglages ne l'efface pas. Si l'exposition a varié pendant la nuit (exposition non verrouillée), l'encadré le signale
avec l'écart minimum et maximum : les darks à la moyenne restent utiles, mais une exposition verrouillée donne des
darks exactement adaptés. Une nuit sans photo enregistrée (mode *Aurores seulement* sans aurore) ne demande pas de
darks.

Idéal : faire les darks dès l'allumage du matin, dehors, avant que la caméra ne se réchauffe. La température affichée
sert de repère ; elle n'est pas disponible sur un PC ou en simulation.

Logiciels qui les utilisent : Sequator (Windows), Siril (Windows, macOS, Linux), PixInsight, Starry Landscape Stacker (macOS).

**Master dark calculé par Aurion** (depuis la version 1.16.0). À la fin d'une série réussie, Aurion calcule sur le
Pi, à partir des DNG de la série :
- `darks/master/master_dark_ISO<iso>_<pose>s_T<température>.fits` : la médiane pixel à pixel des darks, en valeurs
  brutes du capteur (niveau de noir compris, comme dans les DNG), avant tout dématriçage. La température du nom est
  celle du capteur arrondie à 5 °C (`T35`), `Tinconnue` si elle n'est pas connue. L'en-tête FITS contient `EXPTIME`,
  `ISOSPEED`, `CCD-TEMP`, `DATE-OBS` (UTC), `BAYERPAT`, `INSTRUME`, `STACKCNT` (nombre de darks) et `ROWORDER`.
- `darks/master/hot_pixels_ISO<iso>_<pose>s_T<température>.txt` : la liste des pixels chauds, ceux dont la valeur
  dépasse la médiane de l'image + k fois l'écart absolu médian (MAD, au moins 1). k vaut 6 par défaut (paramètre
  `hot_k` de `POST /api/darks`, de 2 à 50). Format texte : lignes de commentaire commençant par `#` (règle, médiane,
  seuil, nombre), puis une ligne `x y value` par pixel, coordonnées dans le DNG depuis le coin haut gauche.

Les DNG d'origine ne sont jamais modifiés. Le calcul se lit sur la page (*Calcul du master dark : x %*) ; le bouchon
peut être retiré dès que les poses sont terminées. En cas d'échec du calcul, les DNG restent utilisables comme avant.

**Utiliser le master dark dans Siril.** Le FITS s'utilise comme un master dark déjà empilé : il n'est pas nécessaire
de réempiler les DNG de `darks/`.
- Il ne convient qu'aux images prises au même ISO, au même temps de pose et à une température proche (le nom du
  fichier les donne).
- Siril accepte un master dark avec la commande `calibrate` et son option `-dark=` (fichier du master), et sait
  détecter les pixels chauds à partir de ce master avec l'option `-cc=dark`. Options vérifiées dans le code source de
  Siril (`src/core/command_def.h`, novembre 2026) ; pour la syntaxe complète d'un script, se reporter à la
  documentation de Siril. Dans l'interface graphique, le master dark se choisit dans l'étape de prétraitement ; les
  libellés dépendent de la version de Siril.
- Le fichier des pixels chauds d'Aurion n'est pas au format « bad pixel map » de Siril (option `-cc=bpm`) : il sert
  de rapport lisible et pour de futurs traitements d'Aurion.
- Sens de l'image : le FITS est écrit lignes du bas vers le haut, avec `ROWORDER = 'BOTTOM-UP'`, comme Siril range
  lui-même un RAW qu'il ouvre (lecture vérifiée dans son code source). Le master doit donc se superposer aux images
  que Siril convertit depuis les DNG. **À valider** sur une vraie nuit : après calibration, les pixels chauds doivent
  disparaître et non se dédoubler.

### Levier 4 : empilement au post-traitement
Pour une image fixe très propre : empiler 4 à 8 RAW consécutifs dans Sequator ou Siril (le bruit baisse d'environ √N).
Aurion peut aussi produire un JPEG empilé (`_STACKn.jpg`, réglage *Empilement JPEG*) pour un aperçu immédiat.
Depuis la version 1.16.0, la moyenne se fait en lumière linéaire et rejette, pour chaque pixel, la valeur la plus
haute puis la plus basse seulement si elles s'écartent de plus de 2,5 écarts types des autres : avions et satellites
disparaissent sans jeter les bonnes images. Les images ne sont pas recalées sur les étoiles (prévu au lot 2b) : avec
des poses longues, les étoiles de l'image empilée sont filées. Désactivé par défaut (calcul, mémoire et batterie).

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
