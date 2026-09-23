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
2. Téléchargez le fichier **aurion-…-raspios-arm64.img.xz** depuis la page *Releases* du projet sur GitHub.
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
3. Allez dans *Réglages avancés* et **changez le mot de passe Wi-Fi** (10 caractères minimum). Il sera actif au prochain démarrage.

L'heure du Pi se règle automatiquement sur celle du téléphone à chaque connexion.

## 4. Une nuit d'aurores

1. Installez la caméra face au nord, stable, objectif propre, mise au point sur l'infini.
2. Sur le téléphone : *Preview* pour vérifier le cadrage.
3. Option conseillée pour un timelapse propre : *Réglages avancés*, cocher **Verrouiller l'exposition**.
4. Tableau de bord : **Déconnexion, lancer la capture**. Le Wi-Fi se coupe au bout de 15 secondes, c'est normal.
5. La caméra photographie toute la nuit et s'éteint seule à la fin de la plage horaire.

## 5. Récupérer les photos

- **Sur le téléphone** : rallumez, reconnectez-vous au Wi-Fi Aurion, *Galerie*. L'onglet **Meilleures aurores**
  propose les images les plus fortes et leurs RAW en un seul téléchargement.
- **Sur l'ordinateur** : éteignez (Diagnostics, *Éteindre*), puis branchez la clé USB sur l'ordinateur :
  les photos sont à la racine, les journaux dans `sessions/`.

## En cas de problème

| Symptôme | Solution |
|---|---|
| Le Wi-Fi « Aurion » n'apparaît pas | attendre 5 minutes au premier démarrage ; débrancher et rebrancher l'alimentation |
| La page ne s'ouvre pas | taper http://192.168.4.1:8080 dans le navigateur |
| « Clé USB non détectée » | clé en exFAT ou FAT32, la rebrancher ; en essayer une autre |
| « Caméra indisponible » | Pi débranché, vérifier la nappe (sens et loquet) des deux côtés |
| Le Pi s'éteint tout seul | batterie trop faible ou câble trop fin : alimentation 5 V / 3 A |
| Mot de passe Wi-Fi oublié | graver à nouveau la carte SD (les photos de la clé USB ne sont pas touchées) |

Pour les réglages photo (RAW, timelapse, darks) : [GUIDE_PHOTO.md](GUIDE_PHOTO.md).
