# Protocole de test : Raspberry Pi 5 en expédition (V8)

But : vérifier sur le vrai matériel ce que les tests automatiques ne peuvent pas prouver. Le Pi 5 s'éteint, se
rallume seul à l'heure prévue, garde l'heure sans téléphone, et la batterie le laisse faire. Aurion fait lui-même
les vérifications (carte *Vérification d'Aurion*, test de rallumage en 3 minutes) : il reste environ 35 minutes de
manipulations, plus deux nuits de répétition. À la fin, le rapport téléchargé depuis *Diagnostics* permet de mettre à
jour le README et le guide expédition avec des faits, pas des suppositions.

Version : Aurion **1.14.0** ou plus récente. Aucun terminal, aucun réglage caché : tout se fait depuis le téléphone.

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
- Le téléphone et une montre.
- Fortement conseillé : un **testeur USB-C** (petit afficheur qui se branche entre la batterie et le câble, une
  quinzaine d'euros). C'est le seul moyen sûr de voir si le Pi est vraiment éteint (presque 0 W) ou planté
  (plusieurs watts). Il servira aussi à mesurer la consommation (test E1).

---

## Étape 1 : brancher, ouvrir la page, c'est vérifié (5 minutes)

But : tout le matériel fonctionne, sans rien tester à la main. Aurion se vérifie seul.

1. Pi **débranché**. Brancher la pile d'horloge, la nappe, la carte SD TEST, la clé USB de test.
2. Brancher l'alimentation secteur 27 W. Attendre 3 à 5 minutes.
3. Téléphone sur le Wi-Fi **Aurion** : la page s'ouvre.
4. La carte **Vérification d'Aurion** se lance seule (environ une minute) : une vraie photo avec son RAW,
   enregistrée sur la clé et relue, la mémoire, l'heure, l'alimentation. Attendu : **Prêt** ou **Prêt, avec
   conseils**. Si elle affiche **À corriger**, chaque ligne rouge dit quoi faire.

## Étape 2 : rallumage seul, sur secteur (5 minutes)

But : le Pi s'éteint et se rallume seul par son horloge.

1. Menu, **Diagnostics**, **Tester le rallumage (3 minutes)**, confirmer.
2. Aurion s'éteint (le Wi-Fi Aurion disparaît, testeur proche de 0 W) et doit se rallumer seul 3 minutes plus tard.
3. Environ 5 minutes après, se reconnecter au Wi-Fi Aurion et rouvrir **Diagnostics** : le résultat s'affiche
   (« réussi » ou « échec » avec la raison). Si le Wi-Fi ne revient pas au bout de 10 minutes, débrancher et
   rebrancher : la page indiquera l'échec.

## Étape 3 : l'heure sans téléphone, avec la pile (20 minutes)

But : après une coupure totale de courant, le Pi connaît l'heure sans qu'un téléphone la lui donne.

1. **Couper le téléphone du Wi-Fi Aurion** (mode avion) et le garder coupé.
2. Débrancher l'alimentation. Attendre **10 minutes**. Rebrancher. Attendre 5 minutes **sans connecter le téléphone**.
3. Connecter le téléphone, menu **Diagnostics**, **Vérifier Aurion** : la ligne de l'heure doit dire
   « Horloge matérielle : juste même sans téléphone ».

## Étape 4 : rallumage seul, sur la batterie (5 minutes)

But : la batterie ne coupe pas sa sortie quand le Pi est éteint. Beaucoup de batteries s'arrêtent quand presque plus
rien ne consomme, et le Pi 5 ne pourrait alors jamais se rallumer.

Refaire l'étape 2 en remplaçant l'alimentation secteur par la **batterie UGREEN** (chargée, câble court).
- « Réussi » : la batterie convient (l'étape 5 le confirme sur une journée entière).
- « Échec » : la batterie a coupé sa sortie. Ce n'est pas une panne d'Aurion : il faudra une autre batterie pour le
  Pi 5.

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

## Le retour : un seul fichier

Après les étapes, menu **Diagnostics**, **Télécharger le rapport**, et envoyez-moi ce fichier. Il contient les
versions, la dernière vérification, le résultat du test de rallumage et le journal : pas de fiche à remplir. Ajoutez
seulement ce que la caméra ne peut pas mesurer elle-même : le pourcentage de la batterie chaque matin de l'étape 5.
