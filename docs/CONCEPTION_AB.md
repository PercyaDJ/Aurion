# Mise à jour du système A/B (note de conception)

Statut : **reportée** le 30/09/2026. Décision : boîtier fermé, seule l'application Aurion et ses fichiers système sont mis à jour (paquet signé, 1.13.0) ; les paquets du système restent en risque accepté (`AUDIT_SECURITE.md`). Cette note reste la base si la mise à jour complète du système redevient nécessaire (par exemple avec le Pi 5).

## 1. Objectif

Une caméra en boîtier fermé doit pouvoir mettre à jour **tout son système** (Raspberry Pi OS, noyau, paquets
corrigés des CVE signalées par ARBOR, et Aurion) depuis le téléphone, sans jamais devenir inutilisable, même si la
nouvelle version ne démarre pas ou si le courant est coupé au mauvais moment.

Aujourd'hui (1.12.1) :

- l'**application** se met à jour depuis le téléphone, signée (Ed25519), avec retour arrière automatique ;
- le **système** ne se met à jour qu'en regravant la carte SD, donc en ouvrant le boîtier.

## 2. Principe

Deux systèmes complets cohabitent sur la carte SD : **A** et **B**. La caméra tourne sur l'un, la mise à jour
s'écrit dans l'autre, qui ne sert pas.

Analogie : deux chambres. On dort dans la chambre A pendant qu'on aménage la chambre B. On essaie ensuite une nuit
dans B avec un billet valable **une seule nuit**. Si la nuit se passe bien, on déménage officiellement. Si quoi que ce
soit tourne mal (plantage, blocage, coupure de courant), le billet a expiré au réveil suivant et on se retrouve
automatiquement dans A, intacte.

Ce « billet d'une nuit » est une fonction du chargeur de démarrage des Raspberry Pi 4B et 5 (celui en EEPROM) :

- `autoboot.txt`, sur la première partition, désigne la partition de démarrage normale (`boot_partition`) et celle à
  essayer (`[tryboot]`) ;
- `sudo reboot "0 tryboot"` redémarre **une fois** sur la partition d'essai. Le drapeau est effacé par le
  redémarrage suivant, quel qu'il soit : le retour vers le système confirmé ne dépend d'aucun logiciel du nouveau
  système.

Exemple tiré de la documentation Raspberry Pi (config.txt, section autoboot.txt) :

```ini
[all]
tryboot_a_b=1
boot_partition=2
[tryboot]
boot_partition=3
```

Avec `tryboot_a_b=1`, chaque partition garde son `config.txt` normal : la bascule se fait par partition entière, pas
par fichier.

## 3. Découpage de la carte SD

Table MBR (celle de Raspberry Pi OS, outillée par `sfdisk` et `parted` dans `build-image.sh`) :

| Partition | Type | Taille | Contenu |
|---|---|---|---|
| p1 | FAT32 | 64 Mo | `autoboot.txt` seul |
| p2 | FAT32 | 512 Mo | démarrage A (noyau, firmware, `config.txt`, `cmdline.txt` → racine A) |
| p3 | FAT32 | 512 Mo | démarrage B (idem → racine B) |
| p4 | étendue | reste | contient p5 et p6 |
| p5 | ext4 | 5 Go | système A |
| p6 | ext4 | 5 Go | système B |

- Chaque `cmdline.txt` désigne **sa** racine par `root=PARTUUID=<id>-05` ou `-06` : pas de fichier à modifier au
  moment de la bascule.
- Carte SD minimale : **16 Go** (environ 11,2 Go utilisés). Photos et réglages restent sur la clé USB, comme
  aujourd'hui.
- L'image publiée contient déjà ce découpage (B vide). L'agrandissement automatique de Raspberry Pi OS au premier
  démarrage est désactivé : la disposition est figée dès la fabrication. Le fichier `.img.xz` grossit peu, car les
  zones vides se compressent presque entièrement.
- Aucune caméra n'est encore déployée : la première image A/B sera simplement celle gravée à l'installation. Pas de
  migration à écrire.

## 4. Ce que publie une release

En plus des fichiers actuels :

