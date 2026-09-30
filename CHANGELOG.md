# Journal des versions

## 1.13.0

Objectif : chaque nouvelle fonctionnalité d'Aurion arrive par le téléphone, boîtier fermé, même quand elle touche au
système de la caméra.

- **Paquet de mise à jour complet** : la mise à jour depuis GitHub installe `aurion-update.tar.gz`, qui contient le
  programme **et** ses fichiers système : `aurion-helper` (le seul programme lancé en root), les services systemd, la
  règle de la clé USB, les clés de signature. Avant, seul le programme changeait et un nouveau helper demandait de
  regraver la carte SD.
- **Signature revérifiée en root** : le helper contrôle de nouveau la signature du paquet avec ses propres copies des
  clés, avant d'installer quoi que ce soit.
- **Retour arrière complet** : les fichiers remplacés sont sauvegardés avant l'installation. Une version qui ne démarre
  pas, ou dont le **Wi-Fi Aurion ne démarre pas**, est retirée seule, programme et fichiers système ensemble, et
  *Diagnostics* en donne la raison. Le retour est lancé par le helper sauvegardé : un helper cassé ne peut pas
  l'empêcher. Le bouton *Revenir à la version précédente* remet aussi l'ensemble.
- **Téléchargement qui reprend** : une coupure du partage de connexion de quelques minutes ne fait plus échouer la
  mise à jour ; le téléchargement reprend là où il s'était arrêté.
- **Fenêtre de résultat** : après la mise à jour, la caméra remet son Wi-Fi Aurion ; à la reconnexion du téléphone,
  une fenêtre indique « Mise à jour validée » (après 3 minutes de fonctionnement) ou le retour à la version précédente
  et sa raison, avec le nombre d'avertissements et d'erreurs non bloquants. Pendant les 3 minutes d'essai, un bandeau
  l'annonce et la page se met à jour seule.
- **Journal de mise à jour** : un fichier par mise à jour (carte SD et clé USB), une ligne par étape (`OK`,
  `AVERTISSEMENT`, `ERREUR`), lisible et téléchargeable dans *Diagnostics*.
- **Accès depuis n'importe quel téléphone ou tablette** : l'image nomme le Pi « aurion » (il s'appelait
  « raspberrypi ») et l'annonce sur le réseau. `http://aurion.local` répond sur le Wi-Fi Aurion, le partage de
  connexion du téléphone ou le Wi-Fi de la maison, sans `:8080` : iPhone, iPad, Android 12 et plus. *Diagnostics*
  affiche aussi l'adresse IP du moment.
- Les réglages sont copiés sur la clé USB juste avant chaque mise à jour.
- Une mise à jour ne réinitialise jamais les réglages ni le mot de passe Wi-Fi : une version qui refuserait la
  configuration en place est annulée.
- Décision de sécurité : les paquets du système (Raspberry Pi OS) ne sont plus mis à jour boîtier fermé, la caméra ne
  rejoignant jamais Internet d'elle-même ; chaque nouvelle image les corrige (registre dans `AUDIT_SECURITE.md`).
- **Passage à la 1.13.0** : une caméra en 1.12 (helper version 4) reçoit le programme seul ; il faut regraver une fois
  la carte SD avec l'image 1.13.0 pour que les mises à jour suivantes installent le paquet complet.

### CI et publication
- CI plus rapide, mêmes contrôles : le test de l'installeur réutilise le programme compilé par les tests (il
  recompilait toutes les dépendances, environ 70 s) ; les tests sont compilés légèrement optimisés et sans
  informations de débogage (simulations de nuit environ 3 fois plus rapides) ; l'empaquetage et l'image ne sont
  refaits que si un fichier qui les compose change.
- Une PR qui modifie le code sans monter la version l'annonce dans la CI : à la fusion, rien ne serait publié ni
  envoyé à ARBOR.
- ARBOR : une clé d'envoi absente fait échouer le workflow Release (au lieu d'un simple avertissement).

### Sécurité de l'image
- Plan ARBOR du 30/09/2026 (image 1.12.1) : les 13 CVE openssl signalées sans correctif (dont CVE-2026-84782,
  CVSS 8.2) sont corrigées par Debian en 3.5.7-1~deb13u3, la version déjà présente dans l'image (avis OSV mis à
  jour le 30/09/2026). Aucune montée de version nécessaire : faux positif ARBOR, à relancer.
