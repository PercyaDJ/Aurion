# Dossier d'Exploitation (DEX)

Version couverte : **1.13.0**. Public : l'utilisateur averti ou la personne qui maintient les caméras.
Pour une première utilisation sans connaissance technique, lire d'abord [GUIDE_DEMARRAGE.md](GUIDE_DEMARRAGE.md).

## 1. Fiche d'identité

| Élément | Valeur |
|---|---|
| Matériel | Raspberry Pi 4 (2 Go et plus) ou Pi 5, caméra HQ IMX477, clé USB exFAT 128 Go et plus, batterie 5 V / 3 A |
| Système | Raspberry Pi OS Lite 64 bits (Bullseye, Bookworm ou Trixie) |
| Service | `aurion.service` (systemd, `Type=notify`, redémarrage automatique, watchdog 180 s) |
| Utilisateur du service | `aurion` (image carte SD) ou l'utilisateur choisi à l'installation |
| Interface | Wi-Fi `Aurion`, `http://192.168.4.1:8080` |
| Compte de maintenance (image) | `pi` / `aurion`, SSH désactivé par défaut |
| Journaux | `journalctl -u aurion` (en RAM, perdus à l'extinction) et `sessions/*/session.log` sur la clé |

## 2. Installation

| Méthode | Commande ou action | Durée |
|---|---|---|
| Image carte SD (recommandé) | Raspberry Pi Imager, *Utiliser une image personnalisée*, `aurion-raspios-arm64.img.xz`, **Non** aux réglages personnalisés | 10 min + 5 min au premier démarrage |
| Paquet Debian | `sudo apt install ./aurion_X.Y.Z_arm64.deb` | 2 min |
| Depuis un clone | `sudo ./install.sh` | 2 min (binaire précompilé) |
| Depuis un PC | `scripts/deploy.sh utilisateur@aurion.local` ou `scripts\deploy.ps1` | 2 min |

Options de `scripts/install.sh` : `--user`, `--country`, `--no-hardening`, `--no-power-saving`, `--no-packages`,
`--default-wifi-password`, `--no-start`, `--uninstall`. Détail : [DEPLOY_RPI.md](../DEPLOY_RPI.md).

Contrôle après installation :

```bash
systemctl is-active aurion            # active
/opt/aurion/aurion --version          # version installée
/opt/aurion/aurion check-config       # configuration valide
sudo -l -U aurion                     # uniquement /usr/local/sbin/aurion-helper
```

## 3. Exploitation courante : une nuit

| Étape | Action | Contrôle |
|---|---|---|
| 1. Mise en place | trépied, face au nord, mise au point sur l'infini, clé USB branchée, alimentation en dernier | - |
| 2. Connexion | Wi-Fi `Aurion` sur le téléphone ; la page s'ouvre seule | l'heure se synchronise (message « Heure synchronisée ») |
| 3. Cadrage | accueil, *Cadrer (aperçu)* | image nette, horizon bas |
| 4. Vérifications | accueil, *Prêt pour la nuit ?* | aucun point rouge ; autonomie de la clé supérieure à la durée prévue |
| 5. Lancement | mode, durée, format, **Lancer la nuit** | écran « C'est parti », Wi-Fi coupé 15 s après |
| 6. Nuit | aucune action | - |
| 7. Fin | extinction automatique | - |
| 8. Récupération | rallumer, Wi-Fi `Aurion`, accueil *Dernière nuit* : *Télécharger les RAW* ou *Tout télécharger* ; ou clé USB sur un ordinateur | nombre de photos et de RAW cohérent avec la durée |

Ordres de grandeur (réglages par défaut, une image toutes les 10 s plus le temps de pose) : environ 360 images par
heure au plus. Taille par image : JPEG environ 4 Mo, DNG environ 24 Mo (estimations remplacées par les tailles
réelles dès que la clé contient des photos).

### Expédition (plusieurs nuits)

| Étape | Action | Contrôle |
|---|---|---|
| Préparation | accueil : **Plusieurs nuits (expédition)**, format **JPG + RAW des aurores**, plage horaire | ligne « Mode expédition : environ N nuits » supérieure à la durée du séjour ; ligne « Heure » verte |
| Chaque soir, Pi 5 | rien : réveil 10 min avant la plage, départ automatique | - |
| Chaque soir, Pi 4 | brancher une batterie chargée ; sans module horloge, ouvrir la page une fois pour donner l'heure | départ automatique 5 min après avoir quitté la page |
| Relève | clé USB sur un ordinateur : `sessions/<nuit>/RAW`, `JPG`, `aurores.csv` | une nuit par dossier |

Détail et calculs d'énergie et de stockage : [GUIDE_EXPEDITION.md](GUIDE_EXPEDITION.md).

## 4. Supervision

### Depuis le téléphone

| Où | Ce qu'on y voit |
|---|---|
| Accueil | vérifications (caméra, clé, heure, alimentation, température, mot de passe), dernière nuit |
| Diagnostics | version, CPU, mémoire, température, journal en direct, mise à jour, mode maintenance Wi-Fi |
| Stockage (mode expert) | espace libre, état de la clé |

### En SSH (mode maintenance)

SSH est désactivé sur l'image. Pour l'activer : brancher la carte SD sur un ordinateur, créer un fichier vide
`ssh` dans la partition `bootfs`, remettre la carte. Puis, depuis le même réseau :

```bash
ssh pi@aurion.local                      # mot de passe : aurion (le changer : passwd)
journalctl -u aurion -f                  # journal en direct
vcgencmd get_throttled                   # 0x0 = alimentation correcte
vcgencmd measure_temp                    # température CPU
findmnt /mnt/capture                     # clé montée ?
ls /mnt/capture/sessions/                # nuits enregistrées (RAW/, JPG/, aurores.csv)
cat /sys/class/rtc/rtc0/since_epoch      # horloge matérielle présente (Pi 5, DS3231)
tail -n 50 /mnt/capture/sessions/*/session.log
cat /opt/aurion/config/night.json        # présent = une nuit est en cours ou a été interrompue
```

Les journaux système sont en RAM : pour garder le journal d'une nuit, se fier à `session.log` sur la clé.

## 5. Mises à jour

| Méthode | Procédure | Retour arrière |
|---|---|---|
| Depuis GitHub (sans ordinateur) | *Diagnostics*, *Mettre à jour depuis GitHub* : canal stable ou développement, partage de connexion du téléphone. Installe le paquet signé `aurion-update.tar.gz` : programme **et** fichiers système (1.13.0) | bouton *Revenir à la version précédente* |
| Par fichier | *Diagnostics*, *Mise à jour par fichier*, les deux fichiers `aurion-arm64` et `aurion-arm64.sig` de la release | idem (`/opt/aurion/aurion.prev`) |
| Paquet | `sudo apt install ./aurion_X.Y.Z_arm64.deb` | réinstaller le paquet précédent |
| Script | `sudo ./install.sh` depuis un clone à jour | idem |

**Paquet de mise à jour (1.13.0)** : la mise à jour depuis GitHub installe `aurion-update.tar.gz`, qui contient le
programme et les fichiers système dont il a besoin : `aurion-helper` (le seul programme lancé en root), les services
systemd, la règle de la clé USB et les clés publiques. Une nouvelle fonctionnalité qui touche au système arrive donc
sans regraver la carte SD. Déroulé :

1. Aurion copie les réglages sur la clé USB, télécharge le paquet (le téléchargement reprend tout seul après une
   coupure du partage de connexion, pendant quelques minutes), contrôle son empreinte et sa signature.
2. `aurion-helper app-update` (root) **revérifie la signature** avec les clés de `/usr/local/share/aurion/keys`,
   sauvegarde les fichiers qu'il va remplacer dans `/var/lib/aurion/prev/`, puis lance le `install.sh --update` du
   paquet : ni apt, ni `config.txt`, ni réglages touchés. Si l'installation échoue en cours de route, tout est remis
   comme avant. Une nouvelle version qui refuse la configuration en place est refusée (le mot de passe Wi-Fi n'est
   jamais réinitialisé par une mise à jour).
3. Aurion redémarre sur la nouvelle version, à l'essai.

Une caméra en 1.12 (helper version 4) ne sait pas installer le paquet : elle reçoit le programme seul, et
*Diagnostics* signale « helper à mettre à jour ». Il faut alors regraver une fois la carte SD (image 1.13.0 ou plus
récente) ; ensuite, tout arrive par le téléphone.

**Retour arrière automatique (1.12.1, étendu en 1.13.0)** : une version installée démarre « à l'essai » (fichier
`/opt/aurion/aurion.trial`). Si le service échoue 3 fois en 15 minutes, systemd lance `aurion-rollback.service`, qui
remet la version précédente (`aurion.prev`, la version en échec est gardée en `aurion.failed`) et relance le service ;
*Diagnostics* l'annonce au démarrage suivant. Après 3 minutes de fonctionnement, la version est confirmée et le
fichier d'essai disparaît. Une version confirmée qui échoue (matériel, par exemple) n'est jamais remplacée : le
service est simplement relancé.

