# Aurion

**Aurion transforme un Raspberry Pi et sa caméra HQ en appareil photo autonome pour les aurores boréales.**

On le pose dehors, face au nord, on le règle depuis son téléphone, et il photographie le ciel toute la nuit sans
personne à côté. Le matin, on récupère les photos (RAW pour la retouche et le timelapse), avec les aurores déjà
repérées et classées de la plus forte à la plus faible.

- **Aucun ordinateur sur le terrain** : le Raspberry Pi crée son propre Wi-Fi, tout se pilote depuis le navigateur
  du téléphone, sans application à installer et sans internet.
- **Autonome** : exposition automatique, extinction en fin de nuit, reprise seule après une coupure de courant.
- **Deux modes** : *Toute la nuit* (idéal timelapse, les images avec aurore sont marquées) ou *Aurores seulement*
  (surveille le ciel et n'enregistre qu'à partir de la première aurore confirmée).
- **Plusieurs nuits d'affilée** (mode expédition) : chaque soir, la nuit démarre seule.
- **Mise à jour depuis le téléphone**, avec retour arrière en un geste.

Ce README suffit pour passer du carton à la première nuit d'aurores. Le reste de la documentation est dans
[docs/](docs/README.md).

---

## 1. Le matériel (liste d'achat)

Cette partie suffit pour commander. Elle se lit dans l'ordre : choisir le Raspberry Pi (1.1), commander la liste
correspondante (1.2), vérifier l'objectif (1.3), ajouter ou non les options (1.4).

