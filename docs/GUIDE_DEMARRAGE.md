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
   - **Toute la nuit** (conseillé, idéal pour un timelapse) : une photo toutes les 10 secondes, les photos avec
     aurore sont marquées ; ou **Aurores seulement** : n'enregistre que pendant les aurores ;
   - la **durée** : la plage horaire habituelle (21:00 à 06:00) ou un nombre d'heures à partir de maintenant ;
   - les **photos** : RAW + JPG (conseillé), RAW seul ou JPG seul.
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
  l'ordinateur : les photos sont à la racine, les journaux dans `sessions/`.

## Menu simple et mode expert

Le menu ne montre que l'essentiel : Accueil, Photos, Cadrage, Réglages photo, Diagnostics.
Cochez **Mode expert** en bas du menu pour afficher les réglages fins, les presets et le stockage
(le choix est mémorisé sur le téléphone).

## En cas de problème

| Symptôme | Solution |
|---|---|
| Le Wi-Fi « Aurion » n'apparaît pas | attendre 5 minutes au premier démarrage ; débrancher et rebrancher l'alimentation |
| La page ne s'ouvre pas | taper http://192.168.4.1:8080 dans le navigateur |
| Point rouge « Clé USB absente » | clé en exFAT ou FAT32, la rebrancher ; en essayer une autre |
| Point rouge « Caméra non détectée » | Pi débranché, vérifier la nappe (sens et loquet) des deux côtés |
| Point rouge « Alimentation trop faible » ou le Pi s'éteint seul | batterie 5 V / 3 A, câble court et épais |
| Mot de passe Wi-Fi oublié | sur un ordinateur, créez un fichier vide nommé `aurion-reset-wifi.txt` à la racine de la clé USB, rebranchez la clé sur le Pi et rallumez : le mot de passe redevient **aurora2024** (le fichier est renommé `.done`, vos photos ne sont pas touchées) |
| « Nuit interrompue » à l'allumage alors que vous voulez récupérer vos photos | appuyez sur **Annuler la reprise** |

Pour les réglages photo (RAW, timelapse, darks) : [GUIDE_PHOTO.md](GUIDE_PHOTO.md).