Depuis la 1.13.0 : après une mise à jour par paquet, le retour arrière remet aussi les fichiers système sauvegardés
(helper, services, règle USB, clés), et il est lancé par le helper **sauvegardé**, pas par le nouveau : un helper cassé
ne peut pas empêcher son propre retour arrière. Une version dont le Wi-Fi Aurion ne démarre pas n'est jamais
confirmée : elle est retirée au bout des 3 minutes, sans quoi le téléphone ne pourrait plus joindre la caméra.
*Diagnostics* affiche la raison du retour. Le bouton *Revenir à la version précédente* remet, lui aussi, programme et
fichiers système quand la version en place vient d'un paquet.

La mise à jour est refusée pendant une nuit. Depuis la 1.12.0, seul un binaire **signé par le projet** est installé,
par GitHub comme par fichier (voir « Signature des versions » ci-dessous). Retour arrière manuel :

```bash
sudo systemctl stop aurion
sudo mv /opt/aurion/aurion.prev /opt/aurion/aurion
sudo systemctl start aurion
```

### Signature des versions

Chaque fichier installable publié (`aurion-update.tar.gz`, `aurion-arm64`) est accompagné de sa signature Ed25519
(`.sig`). Aurion contient deux clés publiques (`keys/`) et refuse tout ce qui n'est pas signé par l'une d'elles ; pour
le paquet, le helper root vérifie de nouveau avec ses propres copies des clés : un tiers qui publierait
une release (compte ou jeton GitHub compromis) ou enverrait un fichier depuis le Wi-Fi Aurion ne peut rien installer.

