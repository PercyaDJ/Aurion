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

## 1. Le matériel

### Indispensable

| Élément | Choix conseillé | Remarques |
|---|---|---|
| **Raspberry Pi** | Raspberry Pi 4 (2 Go de mémoire ou plus) ou Raspberry Pi 5 | voir « Pi 4 ou Pi 5 ? » ci-dessous |
| **Caméra** | Raspberry Pi **HQ Camera** (capteur Sony IMX477) | c'est la seule caméra prise en charge |
| **Objectif** | grand angle et lumineux, monture C ou CS (celle de la HQ Camera) | le ciel entier compte : la focale la plus courte possible, l'ouverture la plus grande possible (par exemple l'objectif officiel 6 mm CS) |
| **Nappe caméra** | Pi 4 : celle fournie avec la caméra. **Pi 5 : une nappe « 22 broches vers 15 broches »** | le Pi 5 a un connecteur caméra plus petit : la nappe d'origine ne rentre pas |
| **Carte micro-SD** | 16 Go ou plus | reçoit le système et l'application |
| **Clé USB** | 128 Go ou plus, formatée en **exFAT** | reçoit les photos et une copie des réglages. Une photo RAW pèse environ 24 Mo |
| **Alimentation** | batterie USB-C **5 V / 3 A**, avec un câble court et épais | une batterie de 20 000 mAh a tenu une nuit d'hiver complète (il restait 30 %). Sur secteur : l'alimentation officielle du Pi |
| **Un téléphone** | n'importe quel smartphone avec un navigateur | pour tout piloter |
| **Un ordinateur** | une seule fois | pour préparer la carte SD |

À prévoir aussi : un support stable (trépied), et de quoi protéger le montage de l'humidité et du froid. Les
batteries perdent 30 à 50 % de leur capacité vers -10 °C : gardez-la au chaud (poche, boîte isolée).

### Pi 4 ou Pi 5 ?

| | Raspberry Pi 4 | Raspberry Pi 5 |
|---|---|---|
| Une nuit à la fois | parfait | parfait |
| Plusieurs nuits d'affilée | il faut rebrancher la batterie (ou appuyer sur son bouton) chaque soir | **se rallume seul** chaque soir grâce à son horloge intégrée |
| Consommation au repos | plus faible | plus élevée |
| Nappe caméra | celle de la caméra | nappe 22 vers 15 broches à acheter |

### En option

| Élément | Pour quoi |
|---|---|
| Pile d'horloge **ML-2020** (Pi 5) | garder l'heure quand la batterie est débranchée : utile en mode expédition |
| Module horloge **DS3231** (Pi 4) | démarrer seul à la bonne heure sans téléphone. Montage pas encore validé sur le matériel : voir [docs/GUIDE_EXPEDITION.md](docs/GUIDE_EXPEDITION.md) |

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

L'heure du Pi se règle automatiquement sur celle du téléphone à chaque connexion. C'est tout : Aurion est prêt.

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
s'éteint le jour et se rallume seul le soir ; un Pi 4 demande de rebrancher la batterie. Comptez une batterie
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
