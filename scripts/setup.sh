#!/usr/bin/env bash
set -euo pipefail

AURION_DIR="$(cd "$(dirname "$0")/.." && pwd)"

echo "==================================================="
echo "  🌌 Aurion - Installation Tout-en-un (Safe Mode)  "
echo "==================================================="
echo ""
echo "Ce script va configurer le système (bootstrap),"
echo "installer les dépendances, compiler le projet et redémarrer."
echo "Temps estimé : 15 à 30 minutes sur Raspberry Pi 4."
echo ""
echo "ATTENTION : Assurez-vous que votre clé USB est branchée !"
echo "La compilation est effectuée localement sur ce Raspberry Pi."
echo ""
echo "Une demande de mot de passe sudo va apparaître pour débuter."
echo "==================================================="
echo ""

cd "$AURION_DIR"

# Demande des droits sudo dès le début
sudo -v

# Maintient le ticket sudo actif en arrière-plan (évite le timeout pendant la compilation de 30 min)
while true; do sudo -n true; sleep 60; kill -0 "$$" || exit; done 2>/dev/null &

# 1. Bootstrap (nécessite sudo)
echo ""
echo "[1/3] Hardening du système (Bootstrap)..."
sudo bash scripts/bootstrap.sh --yes

# 2. Installation (exécuté en tant qu'utilisateur normal)
echo ""
echo "[2/3] Installation et compilation d'Aurion..."
bash scripts/install.sh

# 3. Reboot
echo ""
echo "==================================================="
echo "  ✅ Installation terminée avec succès !"
echo "  Le système va redémarrer dans 10 secondes."
echo "  Au redémarrage, Aurion se lancera automatiquement."
echo "==================================================="
sleep 10
sudo reboot