| Clé | Où est la partie privée | Rôle |
|---|---|---|
| `aurion-signing` | secret `AURION_SIGNING_KEY` de l'environnement GitHub **release** (fichier PEM entier, lignes `BEGIN`/`END` comprises) | signe chaque version dans le workflow Release |
| `aurion-secours` | hors ligne chez le propriétaire, jamais sur GitHub | remplacer la clé principale si elle est perdue ou volée |

- L'environnement **release** (*Settings > Environments*) a le propriétaire comme validateur obligatoire : chaque
  publication (edge comme stable) attend son clic *Approve*. Sans ce clic, rien n'est signé ni publié.
- Le workflow vérifie que le secret correspond à `keys/aurion-signing.pub` avant de publier ; secret absent ou faux :
  la publication s'arrête, aucun binaire non signé ne sort.
- **Clé principale perdue ou compromise** : générer une nouvelle paire, remplacer `keys/aurion-signing.pub` et le
  secret, puis signer **à la main avec la clé de secours** le paquet de cette version et publier cette signature à
  la place de celle du workflow. Les caméras l'acceptent, puis ne font plus confiance qu'aux nouvelles clés (le paquet
  les installe aussi pour le helper) : aucune n'a besoin d'être ouverte.
  `openssl pkeyutl -sign -rawin -inkey aurion-secours.key -in aurion-update.tar.gz -out aurion-update.tar.gz.sig`
- Transition : une caméra en 1.11.x ou avant ne vérifie pas encore les signatures ; elle installe la 1.12.0
  normalement, et n'accepte ensuite que des versions signées.

### Système ou application ?

| Ce qui change | Comment |
|---|---|
| Application seule (cas courant) | depuis le téléphone, *Mettre à jour depuis GitHub* |
| Programme système `aurion-helper`, paquets, réglages du système (Diagnostics signale « programme système plus ancien ») | regraver l'image `aurion-raspios-arm64.img.xz` : les réglages reviennent de la clé USB |

## 6. Sauvegarde et restauration

