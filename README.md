# AuroraCam

**Caméra autonome de capture d'aurores** pour Raspberry Pi 4 (HQ Camera IMX477).

## Architecture

Ports / Adapters (hexagonal) :
- **Core** : machine à états, détection, exposition, config — logique pure, aucune dépendance hardware
- **Ports** : traits `CameraPort`, `StoragePort`, `ClockPort`, `NetworkApPort`, `SystemPort`
- **Adapters PC** : mocks pour développement et tests sur PC
- **Adapters RPi** : implémentations réelles (libcamera, hostapd, etc.)

## Développement PC

```bash
# Compiler
cargo build

# Tests
cargo test

# Serveur web (mode ARM pour tester l'UI)
cargo run -- serve

# Simulation d'une nuit complète
cargo run -- simulate
```

## Cycle de vie

`Boot → Arm → Disconnect → Calibration → Watch → Run → Shutdown`

- **Boot** : montage USB, config, hotspot Wi-Fi, serveur web
- **Arm** : interface web pour configuration
- **Disconnect** : coupure Wi-Fi, verrouillage config
- **Calibration** : ajustement auto ISO/shutter
- **Watch** : capture périodique + détection d'aurore
- **Run** : capture continue (2 détections positives consécutives)
- **Shutdown** : sync, démontage, arrêt
- **SafeMode** : protection données si stockage critique

## Plateforme cible

- Raspberry Pi 4 Model B (4 Go RAM)
- Caméra RPi HQ (IMX477)
- Objectif M12 2.7mm 12MP
- Stockage USB 128 Go

## Licence

Propriétaire — tous droits réservés.