| Fichier | Rôle |
|---|---|
| `aurion-system-root.ext4.xz` | système de fichiers racine, réduit à sa taille utile (`resize2fs -M`) |
| `aurion-system-boot.tar.xz` | fichiers de la partition de démarrage (sans `cmdline.txt`, écrit par la caméra) |
| `aurion-system.manifest` | version, taille et SHA-256 de chaque fichier décompressé |
| `aurion-system.manifest.sig` | signature Ed25519 du manifeste (même clé que le binaire) |

Une seule signature couvre tout le système : la caméra vérifie le manifeste, puis chaque empreinte pendant
l'écriture. Ces fichiers sont produits par la même étape que l'image carte SD : quelques secondes d'Actions en plus,
pas une nouvelle construction.

## 5. Ce que met à jour une mise à jour du système

Une seule action depuis le téléphone met tout à jour :

| Élément | Où il vit | Mis à jour par |
|---|---|---|
| Raspberry Pi OS et tous ses paquets (correctifs CVE signalés par ARBOR) | racine du système inactif | écriture de la racine (§ 7, étape 4) |
| Noyau, firmware de démarrage (`start*.elf`, `fixup*.dat`), overlays, `config.txt` | partition de démarrage du système inactif | écriture de la partition de démarrage (étape 5) |
| Pilote de la caméra, `libcamera`, fichiers de réglage du capteur IMX477 | racine | écriture de la racine |
| Application Aurion et `aurion-helper` | racine | écriture de la racine |
| Chargeur de démarrage en EEPROM | puce EEPROM, hors carte SD | étape à part, après confirmation (§ 9) |

La caméra HQ (IMX477) elle-même n'a pas, à ma connaissance, de micrologiciel propre qu'on puisse mettre à jour :
tout ce qui la pilote est dans le système, donc mis à jour avec lui. À confirmer lors de l'essai V17.

## 6. Le téléphone apporte Internet

La caméra n'a pas d'autre accès à Internet que le téléphone. Deux cas, selon qui émet le Wi-Fi :

1. **Le téléphone émet** (partage de connexion) : la caméra s'y connecte comme au Wi-Fi de la maison (mode
   maintenance, déjà en place) et télécharge elle-même. C'est le cas fiable, à privilégier.
