# Guide de démarrage (aucune connaissance technique nécessaire)

## Ce qu'il faut

| Matériel | Détail |
|---|---|
| Raspberry Pi 4 (2 Go ou plus) | le Pi 5 fonctionne aussi, avec la nappe caméra adaptée au Pi 5 |
| Caméra Raspberry Pi HQ (IMX477) + objectif | un objectif grand angle lumineux pour les aurores |
| Carte micro-SD de 16 Go ou plus | pour le système |
| Clé USB de 128 Go ou plus, formatée en **exFAT** | pour les photos (sur un PC : clic droit, *Formater*, exFAT) |
| Batterie USB-C 5 V / 3 A (ou alimentation officielle) | avec un câble court et épais |
| Un téléphone | pour tout piloter |
| Un ordinateur (une seule fois) | pour préparer la carte SD |

## 1. Préparer la carte SD (10 minutes, une seule fois)

1. Sur l'ordinateur, installez **Raspberry Pi Imager** (gratuit, raspberrypi.com/software).
2. Téléchargez **[aurion-raspios-arm64.img.xz](https://github.com/PercyaDJ/Aurion/releases/latest/download/aurion-raspios-arm64.img.xz)** (environ 600 Mo). Ce fichier contient tout :
   le système du Raspberry Pi et l'application Aurion. Le lien donne toujours la dernière version.
3. Dans Raspberry Pi Imager :
   - *Appareil* : votre modèle de Raspberry Pi ;
   - *Système d'exploitation* : tout en bas, **Utiliser une image personnalisée**, puis le fichier téléchargé ;
   - *Stockage* : la carte SD ;
   - à la question des réglages personnalisés, répondez **Non**.
4. Attendez la fin de l'écriture, retirez la carte.

## 2. Brancher le matériel (Raspberry Pi éteint et débranché)

1. **Caméra** : soulevez doucement le loquet du connecteur *CAMERA* du Pi, insérez la nappe à fond, contacts
   métalliques tournés vers les prises HDMI (Pi 4), rabaissez le loquet. Même chose côté caméra.
2. **Carte SD** : dans le lecteur sous le Pi.
3. **Clé USB** : dans un port USB (bleu de préférence).
4. **Alimentation** : en dernier, sur la prise USB-C.

Les voyants du Pi restent **éteints** : c'est volontaire, pour ne pas éclairer la photo et économiser la batterie.

## 3. Premier allumage (3 à 5 minutes)

Le Pi s'installe tout seul au premier démarrage, il peut redémarrer une fois. Ensuite, sur le téléphone :

1. Réglages Wi-Fi : rejoignez **Aurion**, mot de passe **aurora2024**.
2. La page Aurion s'ouvre toute seule. Sinon, ouvrez le navigateur à l'adresse **http://192.168.4.1:8080**.
3. Un encadré **Protégez votre caméra** propose de choisir votre mot de passe Wi-Fi (10 caractères minimum).
   Le Wi-Fi Aurion redémarre aussitôt : reconnectez-vous avec le nouveau mot de passe.

L'heure du Pi se règle automatiquement sur celle du téléphone à chaque connexion.

## 4. Une nuit d'aurores

1. Installez la caméra face au nord, stable, objectif propre, mise au point sur l'infini.
2. Accueil, **Cadrer (aperçu)** : vérifiez le cadrage.
3. Revenez à l'accueil. La liste **Prêt pour la nuit ?** vérifie la caméra, la clé USB (et le nombre d'heures
   de photos qu'elle peut contenir), l'heure et l'alimentation. Un point rouge empêche de lancer la nuit
   et dit quoi faire ; un point orange est un conseil.
4. Choisissez :
   - **Toute la nuit** (conseillé, idéal pour un timelapse) : photos à la suite, chaque pose durant le temps choisi
     par l'exposition automatique ; les photos avec aurore sont marquées ; ou **Aurores seulement** : surveille le
     ciel et ne commence à enregistrer qu'à la première aurore confirmée ;
   - la **durée** : la plage horaire habituelle (21:00 à 06:00) ou un nombre d'heures à partir de maintenant ;
   - les **photos** : RAW (conseillé), RAW + JPG ou JPG seul.
5. Appuyez sur **Lancer la nuit**. Le Wi-Fi se coupe au bout de 15 secondes, c'est normal : vous pouvez partir.
6. La caméra photographie seule et s'éteint seule à la fin.

**Coupure de courant pendant la nuit** (batterie vide ou changée, câble débranché) : rebranchez simplement.
Aurion rallume son Wi-Fi 5 minutes (au cas où vous voudriez annuler depuis le téléphone), puis reprend seul
la nuit jusqu'à l'heure de fin prévue.

## 5. Récupérer les photos

- **Sur le téléphone** : rallumez, reconnectez-vous au Wi-Fi Aurion. L'accueil affiche la **Dernière nuit**
  (nombre de photos, d'aurores, de RAW) avec trois boutons : **Voir les plus belles**, **Télécharger les RAW**,
  **Tout télécharger**. Dans *Photos*, onglet *Par session*, chaque nuit a ses boutons RAW, JPG et Tout.
- **Sur l'ordinateur** : accueil, **Éteindre Aurion**, attendez 20 secondes, puis branchez la clé USB sur
  l'ordinateur : chaque nuit a son dossier dans `sessions/` (sous-dossiers `RAW` et `JPG`, et `aurores.csv`
  qui liste les images avec aurore, de la plus forte à la plus faible).

## Mettre à jour

Aurion est en deux parties, comme un appareil photo avec son firmware et ses réglages :

| Partie | Où | Mise à jour |
|---|---|---|
| **Système + application** (l'image) | carte SD | regraver la dernière image avec Raspberry Pi Imager : quand une nouvelle version le demande (*Diagnostics* l'indique) ou pour repartir à neuf |
| **Application seule** | carte SD | depuis le téléphone : *Diagnostics*, **Mettre à jour depuis GitHub** (le système n'est pas touché) ; retour arrière en un geste |
| **Photos et copie des réglages** | clé USB | jamais effacées par une mise à jour |

Regraver la carte SD ne fait rien perdre : au premier démarrage, Aurion reprend ses réglages (mot de passe Wi-Fi,
réglages photo, mode expédition) dans le fichier `aurion-reglages.json` de la clé USB. La clé est ainsi la mémoire
de la caméra, la carte SD n'est que le programme.

## En combien de temps ?

**Une seule fois** (préparation, au chaud) :

| Étape | Durée |
|---|---|
| Télécharger l'image (environ 600 Mo) | selon la connexion |
| Graver la carte SD avec Raspberry Pi Imager | quelques minutes selon la carte |
| Premier démarrage (installation automatique, sans internet) | 3 à 5 minutes |
| Mot de passe, réglages | 2 à 3 minutes |

**Chaque soir** (dehors, dans le froid) :

1. Brancher la batterie.
2. Le téléphone rejoint tout seul le Wi-Fi Aurion (il le connaît déjà) : la page s'ouvre.
3. Vérifier la liste *Prêt pour la nuit ?* ; les réglages de la veille sont déjà remplis.
4. **Lancer la nuit**.

Le Wi-Fi Aurion démarre en premier, avant tout le reste. Le temps réel entre l'allumage et le Wi-Fi prêt est affiché
dans *Diagnostics* (« Wi-Fi prêt après l'allumage ») : c'est la mesure à vérifier sur votre Raspberry Pi. En mode
expédition, il n'y a même rien à faire : la nuit démarre seule.

## Plusieurs nuits d'affilée

Cochez **Plusieurs nuits (expédition)** sur l'accueil : chaque soir la nuit démarre seule. Tout est expliqué dans
[GUIDE_EXPEDITION.md](GUIDE_EXPEDITION.md).

## Menu simple et mode expert

Le menu ne montre que l'essentiel : Accueil, Photos, Cadrage, Réglages photo, Diagnostics.
Cochez **Mode expert** en bas du menu pour afficher les réglages fins, les presets et le stockage
(le choix est mémorisé sur le téléphone).

## En cas de problème

| Symptôme | Solution |
|---|---|
| Le Wi-Fi « Aurion » n'apparaît pas | attendre 5 minutes au premier démarrage ; débrancher et rebrancher l'alimentation |
| La page ne s'ouvre pas | taper http://192.168.4.1:8080 dans le navigateur |
| Point rouge « Clé USB absente » | rebrancher la clé ; en essayer une autre |
| Point rouge « Clé USB à préparer » | la clé n'est pas lisible (neuve non formatée, format d'ordinateur Linux…) : bouton **Préparer la clé** sous le point rouge. **Tout son contenu est effacé** |
| Point rouge « Caméra non détectée » | Pi débranché, vérifier la nappe (sens et loquet) des deux côtés |
| Point rouge « Alimentation trop faible » ou le Pi s'éteint seul | batterie 5 V / 3 A, câble court et épais |
| Mot de passe Wi-Fi oublié | sur un ordinateur, créez un fichier vide nommé `aurion-reset-wifi.txt` à la racine de la clé USB, rebranchez la clé sur le Pi et rallumez : le mot de passe redevient **aurora2024** (le fichier est renommé `.done`, vos photos ne sont pas touchées) |
| « Nuit interrompue » à l'allumage alors que vous voulez récupérer vos photos | appuyez sur **Annuler la reprise** |

Pour les réglages photo (RAW, timelapse, darks) : [GUIDE_PHOTO.md](GUIDE_PHOTO.md).
