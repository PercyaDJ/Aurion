# Protocole de test : Raspberry Pi 5 en expédition (V8)

But : vérifier sur le vrai matériel ce que les tests automatiques ne peuvent pas prouver. Le Pi 5 s'éteint, se
rallume seul à l'heure prévue, garde l'heure sans téléphone, et la batterie le laisse faire. À la fin, la fiche de
résultats (dernière section) permet de mettre à jour le README et le guide expédition avec des faits, pas des
suppositions.

Version : Aurion **1.13.9** ou plus récente. Aucun terminal, aucun réglage caché : tout se fait depuis le téléphone.

---

## 0. Tester sans rien risquer

### La règle d'or : deux cartes SD

Pensez à une roue de secours. La carte SD de votre prochaine expédition est la roue de secours : elle ne sert jamais
aux essais.

| Carte | Étiquette | Contenu | On y touche ? |
|---|---|---|---|
| Carte 1 | **EXPÉDITION** | la version qui a déjà marché | **jamais pendant les essais**, rangée dans une boîte |
| Carte 2 | **TEST** | Aurion 1.13.9, gravée pour ces essais | oui, autant qu'on veut |

Si un essai tourne mal, on remet la carte EXPÉDITION et la caméra redevient exactement celle d'avant. Une carte SD
de 32 Go coûte une dizaine d'euros : c'est l'assurance la moins chère du projet. Pour les photos, prenez aussi une
**clé USB de test** : celle de l'expédition garde ses réglages et ses photos.

### Ce qui peut réellement abîmer le matériel, et ce qui ne le peut pas

Un logiciel ne grille pas un Raspberry Pi. Au pire, une mauvaise version plante ou ne démarre pas : on change de
carte SD et tout repart. Les vrais risques sont physiques, et tous s'évitent :

| Risque réel | Comment l'éviter |
|---|---|
| Brancher ou débrancher la **nappe caméra** sous tension | toujours **débrancher l'alimentation** avant de toucher la nappe, la pile ou la carte SD |
| Mettre une **mauvaise pile** d'horloge | uniquement la pile officielle **ML-2020** avec sa prise (liste d'achat du README). Jamais une pile bouton CR2032 bricolée |
| **Électricité statique** | toucher un objet métallique (radiateur, robinet) avant de manipuler la carte, la tenir par les bords |
| Court-circuit sous le Pi | le poser sur une surface non métallique (bois, carton, plastique), jamais sur un plan de travail en inox ou un câble dénudé |
| **Condensation** en rentrant du froid | laisser la caméra 1 à 2 h dans un sac fermé avant de l'ouvrir au chaud |

Ce qui ne risque rien : laisser le Pi allumé des heures, le débrancher brutalement (Aurion est fait pour), lancer et
arrêter des nuits, changer les réglages, mettre à jour depuis le téléphone (retour arrière dans *Diagnostics*,
**Revenir à la version précédente**).

### Le calendrier avant l'expédition

- **Au plus tard un mois avant le départ** : finir ces essais. Les problèmes découverts tard sont ceux qu'on n'a pas
  le temps de corriger.
- **Deux semaines avant** : la version choisie est **gelée**. Plus aucune mise à jour sur la carte EXPÉDITION.
- **Une semaine avant** : une nuit complète de répétition générale (étape 5) avec la carte EXPÉDITION, dans le jardin
  ou sur le balcon.

---

## Matériel pour les essais