2. **La caméra émet** (Wi-Fi Aurion) : le téléphone garde ses données mobiles pour lui, il ne les partage pas avec
   la caméra. Faire télécharger les fichiers par la page, dans le navigateur, puis les envoyer à la caméra n'est pas
   possible : vérifié le 30/09/2026, GitHub sert les fichiers de release sans l'en-tête `Access-Control-Allow-Origin`
   (seule son API l'envoie), donc le navigateur refuse de les lire. Dans ce cas, la page explique comment activer le
   partage de connexion et y connecter la caméra, en deux gestes.

Une coupure de quelques minutes ne fait rien perdre : le téléchargement reprend là où il s'était
arrêté (§ 7, étape 3).

## 7. Déroulé d'une mise à jour du système

Côté téléphone : *Diagnostics*, *Mettre à jour le système*. Autorisé hors nuit, et sur secteur ou batterie
suffisante. Chaque étape est écrite dans le journal de mise à jour (§ 10).

1. **Sauvegarder** les réglages sur la clé USB (sauvegarde déjà existante, faite ici de force).
2. **Vérifier** : télécharger le manifeste et sa signature, refuser si la signature n'est pas faite par une des deux
   clés du projet, ou si la version n'est pas plus récente.
3. **Télécharger** les fichiers compressés (environ 400 Mo) sur la clé USB, qui a la place. Reprise automatique
   après une coupure réseau (téléchargement par morceaux), sans limite de durée tant que l'utilisateur n'annule pas.
   Empreinte contrôlée à la fin : fausse, le fichier est jeté et retéléchargé.
4. **Écrire le système inactif** : décompression de la clé USB vers la racine inactive, empreinte recontrôlée à
   l'écriture, agrandissement à la taille de la partition (`resize2fs`), `e2fsck`.
5. **Préparer son démarrage** : formater sa partition de démarrage, y extraire l'archive, écrire son `cmdline.txt`
   (sa racine, `panic=10`).
6. **Reporter l'état** du système actif vers le nouveau (§ 8).
7. **Essayer** : noter « essai du système X » dans l'état du système actif, puis `reboot "0 tryboot"`.

Coupure de courant pendant les étapes 1 à 6 : le système actif redémarre normalement, rien n'a changé pour lui ; la
mise à jour reprend au prochain lancement sans retélécharger ce qui est déjà sur la clé USB.

## 8. Ce qui est recopié d'un système à l'autre

Recopié à l'étape 6 (quelques Ko) :

- réglages Aurion (`/opt/aurion/config`), dont le mot de passe Wi-Fi changé dans l'application ;
- connexions Wi-Fi enregistrées (`/etc/NetworkManager/system-connections`), dont le partage de connexion du
  téléphone ;
- nom d'hôte, fuseau horaire.

La clé USB n'est pas touchée (photos, darks), sauf pour recevoir la sauvegarde des réglages, le téléchargement et le
journal.

Recopier plutôt que partager une partition : l'ancien système reste exactement tel qu'il était avant l'essai. Un
retour arrière retrouve donc ses propres réglages, jamais des réglages écrits par une version plus récente.

## 9. Confirmation ou retour

Au démarrage, Aurion compare la racine sur laquelle il tourne (`/proc/cmdline`) à celle que `autoboot.txt` désigne
comme normale. S'ils diffèrent, c'est un essai.

**Essai réussi** : Aurion et sa page répondent pendant 3 minutes sans interruption (la caméra n'est pas exigée).
Aurion rend alors le nouveau système permanent (réécriture de `autoboot.txt`, puis `sync`), puis :

- met à jour l'EEPROM si la release le demande, avec l'outil officiel `rpi-eeprom-update` (voir § 11) ;
- remonte son Wi-Fi Aurion (s'il était connecté au partage de connexion du téléphone pour télécharger) ;
- à la prochaine ouverture de la page, affiche une fenêtre « Mise à jour vers X validée », avec le nombre
  d'avertissements et un lien vers le journal (§ 10).

Le téléphone se reconnecte de lui-même au Wi-Fi Aurion s'il l'a déjà enregistré. Sinon, il suffit de le choisir
dans les réglages Wi-Fi.

**Essai raté**, quelle qu'en soit la forme :

- noyau qui ne démarre pas, `panic` : redémarrage automatique (`panic=10`) ;
- système bloqué : le chien de garde matériel, déjà activé (`dtparam=watchdog=on`, `RuntimeWatchdogSec=15s`),
  redémarre le Pi ;
- Aurion qui ne tient pas 3 minutes : Aurion écrit la raison dans le journal, puis demande un redémarrage normal ;
- coupure de courant pendant l'essai.

Dans tous ces cas, le redémarrage suivant se fait **sans** le drapeau d'essai, donc sur l'ancien système, jamais
modifié. Il voit sa note « essai du système X » sans confirmation, et la fenêtre affiche « Mise à jour vers X
échouée, retour à la version Y » avec la raison connue et le lien vers le journal. La note est effacée : pas de
nouvel essai automatique en boucle.

La raison est connue quand le nouveau système a eu le temps de l'écrire (Aurion qui ne démarre pas, service en
échec). Si le noyau n'a jamais démarré ou si le système s'est bloqué, le message l'explique ainsi : « le nouveau
système n'a pas démarré ».

## 10. Journal de mise à jour

- Un fichier par mise à jour, sur la clé USB (`aurion-maj/AAAA-MM-JJ_HHMM_X.log`) et en copie sur la carte SD. La
  clé USB le garde même si la carte SD était regravée.
- Chaque étape y écrit début, fin et durée, puis `OK`, `AVERTISSEMENT` ou `ERREUR`. Un avertissement n'arrête pas la
  mise à jour : par exemple une reprise de téléchargement, une EEPROM déjà à jour, ou un réglage inconnu ignoré.
- Le nouveau système ajoute au même fichier ses propres étapes (démarrage, confirmation, EEPROM).
- *Diagnostics*, rubrique *Mises à jour* : la liste des journaux, chacun lisible et téléchargeable, avec en tête le
  résumé (résultat, version de départ et d'arrivée, nombre d'avertissements et d'erreurs).

## 11. Risques et parades

| Risque | Parade | Reste à vérifier |
|---|---|---|
| Coupure pendant la réécriture de `autoboot.txt` à la confirmation | fichier de quelques octets, écrit 3 minutes après un démarrage réussi (fenêtre de l'ordre de la milliseconde) ; écrit dans un fichier temporaire, relu, puis renommé | comportement du chargeur si `autoboot.txt` est illisible : essai V17 |
| Chargeur EEPROM trop ancien pour `tryboot_a_b` | la version minimale n'est pas donnée par la documentation ; l'image embarque `rpi-eeprom` à jour | essai V17 sur un Pi 4 et un Pi 5 |
| Mise à jour de l'EEPROM | faite seulement après confirmation du nouveau système, et seulement si la release la demande ; la documentation Raspberry Pi indique une mise à jour A/B de l'EEPROM sur le Pi 5 uniquement : sur le Pi 4, c'est la seule étape sans retour arrière complet | voir § 13 |
| Usure de la carte SD | une écriture de la taille utile du système (environ 2,5 Go) par mise à jour, soit quelques-unes par an | |
| Version plus ancienne publiée par erreur | refusée (étape 2), sauf retour arrière volontaire depuis *Diagnostics* | |
| Clé USB pleine | la mise à jour vérifie la place (environ 1 Go) avant de commencer, et le dit sinon | |

## 12. Tests

- **CI** : l'image construite est contrôlée sans Pi (partitions, `autoboot.txt`, `cmdline.txt` de chaque côté,
  manifeste signé et vérifié). Le déroulé des § 7 à 10 est testé sur des fichiers qui simulent les partitions,
  comme `aurion-helper` aujourd'hui (mode simulé), y compris coupure réseau et reprise.
- **Sur le Pi, avant de fermer le boîtier (V17)** : mise à jour réussie par partage de connexion du téléphone ;
  coupure du partage pendant le téléchargement ; mise à jour vers un système volontairement cassé (noyau absent,
  Aurion qui ne démarre pas) avec retour et raison affichée ; coupure de courant pendant l'écriture, puis pendant
  l'essai ; sur un Pi 4 et, si possible, un Pi 5.

## 13. Découpage en PR

1. **Image A/B** : `build-image.sh` produit le découpage du § 3, `autoboot.txt`, deux `cmdline.txt` ; premier
   démarrage adapté. Rien ne change encore pour l'utilisateur.
2. **Release** : fichiers du § 4, manifeste signé dans l'étape de signature existante.
3. **Caméra** : téléchargement avec reprise, commandes `aurion-helper` (écrire le système inactif, essayer,
   confirmer, EEPROM), logique Aurion (§ 7 et 9), journal (§ 10), bouton et fenêtre de résultat.
4. **Documentation** : DEX, guide de démarrage, V17 dans le plan d'action.

## 14. Décisions

Prises le 30/09/2026 :

1. Réglages **recopiés** d'un système à l'autre, sauvegardés sur la clé USB avant la mise à jour.
2. Mise à jour **en ligne uniquement**, par le partage de connexion du téléphone (§ 6).
3. Carte SD de **16 Go minimum** (16, 32 ou 128 Go).
4. Tout se met à jour d'un coup : système, noyau et firmware, pilote de la caméra, application.
5. Confirmation : Aurion répond pendant 3 minutes. En cas d'échec, retour automatique avec la raison. En cas de
   succès, le Wi-Fi Aurion revient et une fenêtre confirme, avec le lien vers le journal, les avertissements et les
   erreurs non bloquantes.

Encore ouvert :

1. **EEPROM sur le Pi 4** (§ 11) : la mettre à jour quand une release le demande (seule étape sans retour arrière
   complet), ou ne jamais y toucher sur un boîtier fermé. Recommandation : la mettre à jour seulement si la release
   corrige un défaut qui touche Aurion.