- La fabrication de l'image échoue si libssl3t64, openssl ou openssl-provider-legacy restent sous
  3.5.7-1~deb13u3 : une image publiée ne peut plus revenir sur une version vulnérable.

## 1.12.1

Objectif : une mise à jour ratée ne rend jamais la caméra inutilisable, boîtier fermé.

- **Retour arrière automatique** : une nouvelle version démarre « à l'essai ». Si elle ne démarre pas (3 échecs du
  service en 15 minutes), systemd remet seul la version précédente et relance la caméra ; la page *Diagnostics*
  l'annonce. Après 3 minutes de fonctionnement, la version est confirmée.
- Une version confirmée qui échoue (caméra débranchée, par exemple) est relancée sans limite, jamais abandonnée.
- Retour arrière manuel : il met fin à l'essai (pas de retour automatique vers la version qu'on vient de quitter).
- Programme système `aurion-helper` en version 4 (commande `app-rollback`) : pour en profiter, la carte SD doit
  porter l'image 1.12.1 ou plus récente (les mises à jour du système depuis le téléphone arrivent avec l'image A/B).

## 1.12.0

Objectif : n'installer que les versions publiées par le projet, première brique d'une caméra mise à jour boîtier
fermé.

- **Versions signées** : chaque binaire publié est signé (Ed25519) par le workflow Release, dans un environnement
  GitHub qui attend l'approbation du propriétaire. La caméra refuse tout binaire non signé ou signé par une autre
  clé, qu'il vienne de GitHub ou d'un envoi depuis le téléphone. Protège d'une release fabriquée par un tiers (compte
  ou jeton GitHub compromis) et d'un binaire envoyé par quelqu'un connecté au Wi-Fi Aurion.
- **Clé de secours** : une seconde clé publique, dont la partie privée reste hors ligne, permet de remplacer la clé
  principale sans ouvrir le boîtier (procédure dans DEX.md § 5).
- **Envoi par fichier** : choisir les deux fichiers de la release, `aurion-arm64` et `aurion-arm64.sig`.
- Passage à la 1.12.0 : une caméra en 1.11.x l'installe normalement ; elle n'accepte ensuite que des versions
  signées.

## 1.11.3

Objectif : fermer les risques de sécurité qui ne demandent pas le matériel.

- **Mise à jour depuis GitHub vérifiée** : le binaire téléchargé est comparé à l'empreinte SHA-256 publiée avec lui
  (`aurion-arm64.sha256`). Empreinte absente ou différente : rien n'est installé. Avant, un téléchargement tronqué ou
  modifié en chemin pouvait être installé.
- **Mot de passe du Wi-Fi de maintenance** (partage de connexion du téléphone) écrit dans un fichier NetworkManager
  réservé à root, comme celui du hotspot depuis la 1.11.0 : il ne passe plus en ligne de commande. Un partage de
  connexion en WPA3 seul reste possible (ancienne méthode en repli).
- **Service durci** : options systemd compatibles avec le helper root (`ProtectHome`, `ProtectKernelLogs`,
  `RestrictNamespaces`…), exposition mesurée par `systemd-analyze security` de 8,8 à 7,5.
- **Image de base figée** : Raspberry Pi OS Lite du 15/09/2026, vérifiée par son empreinte SHA-256 à chaque
  fabrication (avant : « la dernière », sans contrôle). Les mises à jour de sécurité Debian restent appliquées.
- Fin d'une mise à jour depuis GitHub : le résultat est enregistré avant de lever le drapeau « en cours ». Dans
  l'intervalle, la page Diagnostics pouvait lire « interrompue » et cesser de suivre une mise à jour réussie (la
  première fabrication de la 1.11.3 s'est arrêtée sur ce test).
- **10 paquets inutiles retirés de l'image** : outils Python d'installation, strace, htop, v4l-utils,
  wireless-tools, traductions de NetworkManager, man-db, ntfs-3g (les clés NTFS passent par le pilote du noyau).

## 1.11.2