Prix relevés le 23/11/2026 chez [Kubii](https://www.kubii.com) (revendeur officiel Raspberry Pi en France) et
[LDLC](https://www.ldlc.com). Ils changent avec le temps. Si un article est en rupture, le même produit officiel se
trouve chez les autres revendeurs agréés : bouton **Buy now** de sa page sur
[raspberrypi.com](https://www.raspberrypi.com/products/).

### 1.1 Raspberry Pi 4 ou Raspberry Pi 5 ?

Les deux font exactement les mêmes photos. La différence est ailleurs :

| | Raspberry Pi 4 | Raspberry Pi 5 |
|---|---|---|
| Pour qui | une nuit à la fois, ou plusieurs nuits en rebranchant la batterie chaque soir | partir plusieurs nuits sans revenir à la caméra |
| Ce qui est testé | **une nuit d'hiver complète, avec la HQ Camera en version M12 et une batterie UGREEN Nexode 25 000 mAh** | les mêmes photos ; le rallumage automatique est prévu mais **pas encore vérifié sur le matériel** |
| Le soir suivant | il faut rebrancher la batterie (ou appuyer sur son bouton) | il doit se rallumer seul, avec une batterie qui reste allumée quand le Pi est éteint (beaucoup se coupent) |
| L'heure sans téléphone | perdue à chaque coupure : le téléphone la remet en se connectant | gardée par sa pile d'horloge (option à 6 €) |
| Consommation | plus faible : la batterie dure plus | plus élevée |
| Montage | la nappe de la caméra suffit | une nappe plus petite à acheter (1,32 €) |
| Budget de la liste 1.2 (sans l'alimentation secteur) | environ 335 € | environ 350 € (355 € avec la pile d'horloge) |

**En résumé** : vous débutez ou vous sortez une nuit à la fois, prenez le **Pi 4**, c'est la configuration éprouvée
(Pi 4, HQ Camera M12, batterie UGREEN 25 000 mAh).
Vous voulez laisser la caméra plusieurs nuits seule et acceptez d'essuyer les plâtres, prenez le **Pi 5**.

### 1.2 Liste d'achat (HBOM)

Une ligne = un article à commander, en un exemplaire. « Pareil » : le même article que la colonne d'à côté.

| Élément | Avec un Raspberry Pi 4 | Avec un Raspberry Pi 5 |
|---|---|---|
| **Raspberry Pi** | [Raspberry Pi 4 Model B 4 Go](https://www.kubii.com/en/nano-computers/2772-raspberry-pi-4-model-b-4gb-5056561800349.html) (124,80 €). La version 2 Go suffit aussi | [Raspberry Pi 5 4 Go](https://www.kubii.com/fr/cartes-nano-ordinateurs/4106-1831-raspberry-pi-5-3272496315938.html) (137,40 €). La version 2 Go suffit aussi |
| **Caméra** | [Raspberry Pi HQ Camera, version M12](https://www.kubii.com/en/cameras-sensors/3880-official-hq-camera-m12-mount-5056561803227.html) (58,50 €), celle de la configuration testée. Autre choix : la [version C/CS](https://www.kubii.com/en/cameras-sensors/2950-official-hq-camera-c-to-cs-mount-5056561800127.html) (57,00 €), voir 1.3 | pareil |
| **Objectif** | pour la version M12 : [objectif M12 2,7 mm f/2.5 pour HQ Camera](https://www.kubii.com/en/objectives-supports/3881-1694-m12-mount-lenses-for-official-hq-camera-3272496313767.html) (28,20 €, fisheye 140°). Pour la version C/CS : [objectif officiel 6 mm f/1.2](https://www.kubii.com/en/objectives-supports/2952-official-6mm-wide-angle-lens-3272496301498.html) (28,80 €) | pareil |
| **Nappe caméra** | rien à acheter : fournie avec la caméra | [nappe caméra 200 mm pour Pi 5, réf. SC1128](https://www.kubii.com/en/cables-adapters-converters/4116-1859-csi-mipi-camera-cable-for-raspberry-pi-5-3272496316669.html) (1,32 €) |
| **Carte micro-SD** | [carte SD officielle Raspberry Pi 32 Go](https://www.kubii.com/en/storage-device/4392-2099-official-raspberry-pi-sd-card-3272496319158.html) (33,60 €) | pareil |
| **Clé USB** (les photos) | [SanDisk Ultra Fit 128 Go, USB 3.0](https://www.ldlc.com/fiche/PB00251354.html) (39,25 €) | pareil |
| **Batterie** (sur le terrain) | [UGREEN Nexode 25 000 mAh 145 W, réf. 90597A](https://fr.ugreen.com/products/ugreen-batterie-externe-portable-usb-c-25000-mah-145w-3-sorties-cable) (49,99 € en promotion, 513 g) : **celle testée avec Aurion** | pareil |
| **Alimentation secteur** (à la maison, pour les essais) | [alimentation officielle 15 W USB-C, prise UE](https://www.kubii.com/fr/alimentations/2678-1049-alimentation-officielle-usb-type-c-raspberry-pi-3272496300002.html) (9,60 €) | [alimentation officielle 27 W USB-C, prise UE](https://www.kubii.com/en/power-supplies/4107-1818-power-supply-raspberry-pi-27w-usb-c-3272496315761.html) (12,90 €) |
| **Trépied** | n'importe quel trépied photo : la caméra a un pas de vis standard (1/4 de pouce) | pareil |

À avoir déjà : un **smartphone** (pour tout piloter, sans application à installer) et, une seule fois, un
**ordinateur** qui lit les cartes micro-SD (sinon un lecteur USB de carte micro-SD, quelques euros).

Pourquoi ces choix, pour remplacer un article par un équivalent :
- **Clé USB** : 128 Go ou plus. Une photo RAW pèse environ 24 Mo. Aurion la formate lui-même (bouton *Préparer la
  clé*).
- **Batterie** : elle doit fournir **5 V / 3 A en USB-C**, c'est écrit dans ses caractéristiques (la UGREEN l'indique
  sur ses deux ports USB-C) et faire 20 000 mAh ou plus. Un câble USB-C vers USB-C court et épais (50 cm, 3 A) est
  nécessaire si la batterie n'en fournit pas. Autres batteries qui annoncent 5 V / 3 A en USB-C, **non testées avec
  Aurion** : [Vention 20 000 mAh 100 W](https://www.kubii.com/en/cells-batteries/4910-20000-mah-100-w-external-battery-6922794791206.html)
  (71,95 €, câble de 50 cm fourni), [Belkin 20K Boost Charge](https://www.ldlc.com/fiche/PB00503199.html) (34,75 €).
  Pour un Pi 5 qui doit se rallumer seul, on ne sait pas encore si ces batteries restent allumées quand le Pi
  s'éteint. Les
  batteries perdent 30 à 50 % de leur capacité vers -10 °C : gardez-la au chaud (poche, boîte isolée).
- **Carte micro-SD** : 16 Go ou plus, de marque. La carte officielle est sûre, une autre bonne carte convient.

### 1.3 L'objectif : lequel garder, lequel acheter

La HQ Camera existe en deux versions qui ne diffèrent que par la **monture**, la façon de fixer l'objectif :
**C/CS** (gros objectifs vissés) ou **M12** (petits objectifs). Aurion fonctionne pareil avec les deux. Seul compte
l'objectif, jugé sur deux chiffres écrits dessus ou sur sa fiche : sa **focale** (en mm) et son **ouverture**
(« f/ » suivi d'un nombre).

Pour photographier des aurores, il faut **voir grand** (petite focale) et **capter beaucoup de lumière** (petit
nombre après « f/ »).

**La focale** : plus elle est petite, plus on voit de ciel. Valeurs calculées pour le capteur de la HQ Camera.

| Focale | Équivalent appareil photo classique | Largeur de ciel vue | Pose max avant que les étoiles filent | Pour les aurores |
|---|---|---|---|---|
| 2,1 mm | 12 mm | 113° | 42 s | idéal (très grand champ) |
| 2,7 mm | 15 mm | 99° | 33 s | idéal |
| 3,2 mm | 18 mm | 89° | 28 s | très bien |
| 4 mm | 22 mm | 76° | 22 s | bien |
| 6 mm | 34 mm | 55° | 15 s | correct : champ plus serré, ciel cadré |
| 8 mm | 45 mm | 43° | 11 s | déconseillé : trop serré |
| 12 mm et plus | 67 mm et plus | 29° et moins | 7 s et moins | à éviter |

Les objectifs « fisheye » (œil de poisson) voient plus large que ce tableau. Par exemple, l'objectif M12 2,7 mm
vendu pour la HQ Camera annonce 140° de large.

**L'ouverture** : plus le nombre est petit, plus l'image est lumineuse à temps de pose égal.

| Ouverture | Lumière reçue (référence f/2.0) | Pour les aurores |
|---|---|---|
| f/1.0 à f/1.4 | 2 à 4 fois plus | excellent |
| f/1.6 à f/1.8 | 1,2 à 1,6 fois plus | très bien |
| f/2.0 | référence | bien |
| f/2.5 | 1,6 fois moins | correct |
| f/2.8 | 2 fois moins | limite : aurores faibles difficiles |
| f/4 et plus | 4 fois moins et pire | à éviter |

« Idéal », « correct » et « à éviter » sont des conseils de photographe, pas des mesures faites avec Aurion.

**Vous avez déjà une HQ Camera M12 ?** Gardez-la : c'est la version de la configuration testée. Regardez son objectif dans les deux tableaux : vers 2 à 4 mm et
f/2.5 ou moins, il convient. Sinon, l'objectif M12 [2,7 mm f/2.5 pour HQ Camera](https://www.kubii.com/en/objectives-supports/3881-1694-m12-mount-lenses-for-official-hq-camera-3272496313767.html)
(28,20 €, fisheye 140°) est fait pour elle.

**Vous achetez ?** La version **M12** est celle de la configuration testée ; avec l'objectif 2,7 mm, elle voit beaucoup
de ciel, avec un peu moins de lumière. La version C/CS avec l'objectif officiel 6 mm f/1.2 reçoit plus de
lumière, avec un champ plus serré, mais n'a pas encore été essayée avec Aurion. Les deux conviennent sur le papier.

Dans Aurion, indiquez la focale de votre objectif : menu, cocher **Mode expert**, **Réglages experts**,
**Longueur focale (mm)**. Elle sert seulement à vous prévenir si la pose fait filer les étoiles : une erreur ne bloque
rien.

### 1.4 Les options : à quoi elles servent, faut-il les acheter ?

| Option | Le problème qu'elle règle | Faut-il l'acheter ? | Où |
|---|---|---|---|
| **Pile d'horloge** (Pi 5 seulement) | Le Pi 5 a une horloge, mais elle s'arrête dès qu'on débranche sa batterie. La pile la fait tourner sans courant : le Pi connaît l'heure au rallumage, sans téléphone. Indispensable pour qu'il démarre seul le soir | **Oui si Pi 5 et plusieurs nuits seul.** Non sinon | [pile RTC officielle pour Pi 5, ML-2020](https://www.kubii.com/en/cells-batteries/4110-rtc-battery-for-raspberry-pi-5-5056561803739.html) (6,00 €), se branche sur le connecteur de pile du Pi 5 |
| **Module horloge DS3231** (Pi 4 seulement) | Le Pi 4 n'a pas d'horloge : sans téléphone le soir, il ne sait pas l'heure et la nuit ne démarre pas seule. Ce module lui en ajoute une | **Non si vous débutez.** À brancher sur les broches du Pi et à régler à la main, **pas encore validé avec Aurion** : voir [docs/GUIDE_EXPEDITION.md](docs/GUIDE_EXPEDITION.md) | un module « RTC DS3231 pour Raspberry Pi » avec sa pile ; pas de référence vérifiée à ce jour |
| **Boîtier du Pi** | Protège le Pi de la poussière, des chocs et un peu de l'humidité. La caméra reste à l'extérieur, reliée par la nappe | Conseillé. Vérifiez qu'il laisse passer la nappe caméra | [boîtier officiel Pi 4](https://www.kubii.com/en/cases-mountings/2681-1051-official-case-for-raspberry-pi-4-3272496298583.html) (5,94 €), [boîtier officiel Pi 5 avec ventilateur](https://www.kubii.com/en/ventilated-smart-cases/4108-1903-official-case-for-raspberry-pi-5-with-fan-3272496316638.html) (10,68 €) |

Ces boîtiers ne protègent pas de la pluie ni du givre. Pour dehors toute la nuit, il faut une boîte fermée qui garde
la caméra face au ciel, la batterie au chaud et l'objectif à l'abri de la buée. Le prototype d'Aurion utilise un
boîtier imprimé en 3D en **ASA**, un plastique qui supporte le froid, l'humidité et le soleil. Son fichier sera publié
dans ce dépôt une fois finalisé.

---

## 2. Installation (une seule fois, environ 15 minutes)

### Étape 1 : préparer la carte SD sur l'ordinateur

1. Installez **Raspberry Pi Imager** (gratuit) : <https://www.raspberrypi.com/software/>.
2. Téléchargez l'image Aurion, environ 600 Mo. Elle contient tout : le système du Raspberry Pi et l'application.
   **[aurion-raspios-arm64.img.xz](https://github.com/PercyaDJ/Aurion/releases/latest/download/aurion-raspios-arm64.img.xz)**
   (ce lien donne toujours la dernière version).
3. Ouvrez Raspberry Pi Imager :
   - *Appareil* : votre modèle de Raspberry Pi ;
   - *Système d'exploitation* : tout en bas, **Utiliser une image personnalisée**, puis le fichier téléchargé ;
   - *Stockage* : la carte micro-SD ;
   - à la question des réglages personnalisés, répondez **Non** (Aurion se configure seul).
4. Attendez la fin de l'écriture et de la vérification, puis retirez la carte.

### Étape 2 : tout brancher (Raspberry Pi débranché)

1. **Caméra** : soulevez doucement le loquet du connecteur *CAMERA* du Pi, insérez la nappe à fond, puis rabaissez
   le loquet. Sur un Pi 4, les contacts métalliques de la nappe sont tournés vers les prises HDMI. Faites de même côté
   caméra. Vissez l'objectif sur la caméra.
2. **Carte micro-SD** : dans le lecteur, sous le Pi.
3. **Clé USB** : dans un port USB (bleu de préférence).
4. **Alimentation** : en dernier, sur la prise USB-C.

Les voyants du Pi restent éteints : c'est voulu, pour ne pas éclairer la photo et pour économiser la batterie.

### Étape 3 : premier allumage (3 à 5 minutes)

Au premier démarrage, le Pi s'installe tout seul et peut redémarrer une fois. Ensuite, sur le téléphone :

1. Dans les réglages Wi-Fi, rejoignez le réseau **Aurion**, mot de passe **aurora2024**.
2. La page Aurion s'ouvre toute seule. Sinon, ouvrez le navigateur sur **<http://aurion.local>**
   (ou **<http://192.168.4.1:8080>**).
3. Un encadré **Protégez votre caméra** vous demande de choisir votre propre mot de passe Wi-Fi (10 caractères
   minimum). Faites-le : le mot de passe d'origine est public. Le Wi-Fi Aurion redémarre, reconnectez-vous avec le
   nouveau mot de passe.

L'heure du Pi se règle automatiquement sur celle du téléphone à chaque connexion.

À la première ouverture, la carte **Vérification d'Aurion** se lance seule (environ une minute) : une vraie photo avec
son RAW, enregistrée sur la clé USB et relue, plus la mémoire, l'heure et l'alimentation. Quand elle affiche **Prêt**,
c'est tout : Aurion est opérationnel. Sinon, chaque ligne rouge dit quoi faire. En cas de souci,
*Diagnostics*, **Télécharger le rapport** donne un fichier à envoyer pour obtenir de l'aide.

> **Clé USB neuve ou illisible** : si l'accueil affiche le point rouge « Clé USB à préparer », appuyez sur
> **Préparer la clé**. Attention, tout son contenu est effacé.

---

## 3. Votre première nuit d'aurores

1. **Installez la caméra** dehors, face au nord, bien stable, objectif propre. Faites la mise au point sur
   l'infini (une étoile brillante ou une lumière très lointaine aide).
2. **Allumez** (branchez la batterie) et connectez le téléphone au Wi-Fi **Aurion**.
3. Sur l'accueil, **Cadrer (aperçu)** : vérifiez le cadrage, puis revenez à l'accueil.
4. Lisez la liste **Prêt pour la nuit ?** : elle vérifie la caméra, la clé USB (et combien d'heures de photos elle
   peut contenir), l'heure et l'alimentation. Un point rouge bloque le lancement et dit quoi faire ; un point orange
   est un simple conseil.
5. Choisissez :
   - le **mode** : *Toute la nuit* (conseillé) ou *Aurores seulement* ;
   - la **durée** : la plage horaire habituelle (21:00 à 06:00) ou un nombre d'heures à partir de maintenant ;
   - les **photos** : RAW (conseillé), RAW + JPG, ou JPG seul.
6. Appuyez sur **Lancer la nuit**. Le Wi-Fi se coupe au bout de 15 secondes pour économiser la batterie : c'est
   normal, vous pouvez partir.

Aurion photographie seul et **s'éteint seul** à la fin de la nuit.

Si le courant est coupé pendant la nuit (batterie vide ou changée, câble arraché), rebranchez simplement : Aurion
rallume son Wi-Fi 5 minutes, au cas où vous voudriez annuler, puis reprend la nuit là où elle s'était arrêtée.

Les soirs suivants, c'est encore plus court : branchez la batterie, le téléphone retrouve seul le Wi-Fi Aurion, les
réglages de la veille sont déjà remplis, appuyez sur **Lancer la nuit**.

Conseils de réglage pour le timelapse (intervalle, balance des blancs, exposition verrouillée, darks) :
[docs/GUIDE_PHOTO.md](docs/GUIDE_PHOTO.md).

---

## 4. Récupérer les photos

**Sur le téléphone** : rallumez Aurion et reconnectez-vous au Wi-Fi. L'accueil affiche la **Dernière nuit** (nombre
de photos, d'aurores, de RAW) avec trois boutons : **Voir les plus belles**, **Télécharger les RAW** et
**Tout télécharger**. Le menu *Photos* donne accès à toutes les nuits.

**Sur l'ordinateur** : sur l'accueil, appuyez sur **Éteindre Aurion**, attendez 20 secondes, puis branchez la clé
USB sur l'ordinateur. Chaque nuit a son dossier :

```
sessions/<nuit>/
├── RAW/          photos RAW (DNG)
├── JPG/          photos JPEG
└── aurores.csv   images avec aurore, de la plus forte à la plus faible
```

---

## 5. Plusieurs nuits sans toucher à la caméra

Cochez **Plusieurs nuits (expédition)** sur l'accueil : chaque soir, la nuit démarre seule. Un Raspberry Pi 5
s'éteint le jour et doit se rallumer seul le soir (pile d'horloge et batterie qui reste allumée nécessaires, pas
encore vérifié sur le matériel) ; un Pi 4 demande de rebrancher la batterie. Comptez une batterie
chargée par nuit (environ 14 000 mAh mesurés en hiver).

Tout est expliqué dans [docs/GUIDE_EXPEDITION.md](docs/GUIDE_EXPEDITION.md).

---

## 6. Mettre à jour

| Ce qui change | Comment |
|---|---|
| **L'application** (cas le plus courant) | depuis le téléphone, connecté au Wi-Fi Aurion : *Diagnostics*, **Mettre à jour depuis GitHub**. Choisissez *Stable*, saisissez le nom et le mot de passe du partage de connexion du téléphone, appuyez sur **Mettre à jour maintenant**, puis activez le partage de connexion. Aurion télécharge la nouvelle version (2 à 5 minutes), remet son Wi-Fi, et une fenêtre donne le résultat à la reconnexion. Si la nouvelle version pose problème, elle est retirée seule ; sinon, bouton *Revenir à la version précédente* |
| **Le système** (correctifs de sécurité du Raspberry Pi) | regravez la dernière image sur la carte SD, comme à l'étape 1. *Diagnostics* indique quand c'est nécessaire |

Une mise à jour ne fait rien perdre : les photos restent sur la clé USB, et Aurion y garde une copie de ses réglages
(`aurion-reglages.json`). Après avoir regravé la carte SD, il les reprend tout seul, mot de passe Wi-Fi compris.

---

## 7. En cas de problème

| Symptôme | Solution |
|---|---|
| Le Wi-Fi « Aurion » n'apparaît pas | au premier démarrage, attendez 5 minutes ; sinon débranchez et rebranchez l'alimentation |
| La page ne s'ouvre pas toute seule | tapez <http://aurion.local> ou <http://192.168.4.1:8080> dans le navigateur |
| Point rouge « Caméra non détectée » | débranchez le Pi, vérifiez la nappe des deux côtés (sens et loquet). Pi 5 : nappe 22 vers 15 broches |
| Point rouge « Clé USB absente » | rebranchez la clé, ou essayez-en une autre |
| Point rouge « Clé USB à préparer » | bouton **Préparer la clé** (efface la clé) |
| Point rouge « Alimentation trop faible », ou le Pi s'éteint seul | batterie 5 V / 3 A, câble court et épais |
| Mot de passe Wi-Fi oublié | sur un ordinateur, créez un fichier vide nommé `aurion-reset-wifi.txt` à la racine de la clé USB, remettez la clé sur le Pi et rallumez : le mot de passe redevient **aurora2024** (les photos ne sont pas touchées) |
| « Nuit interrompue » au démarrage alors que vous voulez juste vos photos | appuyez sur **Annuler la reprise** |

Plus de cas, et le diagnostic avancé : [docs/GUIDE_DEMARRAGE.md](docs/GUIDE_DEMARRAGE.md) et
[docs/DEX.md](docs/DEX.md).

---

## Pour aller plus loin

| Document | Contenu |
|---|---|
| [docs/GUIDE_DEMARRAGE.md](docs/GUIDE_DEMARRAGE.md) | le même parcours, avec plus de détails |
| [docs/GUIDE_PHOTO.md](docs/GUIDE_PHOTO.md) | réglages photo : RAW, timelapse, darks, meilleures aurores |
| [docs/GUIDE_EXPEDITION.md](docs/GUIDE_EXPEDITION.md) | plusieurs nuits d'affilée, énergie, horloge |
| [docs/ENERGIE_ET_FIABILITE.md](docs/ENERGIE_ET_FIABILITE.md) | consommation, coupures de courant, conseils terrain |
| [docs/DEX.md](docs/DEX.md) | exploitation : mises à jour, maintenance, SSH |
| [CHANGELOG.md](CHANGELOG.md) | nouveautés de chaque version |
| [docs/README.md](docs/README.md) | toute la documentation (architecture, sécurité, tests) |

<details>
<summary><strong>Installer sur un Raspberry Pi OS déjà en place (utilisateurs avancés)</strong></summary>

L'image ci-dessus est la méthode recommandée. Pour installer Aurion sur un Raspberry Pi OS Lite 64 bits existant
(Bookworm ou Trixie) :

- avec internet, sur le Pi : `curl -fsSL https://raw.githubusercontent.com/PercyaDJ/Aurion/main/scripts/get.sh | sudo bash`
- paquet Debian de la release : `sudo apt install ./aurion_<version>_arm64.deb`
- depuis un clone du dépôt, sur le Pi : `sudo ./install.sh`

Dans ce cas, le mot de passe du Wi-Fi Aurion est tiré au hasard et affiché à la fin de l'installation. Détails :
[DEPLOY_RPI.md](DEPLOY_RPI.md).
</details>

<details>
<summary><strong>Développement</strong></summary>

Aurion est écrit en Rust (serveur web Axum, interface HTML/CSS/JS sans framework, incluse dans le binaire). Son
architecture hexagonale (ports et adaptateurs) permet de tout tester sur PC, avec une caméra simulée.

```bash
cargo run -- serve --port 8080      # interface web avec caméra simulée : http://localhost:8080
cargo run -- simulate               # nuit simulée dans la console
cargo test                          # tests Rust
cargo clippy --all-targets -- -D warnings
bash tests/helper_test.sh           # helper root (validation des arguments)
```

```
src/
├── core/      configuration, exposition, détection, orchestrateur de la nuit
├── ports/     interfaces caméra, stockage, horloge, réseau, système
├── adapters/  pc/ (simulateurs) et rpi/ (rpicam-still, clé USB, point d'accès Wi-Fi)
├── web/       API, sécurité HTTP, galerie, portail captif, pages
└── cli/       simulation, mesures de performance
scripts/       installation, helper root, fabrication de l'image et des paquets
```

Archive Raspberry Pi depuis Linux : `sudo apt install gcc-aarch64-linux-gnu && bash scripts/package.sh` (binaire
statique, fonctionne sur Bullseye, Bookworm et Trixie).

Publier une version : changer `version` dans `Cargo.toml` et fusionner sur `main`. Le workflow Release teste,
fabrique l'image carte SD, publie la release signée et envoie les inventaires de sécurité (SBOM). Sans changement de
version, seul le canal de développement est mis à jour.

Guides : [docs/GUIDE_DEVELOPPEMENT.md](docs/GUIDE_DEVELOPPEMENT.md), [docs/DAT.md](docs/DAT.md) (architecture),
[docs/TESTS.md](docs/TESTS.md), [docs/AUDIT_SECURITE.md](docs/AUDIT_SECURITE.md).
</details>

## Licence

MIT
