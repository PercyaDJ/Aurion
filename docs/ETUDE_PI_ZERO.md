# Étude : une caméra Aurion plus compacte avec un Raspberry Pi Zero 2 W

Statut : **étude sur papier, rien n'est testé sur le matériel**. Faits vérifiés le 25/11/2026 sur les pages citées en
fin de document ; tout le reste est marqué « à vérifier ».

## En bref

- **C'est plausible** : le Zero 2 W fait tourner le même système qu'Aurion, se branche sur la HQ Camera et son
  Wi-Fi 2,4 GHz est celui qu'utilise le point d'accès d'Aurion.
- **Trois inconnues décident de tout** : la mémoire (512 Mo) suffit-elle pour une photo pleine résolution avec RAW,
  le temps par photo, et le point d'accès Wi-Fi sur cette carte. Aucune ne se tranche sur papier.
- **Le Pi n'est pas ce qui rend la caméra grosse : c'est la batterie.** La batterie UGREEN mesure
  160 × 80 × 26 mm et pèse 513 g ; un Pi 4 mesure 85 × 56 mm, un Zero 2 W 65 × 30 mm. Changer de carte sans changer de
  batterie, c'est changer le moteur d'une voiture qui tire une caravane : la caravane reste. Le Zero ne rend la caméra
  vraiment compacte que s'il consomme assez peu pour qu'une batterie plus petite tienne la nuit. Sa consommation n'est
  pas publiée : il faut la mesurer.
- **Recommandation** : l'expédition de mars se fait avec le **Pi 4 actuel**, la configuration éprouvée. Le Zero est un
  essai à part, sur une carte SD de test, pour un coût d'environ 30 € (étape 1 ci-dessous). Rien n'est commandé de plus
  tant que l'essai n'a pas répondu oui.

## Ce que le Zero 2 W change

| | Raspberry Pi 4 (actuel) | Raspberry Pi Zero 2 W |
|---|---|---|
| Taille de la carte | 85 × 56 mm | **65 × 30 mm** |
| Processeur | 4 cœurs Cortex-A72 1,5 à 1,8 GHz selon la révision | 4 cœurs Cortex-A53 1 GHz : plus lent, à mesurer |
| Mémoire | 2 à 8 Go | **512 Mo** |
| Wi-Fi | 2,4 et 5 GHz | 2,4 GHz seulement : c'est la bande du point d'accès Aurion |
| Prise caméra | standard (nappe fournie avec la caméra) | mini 22 broches : **nappe « Pi Zero » à acheter** |
| Prises USB | 4 ports USB-A | un seul micro-USB de données : **adaptateur OTG** pour la clé USB |
| Alimentation | USB-C | **micro-USB** (5 V / 2,5 A) : câble USB-C vers micro-USB depuis la batterie |
| Système | Raspberry Pi OS 64 bits | le même : le Zero 2 W est dans la liste officielle des cartes compatibles |
| Plage de température | à vérifier | −20 à +70 °C (fiche officielle) |
| Prix | 124,80 € (4 Go) | 22,50 € |

## Ce que dit le code d'Aurion

Audit du dépôt, pour savoir ce qu'une carte de 512 Mo pourrait bloquer :