Objectif : un Pi plus réactif et plus sobre, sans rien changer à ce que fait la nuit (les 23 nuits simulées donnent
exactement les mêmes journaux et fichiers qu'en 1.11.1).

### Réactivité
- La boucle de nuit tourne dans sa propre tâche. Avant, elle partageait la même tâche que le serveur web et le chien
  de garde systemd : pendant l'écriture d'un DNG de 20 Mo sur une clé lente, l'interface ne répondait plus et le chien
  de garde se taisait (systemd tue le service au bout de 180 s).
- Écritures et suppressions lourdes sur la clé (images de la nuit, darks, suppression depuis la galerie) faites hors
  des deux fils du serveur web.
- Aperçu : l'auto-exposition analyse la vignette EXIF (320×240) au lieu de décoder l'image 12 MP complète, jusqu'à
  4 fois par aperçu.
- Accueil : l'estimation d'autonomie ne relit plus la dernière nuit sur la clé toutes les 10 s (mise en cache 2 min,
  vidée à chaque suppression).

### Mémoire
- Réglages par défaut : le JPEG de chaque image (environ 5 Mo) et ses pixels ne sont plus copiés avant d'être
  enregistrés.
- Mise à jour envoyée depuis le téléphone : le fichier n'est plus copié une seconde fois (pic d'environ 256 Mo sur un Pi
  qui peut n'avoir que 1 Go), et la limite est la même partout (64 Mo).
- Cache des vignettes (disque en RAM partagé avec les captures) plafonné à 2000 fichiers.

### Photo
- Aperçu et darks utilisent exactement le même gain que la nuit (ils l'arrondissaient à 2 décimales, la nuit à 1) :
  les darks correspondent aux images.
- Darks refusés sans clé USB (ils remplissaient la carte SD).

### Premier démarrage
- LED, Bluetooth, audio et chien de garde matériel sont écrits dans `config.txt` dès la fabrication de l'image : ils
  ne prenaient effet qu'au deuxième démarrage, la première nuit tournait LED allumées près de l'objectif.
- Le premier démarrage n'attend plus `systemd-udev-settle` (obsolète et lent).

## 1.11.1

Objectif : corriger les défauts trouvés à la relecture complète du dépôt (30/09/2026), chacun avec son test.

### Photo
- **Exposition sans scintillement** : la régulation reprenait à chaque image le pas de l'image précédente, et
  l'exposition oscillait sans fin autour de la cible (environ ±20 % en capture, visible dans les timelapses). Le
  premier pas partait aussi toujours à +15 %, même sur un ciel déjà bien exposé. Chaque image corrige maintenant une
  fraction de l'écart, sans dépasser.
- **Sans clé USB, rien n'est écrit sur la carte SD** : le dossier de capture n'était pas vérifié comme point de
  montage, et une clé absente ou arrachée faisait remplir la carte SD (système compris). La nuit continue et
  signale l'erreur à chaque image.

### Lancement de la nuit
- « Lancer la nuit » et le départ automatique d'expédition attendent la fin d'une série de darks, d'un aperçu ou
  d'une mise à jour depuis GitHub. Avant : deux prises de vue en même temps (captures en échec, pause de 5 min), ou
  redémarrage du programme en pleine nuit.
- Une plage horaire dont le début égale la fin est refusée à l'enregistrement : la nuit n'aurait jamais démarré,
  avec le Wi-Fi déjà coupé. Une configuration déjà enregistrée reste chargée au démarrage.

### Mise à jour depuis le téléphone
- Une mise à jour pouvait échouer au hasard (« le nouveau binaire ne démarre pas : Text file busy ») quand le
  serveur lançait un autre programme au même instant, ce qu'il fait en permanence (helper, vcgencmd). L'essai du
  nouveau binaire réessaie maintenant pendant 1 s. Reproduit en test (1 échec sur 65 à 150 passages chargés,
  0 sur 400 après correction).

### Clé USB (helper root)
- Pi démarrant sur un SSD USB : sa partition de démarrage n'est plus montée comme clé photo, et « Préparer la clé »
  ne le compte plus comme une seconde clé.
- « Préparer la clé » refusait la vraie clé quand un disque SATA ou PCIe était listé après elle.

### Installation
- **Installation en une ligne (`get.sh`) réparée** : elle ne trouvait plus l'archive (réponse de l'API GitHub lue
  ligne à ligne) et s'arrêtait sans message. L'empreinte SHA-256 est désormais obligatoire, et le jeton GitHub ne
  passe plus en ligne de commande.
- **Compilation sur le Pi réparée** (`./install.sh` sans archive, `scripts/setup.sh`) : le script appelait un
  fichier inexistant.

### Publication
- Jeton GitHub en écriture limité à l'étape qui déplace le tag `edge` (il restait disponible pendant la compilation
  et les tests).