- Raspberry Pi 5, HQ Camera, nappe Pi 5, objectif (liste d'achat du README).
- **Pile d'horloge officielle ML-2020** branchée.
- Carte SD **TEST** gravée avec Aurion 1.13.9 et clé USB de test.
- Alimentation secteur officielle 27 W, et la batterie UGREEN chargée à 100 %.
- Le téléphone, une montre, cette fiche imprimée ou ouverte à côté.
- Fortement conseillé : un **testeur USB-C** (petit afficheur qui se branche entre la batterie et le câble, une
  quinzaine d'euros). C'est le seul moyen sûr de voir si le Pi est vraiment éteint (presque 0 W) ou planté
  (plusieurs watts). Il servira aussi à mesurer la consommation (test E1).

---

## Étape 1 : au bureau, sur secteur (15 minutes)

But : le Pi 5 démarre, voit la caméra, et sait qu'il peut se réveiller seul.

1. Pi **débranché**. Brancher la pile d'horloge sur son connecteur, la nappe, la carte SD TEST, la clé USB de test.
2. Brancher l'alimentation secteur 27 W. Attendre 3 à 5 minutes.
3. Téléphone sur le Wi-Fi **Aurion**, page d'accueil.
4. Vérifier dans **Prêt pour la nuit ?** : caméra OK, clé USB OK, heure OK.
5. Cocher **Plusieurs nuits (expédition)**. Le texte doit dire : « Le matin, il s'éteint et se rallume seul le soir
   suivant ». S'il dit « rebranchez la batterie le soir », Aurion ne voit pas l'alarme de réveil : **arrêter là** et
   me l'écrire.
6. *Diagnostics* : noter la version affichée (1.13.9 attendu) et l'heure système.

## Étape 2 : rallumage seul, sur secteur (environ 2 h 30)

But : le Pi s'éteint, puis se rallume seul 10 minutes avant le début de la nuit.

Aurion éteint un Pi 5 quand la nuit commence dans **plus de 2 heures**. On règle donc une nuit courte qui commence
dans 2 h 20.

1. Noter l'heure actuelle, par exemple 14:00.
2. Menu, **Réglages photo** : **Début** = maintenant + 2 h 20 (ici 16:20), **Fin** = début + 30 min (ici 16:50).
   Enregistrer, revenir à l'accueil. Choisir le mode *Toute la nuit*, la durée **De 16:20 à 16:50**, et laisser
   *Plusieurs nuits (expédition)* coché.
3. Appuyer sur **Lancer la nuit**. Le Wi-Fi se coupe au bout de 15 secondes, c'est normal.
4. Dans les 2 minutes : le Pi doit **s'éteindre** (testeur USB-C proche de 0 W, le Wi-Fi Aurion disparaît).
5. Heure de réveil attendue : début moins 10 minutes (ici **16:10**). Vers cette heure, le Wi-Fi **Aurion** doit
   réapparaître sur le téléphone (compter 1 à 3 minutes de démarrage).
6. Ne rien toucher : la nuit doit démarrer seule et photographier de 16:20 à 16:50.
7. Après 16:50 : le Pi range les photos, programme le réveil du lendemain et s'éteint seul (testeur proche de 0 W).
   Pi éteint, retirer la clé USB et l'ouvrir sur l'ordinateur : le dossier `sessions/<nuit>/JPG` ou `RAW` doit
   contenir des photos datées de 16:20 à 16:50 (l'heure est dans le nom de chaque fichier).

Noter dans la fiche : heure d'extinction, heure de réapparition du Wi-Fi, heures de la première et de la dernière
photo.

## Étape 3 : l'heure sans téléphone, avec la pile (20 minutes)

But : après une coupure totale de courant, le Pi connaît l'heure sans qu'un téléphone la lui donne.

1. Pi allumé, page d'accueil ouverte : noter l'heure affichée.
2. **Couper le téléphone du Wi-Fi Aurion** (mode avion, ou Wi-Fi coupé) et le garder coupé.
3. Débrancher l'alimentation. Attendre **10 minutes**.
4. Rebrancher. Attendre 5 minutes **sans connecter le téléphone**.
5. Connecter le téléphone, puis aller tout de suite dans *Diagnostics*. Les logs doivent contenir « Heure donnée par
   l'horloge matérielle (RTC) », et l'heure système doit être juste à la minute près.

À refaire une fois **sans la pile** (Pi débranché pour la retirer) : l'heure doit alors être fausse ou non confirmée
jusqu'à la connexion du téléphone. Cela prouve que c'est bien la pile qui fait le travail. Remettre la pile ensuite.

## Étape 4 : rallumage seul, sur la batterie (environ 2 h 30)

But : la batterie ne coupe pas sa sortie quand le Pi est éteint. Beaucoup de batteries s'arrêtent quand presque plus
rien ne consomme, et le Pi 5 ne pourrait alors jamais se rallumer.

Refaire exactement l'étape 2 en remplaçant l'alimentation secteur par la **batterie UGREEN** (chargée, câble court).
Pendant que le Pi est éteint, regarder l'écran de la batterie et le testeur :

- le Wi-Fi Aurion revient à l'heure prévue : la batterie convient ;
- il ne revient pas, et la batterie ou le testeur affiche 0 : la batterie a coupé sa sortie. Noter au bout de
  combien de temps, si possible. Ce n'est pas une panne d'Aurion : il faudra une autre batterie pour le Pi 5.

## Étape 5 : répétition générale (2 nuits, dehors ou sur un balcon)

But : les vraies conditions, avant d'emporter la caméra.

1. Batterie UGREEN chargée à 100 %, noter le pourcentage affiché.
2. *Réglages photo* : la plage habituelle (par exemple 21:00 à 06:00). Sur l'accueil : durée **De 21:00 à 06:00**,
   expédition cochée, **Lancer la nuit** l'après-midi.
3. Ne plus rien toucher pendant deux nuits.
4. Chaque matin, sans rien débrancher : noter le pourcentage de la batterie (son écran), puis vérifier les photos
   depuis le téléphone et appuyer sur **Éteindre Aurion** pour qu'il se rallume seul le soir.

Si la batterie ne tient pas deux nuits, la recharger le matin : c'est déjà une information utile (nombre de
batteries par nuit).

---

## Fiche de résultats (à me renvoyer)

Recopiez ce tableau dans un message, même incomplet. Une case « non fait » est aussi une information.

| Étape | Ce qu'on regarde | Attendu | Résultat |
|---|---|---|---|
| 1 | version dans *Diagnostics* | 1.13.9 | |
| 1 | texte du mode expédition | « se rallume seul le soir suivant » | |
| 2 | heure de **Lancer la nuit** / heure d'extinction | moins de 2 min d'écart | |
| 2 | heure de réapparition du Wi-Fi | début moins 10 min (+ 1 à 3 min) | |
| 2 | première et dernière photo | début et fin de la plage | |
| 2 | consommation éteint (si testeur) | proche de 0 W | |
| 3 | heure juste sans téléphone, avec pile | oui, message « horloge matérielle (RTC) » | |
| 3 | sans pile | heure fausse ou à confirmer | |
| 4 | la batterie reste allumée, Pi éteint | Wi-Fi revenu à l'heure | |
| 5 | batterie au départ, après nuit 1, après nuit 2 | à mesurer | |
| 5 | la nuit 2 a démarré seule | oui | |
| tout | quelque chose d'inattendu | | |