| Point | Constat | Verdict |
|---|---|---|
| Analyse de chaque photo (exposition, détection d'aurore) | Aurion ne décode que la petite vignette intégrée au JPEG, jamais l'image de 12 Mpx | léger |
| Enregistrement avec les réglages par défaut | le JPEG et le DNG sont écrits tels quels, sans recopie des pixels | léger |
| Fichiers temporaires | `/tmp` est en mémoire, plafonné à 200 Mo ; une photo y passe avec son DNG (environ 30 Mo) | correct |
| **Empilement de photos et retrait des pixels chauds** (options, désactivées par défaut) | décodage pleine image et tampons d'empilement : de l'ordre de 250 Mo | **à éviter sur Zero** |
| Point d'accès Wi-Fi | configuré en 2,4 GHz (`802-11-wireless.band bg`) | compatible sur le papier |
| Détection de la carte | seul le Pi 5 est reconnu à part (réveil automatique) ; un Zero se comporte comme un Pi 4 | rien à changer |
| `rpicam-still` en 4056 × 3040 avec RAW sur 512 Mo | non documenté par Raspberry Pi | **à mesurer** |

Aucune modification de code n'est nécessaire pour faire l'essai : l'image Aurion actuelle doit démarrer telle quelle sur
un Zero 2 W (même système 64 bits, même noyau que le Pi 4). C'est justement ce que l'étape 1 vérifie.

## Essayer sans perdre d'argent

Le principe : dépenser le moins possible pour répondre d'abord aux trois inconnues, et s'arrêter dès qu'une réponse est
non. Ta HQ Camera M12, son objectif, la batterie et la clé USB servent tels quels.

### Étape 1 : l'essai de bureau (environ 30 €)

| Achat | Prix | Remarque |
|---|---|---|
| [Raspberry Pi Zero 2 W](https://www.kubii.com/fr/cartes-nano-ordinateurs/3455-2111-raspberry-pi-zero-2-w-3272496326088.html) | 22,50 € | la page affiche « en stock » mais une réponse du vendeur dit le contraire : vérifier avant de payer |
| [Câble pour caméra, Pi Zero Edition](https://www.kubii.com/fr/nappes-cables-adaptateurs/1830-cable-pour-camera-pi-zero-edition-kubii-3272496006768.html) | 3,95 € | la documentation officielle impose une nappe « Zero » pour la HQ Camera |
| Adaptateur OTG **micro-USB mâle vers USB-A femelle** | 2 à 5 € | vérifier le sens : la fiche micro-USB va dans le Zero, la clé USB dans l'adaptateur |
| Câble USB-C vers micro-USB | quelques euros | pour la batterie et les essais sur secteur |
| Carte SD de test | déjà prévue pour le protocole Pi 5 | la carte EXPÉDITION ne bouge pas |

Puis, sur la carte SD de test gravée avec la dernière image Aurion, Zero **débranché** pour brancher la nappe :

| Test | Comment | Réussi si |
|---|---|---|
| A. Démarrage et point d'accès | brancher, attendre 5 min | le Wi-Fi **Aurion** apparaît et la page s'ouvre |
| B. Caméra | *Cadrage (aperçu)* | l'image s'affiche |
| C. Photo pleine résolution avec RAW | nuit en minuteur de 30 min, photos **RAW + JPG** | aucune interruption ; les fichiers RAW sont complets sur la clé |
| D. Temps par photo | comparer l'heure dans le nom des fichiers successifs | l'écart reste proche de la pose réglée, comme sur le Pi 4 |
| E. Consommation | testeur USB-C entre la batterie et le Zero, en capture | à noter : c'est le chiffre qui dit si une petite batterie suffit |

Un seul « non » parmi A à D arrête l'essai. La perte se limite alors à environ 30 €, et le Zero reste utilisable pour
un autre projet. Si tout est « oui », envoie-moi les résultats : je corrige ce qui doit l'être dans Aurion.

### Étape 2 : seulement si l'étape 1 est réussie

1. Une nuit complète dehors, sur la batterie UGREEN, avec le pourcentage de batterie relevé le matin.
2. Avec la consommation mesurée en E, je calcule la batterie la plus petite qui tient une nuit d'hiver avec de la
   marge. C'est elle qui fixe la taille finale de la caméra.
3. Alors seulement : un boîtier ASA dessiné pour le Zero, la caméra et cette batterie.

## Ce qui reste à vérifier avant tout achat supplémentaire

- Le stock réel du Zero 2 W chez Kubii.
- Que la nappe « Pi Zero Edition » de Kubii convient à la HQ Camera : sa fiche cite les caméras v1, v2 et v3, pas la
  HQ. Le connecteur du Zero est le même que celui du Pi 5 ; la nappe Pi 5 (SC1128) devrait donc aller aussi, mais
  rien d'officiel ne le confirme.
- Le sens de l'adaptateur OTG vendu par Kubii (KUB2910) : sa description laisse penser qu'il est à l'envers de ce qu'il
  faut.
- La consommation du Zero 2 W : non publiée par Raspberry Pi.

## Sources (consultées le 25/11/2026)

- Fiche produit Zero 2 W (PDF) : <https://pip-assets.raspberrypi.com/categories/584-raspberry-pi-zero-2-w/documents/RP-008359-DS-1-raspberry-pi-zero-2-w-product-brief.pdf>
- Page produit Zero 2 W : <https://www.raspberrypi.com/products/raspberry-pi-zero-2-w/>
- Page produit HQ Camera (nappe « Zero » obligatoire, dimensions 38 × 38 × 18,4 mm) : <https://www.raspberrypi.com/products/raspberry-pi-high-quality-camera/>
- Documentation des caméras (connecteur mini 22 broches) : <https://www.raspberrypi.com/documentation/accessories/camera.html>
- Systèmes compatibles (Zero 2 W dans Raspberry Pi OS 64 bits) : <https://www.raspberrypi.com/software/operating-systems/>
- Batterie UGREEN (dimensions et poids) : <https://fr.ugreen.com/products/ugreen-batterie-externe-portable-usb-c-25000-mah-145w-3-sorties-cable>