- Une erreur passagère de l'API GitHub ne refait plus une release existante (seul « introuvable » déclenche une
  publication).
- Une release lance aussi les tests du helper root et ShellCheck (quelques secondes).
- Caches de compilation séparés pour la CI et la release (la CI récupérait le cache arm64 de la release et
  recompilait tout) ; les PR Dependabot ne lancent plus la compilation arm64 ni le test d'image.

## 1.11.0

Non publiée séparément : son contenu est sorti avec la 1.11.1 (tag refusé par GitHub, voir DAT.md § 10).

Objectif : rappel des darks en fin de nuit, derniers points de sécurité fermés, et consommer le moins de minutes GitHub Actions possible
sans perdre l'envoi à ARBOR.

### Rappel « faites vos darks » (PLAN_ACTION Q2)

- En fin de nuit, Aurion calcule les réglages moyens des photos enregistrées : ISO, temps de pose (arrondi à la
  milliseconde), écart minimum et maximum, et température moyenne du processeur quand elle est disponible. Ils sont
  écrits dans `config/dark_reminder.json` et dans le `session.log` de la nuit. Une nuit reprise après une coupure
  complète les moyennes au lieu de les remplacer ; une nuit sans photo enregistrée ne demande pas de darks.
- Au prochain allumage, l'accueil affiche l'encadré **Faites vos darks** avec ces réglages et un bouton qui lance
  10 darks exactement à ces valeurs. L'encadré disparaît quand la série réussit, ou avec *Ignorer*. Une série faite
  depuis la page Cadrage à d'autres réglages le laisse en place.
- API : `GET /api/darks/reminder`, `POST /api/darks/reminder/dismiss`.
- Tests : calcul des moyennes et fusion après reprise, persistance, disparition du rappel (série complète avec un
  faux `rpicam-still`), nuit simulée de bout en bout, et un cas dans le test navigateur.

### Sécurité
- Mot de passe du Wi-Fi Aurion écrit dans un fichier de connexion NetworkManager réservé à root (0600), et non plus
  passé en argument de `nmcli`, où tout processus du Pi pouvait le lire un court instant. Format vérifié avec le
  lecteur de NetworkManager 1.46 (nom du réseau et mot de passe relus à l'identique, espaces et caractères spéciaux
  compris). Si NetworkManager refuse le fichier, l'ancienne méthode prend le relais : le Wi-Fi démarre toujours.
- Registre des risques de l'image : network-manager (CVE-2026-10805, CVE-2025-9615) et sudo (CVE-2026-82474)
  acceptés après lecture des vecteurs CVSS, tous en attaque locale (détail dans AUDIT_SECURITE.md).
- rsync gardé : `raspi-firmware`, indispensable au démarrage du Pi, en dépend. La version corrigée reste imposée à
  la fabrication.