| Donnée | Emplacement | Sauvegarde |
|---|---|---|
| Photos et journaux de nuit | clé USB | copier la clé sur un ordinateur ; ZIP par nuit depuis l'interface |
| Copie des réglages | clé USB, `aurion-reglages.json` (écrite à chaque enregistrement ; le mot de passe du partage de connexion n'y figure pas) | automatique ; reprise automatique sur une carte SD neuve |
| Configuration | `/opt/aurion/config/aurion.json` | `sudo cp` vers la clé USB ; contient le mot de passe Wi-Fi (fichier 0600) |
| Presets | `/opt/aurion/config/presets/` | idem |

Restaurer : remettre les fichiers à leur place (propriétaire : utilisateur du service, droits 0600), puis
`sudo systemctl restart aurion`. Une configuration invalide est signalée par `aurion check-config` ; l'installeur
la remplace par la configuration par défaut en gardant une copie.

## 7. Incidents et résolution

| Symptôme | Cause probable | Diagnostic | Résolution |
|---|---|---|---|
| Wi-Fi `Aurion` absent | premier démarrage en cours ; service arrêté ; hotspot en échec | attendre 5 min ; SSH : `systemctl status aurion`, `journalctl -u aurion \| grep -i hotspot` | débrancher, rebrancher ; `sudo systemctl restart aurion` |
| Mot de passe Wi-Fi oublié | - | - | fichier vide `aurion-reset-wifi.txt` à la racine de la clé USB, rallumer : mot de passe `aurora2024` |
| Point rouge « Caméra non détectée » | nappe mal insérée ou à l'envers | SSH : `rpicam-hello --list-cameras` | Pi débranché, réinsérer la nappe des deux côtés |
| Point rouge « Clé USB absente » | clé mal branchée ou défectueuse | `findmnt /mnt/capture`, `journalctl \| grep aurion-helper` | rebrancher ; essayer une autre clé |
| Point rouge « Clé USB à préparer » | clé branchée mais illisible (non formatée, ext4…) | `lsblk`, `blkid` | bouton **Préparer la clé** (efface tout, exFAT) ; ou formater sur un ordinateur |
| Wi-Fi Aurion long à apparaître | démarrage lent du système | *Diagnostics*, « Wi-Fi prêt après l'allumage » ; `systemd-analyze blame` en SSH | noter le service le plus lent et le signaler |
| Point orange « Place limitée » | clé presque pleine | accueil : heures de capture possibles | télécharger puis supprimer d'anciennes nuits (Photos, Par session) ou passer en JPG seul |
| Point rouge « Alimentation trop faible » | batterie ou câble insuffisants | `vcgencmd get_throttled` (bit 0) | batterie 5 V / 3 A, câble court et épais |
| Le Pi s'éteint seul la nuit | sous-tension persistante (arrêt de protection) ou clé pleine | `session.log` : dernière ligne ; `night.json` présent = nuit interrompue | batterie plus puissante ; à la remise sous tension, la nuit reprend seule si elle n'est pas finie |
| Expédition : la nuit ne démarre pas seule | heure non confirmée (Pi 4 sans horloge) ou clé absente ; page restée ouverte | accueil, carte « Mode expédition » (raison affichée) | ouvrir la page une fois puis la fermer ; brancher la clé ; module DS3231 pour des soirs sans téléphone |
| « Nuit interrompue » à l'allumage | la nuit précédente a été coupée avant sa fin | `cat /opt/aurion/config/night.json` | laisser reprendre (5 min) ou *Annuler la reprise* |
| Heure fausse | Pi 4 sans horloge sauvegardée, aucun téléphone connecté | accueil, ligne « Heure » | se connecter depuis le téléphone : synchronisation automatique |
| Photos floues ou traînées | mise au point, pose trop longue pour la focale | aperçu | mise au point sur une étoile brillante ; réduire `shutter_max_us` (règle des 500) |
| Aucune photo en mode *Aurores seulement* | aucune aurore détectée (normal) ou seuils trop stricts | `event.jsonl` : champ `aurora_score` | mode *Toute la nuit* pour ne rien rater ; ajuster les seuils (mode expert) |
| Page lente ou figée | téléphone en veille, Wi-Fi faible | - | recharger la page ; se rapprocher du Pi |
| Mise à jour refusée | fichier non conforme (pas un binaire arm64 Aurion) ou nuit en cours | message affiché | télécharger le fichier `aurion` de la release |

## 8. Maintenance préventive

| Fréquence | Action |
|---|---|
| Avant chaque sortie | batterie chargée, clé vidée, objectif propre, essai rapide de 5 min en mode minuteur si le matériel a été modifié |
| Chaque mois d'utilisation | darks à la température de la nuit (GUIDE_PHOTO.md) ; vérifier la place sur la clé |
| À chaque nouvelle version | mise à jour depuis l'interface ; relire CHANGELOG.md |
| Chaque saison | formater la clé en exFAT sur un ordinateur après sauvegarde ; vérifier la nappe caméra |

## 9. Désinstallation

```bash
sudo ./install.sh --uninstall     # retire service, helper, sudoers, règle udev, réglages système
```

La configuration et les photos ne sont pas supprimées.

## 10. Contacts et références

- Architecture : [DAT.md](DAT.md) ; sécurité : [AUDIT_SECURITE.md](AUDIT_SECURITE.md) ;
  énergie et coupures : [ENERGIE_ET_FIABILITE.md](ENERGIE_ET_FIABILITE.md).
- Signaler un problème : onglet *Issues* du dépôt GitHub, avec `session.log` de la nuit concernée.
