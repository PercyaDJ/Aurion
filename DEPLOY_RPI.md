# Déployer Aurion sur un Raspberry Pi

## 1. Préparer la carte SD

Avec **Raspberry Pi Imager** :
- Système : *Raspberry Pi OS Lite (64-bit)* (Bookworm ou Trixie).
- Réglages avancés (roue dentée) : nom d'hôte `aurion`, utilisateur et mot de passe, **SSH activé**,
  Wi-Fi de la maison (pour l'installation), fuseau horaire.

Brancher la caméra (nappe CSI), la clé USB (exFAT ou FAT32), démarrer.
Le Pi est joignable en `ssh utilisateur@aurion.local` au bout d'une minute environ.

## 2. Installer

Trois possibilités, au choix (détails dans le [README](README.md)) :

| Depuis | Commande |
|---|---|
| le Pi, avec internet | `curl -fsSL https://raw.githubusercontent.com/PercyaDJ/Aurion/main/scripts/get.sh \| sudo bash` |
| un PC Windows | `.\scripts\deploy.ps1 utilisateur@aurion.local .\aurion-1.5.0-rpi-arm64.tar.gz` |
| un PC Linux / macOS | `scripts/deploy.sh utilisateur@aurion.local aurion-1.5.0-rpi-arm64.tar.gz` |

L'archive `aurion-<version>-rpi-arm64.tar.gz` est construite par GitHub Actions (onglet *Releases*,
ou onglet *Actions*, artefact `aurion-rpi-arm64`).

### Ce que fait `install.sh`

1. Installe les paquets nécessaires : `rpicam-apps`, `iw`, `nftables` (+ `hostapd`/`dnsmasq` si NetworkManager est absent).
2. Copie le binaire dans `/opt/aurion` (l'interface web est incluse dedans).
3. Crée `/opt/aurion/config/aurion.json` avec **un mot de passe Wi-Fi unique** (ou reprend la config existante,
   y compris celle d'une ancienne installation `~/Aurion`).
4. Installe `/usr/local/sbin/aurion-helper`, **seul** programme que le service peut lancer en root.
5. Configure le montage automatique de **n'importe quelle** clé USB sur `/mnt/capture`.
6. Prépare le hotspot et le portail captif.
7. Optimise le système pour le terrain (désactivable avec `--no-hardening`) : journaux en RAM,
   `noatime`, pas de mises à jour automatiques, watchdog matériel, arrêt propre en cas de sous-tension persistante.
8. Active et démarre `aurion.service`, puis affiche le Wi-Fi et son mot de passe.

> Quand le service démarre, le hotspot prend le Wi-Fi du Pi : une session SSH ouverte par le Wi-Fi
> de la maison se coupe. C'est normal. Le résumé (avec le mot de passe) est affiché avant.
> Pour revenir en SSH : se connecter au Wi-Fi Aurion (`ssh utilisateur@192.168.4.1`), utiliser un câble
> réseau, ou *Diagnostics, Connexion Wi-Fi (maintenance)* dans l'interface.

## 3. Mettre à jour

- Depuis le téléphone : *Diagnostics, Mise à jour du logiciel*, fichier `aurion` de la nouvelle archive.
- Ou relancer l'installation avec la nouvelle archive (la configuration est conservée).
- Retour arrière : `sudo cp /opt/aurion/aurion.prev /opt/aurion/aurion && sudo systemctl restart aurion`.

## 4. Commandes utiles

```bash
sudo systemctl status aurion          # état
sudo journalctl -u aurion -f          # journal en direct
sudo systemctl restart aurion         # redémarrer
/opt/aurion/aurion --config-dir /opt/aurion/config check-config   # vérifier la config
sudo ./install.sh --uninstall            # depuis le dossier de l'archive : désinstaller (photos conservées)
```

## 5. Dépannage

| Problème | Piste |
|---|---|
| Pas de Wi-Fi « Aurion » | `sudo journalctl -u aurion -n 50` ; vérifier le pays Wi-Fi (`sudo raspi-config`, *Localisation*) |
| La page ne s'ouvre pas seule | ouvrir `http://192.168.4.1:8080` |
| « Clé USB non détectée » | clé en exFAT/FAT32 ? `lsblk -f` ; rebrancher la clé (montage automatique) |
| Caméra indisponible | `rpicam-hello --list-cameras` ; nappe CSI dans le bon sens |
| Heure fausse | ouvrir l'interface depuis le téléphone : l'heure est synchronisée à l'ouverture |
| Le Pi s'éteint seul | sous-tension : alimentation 5 V / 3 A officielle ou batterie de qualité |

## Compiler sur le Pi (secours)

Sans archive : `git clone` du dépôt puis `bash scripts/setup.sh` (installe Rust, compile 15 à 30 min, puis installe).
