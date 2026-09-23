# Consommation sur batterie et protection contre les coupures

## 1. Économies d'énergie

| Mesure | Où | Effet |
|---|---|---|
| Analyse sur la miniature EXIF 320×240 au lieu de l'image 12 Mpx | `core/jpeg.rs`, `adapters/rpi/camera_rpi.rs` | environ 230 fois moins de calcul par image (§ bench) ; CPU presque au repos entre deux poses, **capteur moins chaud donc moins de bruit thermique dans les RAW** |
| Aucun décodage ni ré-encodage des photos par défaut | `DenoiseConfig` (tout désactivé) | le JPEG et le DNG de `rpicam-still` sont écrits tels quels |
| Miniature de galerie = miniature EXIF | `orchestrator::save_thumbnail` | plus d'encodage par image |
| Wi-Fi coupé pendant la nuit (radio éteinte) | `aurion-helper ap-stop` | le poste le plus gourmand après le CPU |
| Bluetooth, audio désactivés ; LED éteintes | `install.sh` (bloc `config.txt`) | quelques dizaines de mA ; **aucune lumière parasite près de l'objectif** |
| Services inutiles arrêtés (bluetooth, ModemManager, triggerhappy) | `install.sh` | moins de réveils |
| Runtime limité à 2 threads | `main.rs` | moins de réveils CPU |
| Journaux en RAM | `install.sh` | pas d'écriture sur la carte SD |
| Surveillance (*Aurores seulement*) : petite photo JPEG par minute, aucun RAW tant que l'aurore n'est pas confirmée ; processeur au minimum | `orchestrator`, `aurion-helper power-profile watch` | moins de calcul et d'écriture par photo de surveillance |
| Port Ethernet coupé la nuit sans câble | `aurion-helper power-profile` | un circuit de moins alimenté |
| Rien le jour : extinction à la fin de chaque nuit ; Pi 5 éteint jusqu'au soir (réveil par son horloge) au lieu d'attendre allumé | `orchestrator`, `aurion-helper rtc-wake` | toute la batterie sert aux nuits (détail : GUIDE_EXPEDITION.md) |
| Interface : rafraîchissement suspendu quand l'écran du téléphone est éteint ou l'onglet caché | `js/common.js` (`aurionPoll`) | moins de requêtes, moins de réveils du Pi pendant la préparation |

Pour aller plus loin (non appliqué automatiquement, faute de mesure sur le matériel) : débrancher l'écran HDMI, limiter la fréquence CPU
(`arm_freq`), utiliser un Pi 4 plutôt qu'un Pi 5 (consommation au repos plus faible). Ordres de grandeur et mesure :
voir PLAN_ACTION.md (tâche E1, mesure réelle avec un testeur USB).

## 2. Coupures de courant : ce qui est garanti

| Donnée | Protection | Après une coupure |
|---|---|---|
| Photos (JPEG, DNG, miniatures) | écriture sous un nom temporaire, `fsync`, renommage, `fsync` du dossier | chaque photo est complète ou absente, jamais tronquée |
| Journal de session (`event.jsonl`, `session.log`) | `fsync` toutes les 6 images (environ 1 min) et en fin de nuit | au plus la dernière minute de journal perdue |
| Système de fichiers de la clé (FAT32/exFAT) | vérification et réparation automatiques à chaque montage (`systemd-mount --fsck=yes`) ; option `flush` en FAT32 | une clé « sale » est réparée au démarrage suivant |
| Configuration | écriture atomique (fsync + renommage), droits 0600 | ancienne ou nouvelle version, jamais un fichier vide |
| Binaire (mise à jour) | copie `.new` vérifiée puis renommage atomique ; `.prev` conservé | l'ancienne version reste utilisable |
| Carte SD | journaux en RAM, `noatime`, pas de mises à jour automatiques | très peu d'écritures, donc très peu de risque |
| Batterie qui s'épuise | arrêt propre sur sous-tension persistante (3 contrôles de suite) | `sync` puis extinction |
| Blocage du système | watchdog matériel (redémarrage) et watchdog systemd (relance du service) | reprise automatique |
| Nuit interrompue (batterie vide, changée, câble arraché) | `night.json` écrit au lancement, supprimé en fin normale | au redémarrage : Wi-Fi 5 min (annulable depuis le téléphone), puis la nuit reprend seule jusqu'à la fin prévue, 3 fois au plus, **dans le même dossier avec la numérotation qui continue** |
| Fichiers incomplets d'une coupure (`.nom.part`) | jamais listés, supprimés au démarrage de la nuit suivante | la clé ne se remplit pas de fichiers inutilisables |
| Plusieurs semaines de photos | un dossier par nuit (création de fichiers rapide même avec des dizaines de milliers d'images), galerie limitée aux images récentes par requête | pas de ralentissement ni de page figée au fil de l'expédition |

## 3. Recommandations terrain

- Clé USB de bonne qualité en **exFAT**, 128 Go ou plus en RAW. Un DNG non compressé de 12,3 Mpx sur 16 bits pèse environ 24 Mo (taille réelle à vérifier au premier essai) : au plus environ 78 Go pour une nuit de 9 h à une image toutes les 10 s (moins en pratique, le temps de pose s'ajoute à l'intervalle).
- Batterie capable de fournir 3 A en continu ; câble court et épais (les chutes de tension viennent souvent du câble).
- Froid : les batteries perdent 30 à 50 % de capacité vers -10 °C ; garder la batterie au chaud (poche, boîtier isolé).