### Code
- Boucle de la nuit découpée en étapes (`orchestrator::run` passe de 620 lignes à une suite d'appels), décisions
  testées une à une : fenêtre de la nuit, extinction avant la nuit (Pi 5), arrêt sur clé pleine, confirmation
  d'aurore, pause après 5 échecs caméra, RAW gardé pendant une aurore, rythme de capture, journal des images
  (15 tests de plus). Aucun changement de comportement : les 23 nuits simulées produisent exactement les mêmes
  journaux, fichiers, extinctions et réveils qu'avant. Prépare l'intervalle adaptatif et les rafales (Q4, Q5).

### Chaîne de publication
- Un seul job par push sur `main` (workflow Release) : binaire edge, et pour une nouvelle version, image, release et
  envoi à ARBOR. Le workflow Edge est fusionné dedans ; une release compile une seule fois (son binaire sert d'edge).
- Envoi du SBOM de l'image à ARBOR dans le job de la release, juste après la publication. Avant, un second
  workflow déclenché par la fin de la release retéléchargeait et remontait l'image de 400 Mo, et affichait
  « Aurion_Image : skipped » sur chaque push, ce qui laissait croire que l'envoi n'avait pas lieu.
- Envoi à ARBOR à chaque release seulement : plus de passage à chaque push ni chaque lundi. Renvoi manuel possible
  (workflow ARBOR, un job), qui reprend le SBOM de l'image publié avec la dernière release (quelques Mo) au lieu de
  retélécharger l'image entière.
- CI sur les pull requests seulement (la même modification était testée deux fois, au push puis à la PR), en un
  seul job, annulée par un nouveau push. Chrome et Node du runner réutilisés (plus de téléchargement du navigateur).
  Compilation arm64 et test de l'image seulement si l'empaquetage change.
- Documentation seule (`docs/`, `*.md`) : aucun workflow lancé. Plus d'artefacts stockés (les SBOM sont dans les
  releases). Dependabot passe en mensuel.

## 1.10.5

Objectif : fermer les 33 vulnérabilités ouvertes sur l'image carte SD (plan ARBOR du 29/09/2026). Aucun changement
de l'application.

- rsync 3.4.1+ds1-5+deb13u4 remplacé par 3.5.0+ds1-0+deb13u1 (correctif Debian trixie-security) : 33 CVE fermées,
  dont 5 critiques (CVE-2026-53790, CVE-2026-70460, CVE-2026-53791, CVE-2026-70452, CVE-2026-53793) et 19 élevées.
  rsync est un outil local, non joignable depuis le Wi-Fi Aurion.
- La fabrication de l'image échoue si rsync reste sous cette version (archive de sécurité Debian non atteinte) :
  une image publiée porte forcément le correctif.

## 1.10.4

Aucun changement de l'application ni de l'image. Version de contrôle de la chaîne de publication :
- actions GitHub figées par empreinte de commit, passées en versions récentes (checkout 7, upload-artifact 7,
  setup-node 7, action-gh-release 3) ; Dependabot avec 7 jours d'attente ;
- registre des risques acceptés de l'image (AUDIT_SECURITE.md).

## 1.10.3

- En-têtes du noyau et compilateur réellement retirés de l'image : Debian les protège du nettoyage automatique, ils
  sont maintenant nommés un par un, et la fabrication échoue s'ils restent. Image 1.10.2 : 387 Mo (748 Mo en 1.10.1).


Objectif : une image plus légère à télécharger et encore moins exposée, sans rien changer à l'application.

- 47 paquets de plus retirés (environ 455 Mo installés) : en-têtes du noyau et leur compilateur, micrologiciels de
  puces Wi-Fi absentes du Raspberry Pi (Atheros, MediaTek, Realtek, Libertas ; le Wi-Fi du Pi, Broadcom, reste),
  PPP, rpi-update, pastebinit, wget, rich et pygments (seul correctif proposé par ARBOR sur l'image).
- Documentation, pages de manuel et traductions du système retirées (environ 190 Mo) ; les licences restent.
- Zones libérées effacées avant compression.
- La fabrication s'arrête si un paquet indispensable disparaît (démarrage, noyaux Pi 4 et Pi 5, Wi-Fi, EEPROM,
  réseau) ou si un outil appelé par Aurion manque.


Objectif : une image plus sûre, suivie en continu par ARBOR.

### Image carte SD allégée et à jour
- Mises à jour de sécurité Debian appliquées à la fabrication de l'image.
- 92 paquets inutiles pour une caméra hors ligne retirés : accès à distance (rpi-connect), cloud-init, compilateurs,
  débogueur, Bluetooth (déjà désactivé), partage SMB, archiveurs, bibliothèques Python et Go associées. La
  fabrication s'arrête si un outil utilisé par Aurion disparaît.
- `hwclock` ajouté (paquet util-linux-extra) : l'heure donnée par le téléphone est de nouveau écrite dans le module
  horloge. Il manquait dans l'image 1.10.0, où cette écriture était ignorée sans erreur.
- Bibliothèque de test `rand` 0.8.6 (RUSTSEC-2026-0097). Elle n'est pas dans le binaire du Pi.

### Suivi des vulnérabilités (CI)
- Nouveau workflow **ARBOR**, deux projets : l'application (SBOM Syft, Semgrep, Trivy) à chaque push sur `main`,
  et le système de l'image carte SD (633 paquets Debian) après chaque release ; les deux chaque lundi.
- Envoi direct par l'API d'ARBOR (`scripts/arbor-upload.sh`), SBOM de l'image par `scripts/image-sbom.sh`.
- Chaque release publie `aurion-sbom.cdx.json` et `aurion-image-sbom.cdx.json`.

## 1.10.0

Objectif : une mise en route du soir la plus courte possible, dans le froid, et une clé neuve qui marche du premier
coup.

### Allumage rapide (hors première installation)
- Le Wi-Fi Aurion démarre **en premier** : la clé USB est montée ensuite, en parallèle (avant, une clé absente ou non
  reconnue retardait le Wi-Fi d'environ 20 s). Sur une carte SD neuve seulement, la clé passe d'abord pour reprendre
  les réglages.
- Services inutiles coupés à l'installation : attente du réseau au démarrage, fichier d'échange sur la carte SD,
  mises à jour `apt` automatiques ; pas d'écran de démarrage.
- Détection de la caméra lancée en arrière-plan au démarrage : la première page s'affiche tout de suite.
- Temps réel mesuré : *Diagnostics*, « Wi-Fi prêt après l'allumage » (et journal).

### Préparer la clé
- Bouton **Préparer la clé** : sur l'accueil quand une clé est branchée mais illisible (non formatée, ext4…), et
  dans *Stockage* (mode expert). Efface la clé et la formate en exFAT (nom AURION) après confirmation avec son
  modèle et sa taille, puis la monte et y copie les réglages.
- `aurion-helper usb-format` : clés USB uniquement (jamais la carte SD, un disque NVMe ou SATA, ni le disque du
  système), une seule clé branchée ; version du helper 3.

## 1.9.0

Objectif : tout-en-un. Un fichier à graver (système + application), la caméra prête en quelques minutes, et une
carte SD qu'on peut regraver sans rien perdre.

- Image au nom fixe **aurion-raspios-arm64.img.xz** : lien permanent vers la dernière version
  (`releases/latest/download/aurion-raspios-arm64.img.xz`), page de release centrée sur ce fichier.
- **Réglages gardés sur la clé USB** (`aurion-reglages.json`, écrit à chaque enregistrement) et repris
  automatiquement au premier démarrage d'une carte SD neuve ; la clé est la mémoire de la caméra.
- Au démarrage, la clé est montée avant le Wi-Fi Aurion : le mot de passe repris s'applique tout de suite ; le
  fichier de réinitialisation du mot de passe marche aussi au premier démarrage.
- Guide : système ou application, quoi mettre à jour et comment ; durées de mise en route.

## 1.8.0

Objectif : RAW seul, photos à la suite, le moins de consommation possible, et des cycles essai / correction de
quelques minutes depuis le téléphone.

### Photo et stockage
- **RAW seul par défaut** (DNG), photos **à la suite** : la pause entre deux photos vaut 0 par défaut, la pose
  choisie par l'exposition automatique donne la cadence. Réglable (*Pause entre deux photos*).
- Miniatures de galerie aussi pour les nuits en RAW seul.
- Autonomie de la clé calculée sur le **débit réel** de la dernière nuit ; taille type d'un DNG de nuit ramenée à
  14 Mo (mesure de terrain 12 à 15 Mo). Correction des chiffres de stockage de la 1.7 (surestimés).
- Durée réelle de chaque prise enregistrée (`capture_ms` dans `event.jsonl`).

### Consommation
- Surveillance (*Aurores seulement*) : plus aucun RAW lu ni écrit tant que l'aurore n'est pas confirmée.
- Profil d'énergie de nuit (`aurion-helper power-profile`) : processeur au minimum en surveillance, normal en
  capture, port Ethernet coupé s'il n'y a pas de câble.
- Option expérimentale **Prise directe** (`rpicam-still --immediate`) à comparer sur le terrain.

### Mises à jour sans ordinateur
- **Mettre à jour depuis GitHub** (*Diagnostics*) : canal stable (dernière release) ou développement (pré-release
  `edge` reconstruite à chaque modification de `main`). Le Pi rejoint le partage de connexion du téléphone, télécharge,
  vérifie, installe, puis revient sur son Wi-Fi ; résultat conservé après le redémarrage.
- **Revenir à la version précédente** en un geste.
- Version affichée avec le commit pour les versions de développement (`1.8.0-edge.xxxxxxx`).
- Le binaire nu `aurion-arm64` est joint à chaque release ; `curl` ajouté aux paquets installés.
- Version du programme système (`aurion-helper version`) vérifiée : *Diagnostics* signale quand l'image ou le
  `.deb` doit être réinstallé.

### Tests
- 220 tests Rust (30 nuits simulées, dont RAW à la suite avec vraie durée de pose, surveillance sans RAW,
  profils d'énergie ; mise à jour contre un faux GitHub, retour arrière), 57 cas du helper, 29 étapes navigateur.

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
