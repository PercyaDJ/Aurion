# ─────────────────────────────────────────────────────────────
# Aurion — envoi et installation sur le Raspberry Pi depuis Windows
# (ssh/scp sont intégrés à Windows 10 et 11).
#
#   .\scripts\deploy.ps1 pi@aurion.local .\aurion-1.7.0-rpi-arm64.tar.gz
#
# L'archive se télécharge dans l'onglet "Releases" du dépôt GitHub
# (elle est construite automatiquement par GitHub Actions).
# ─────────────────────────────────────────────────────────────
param(
    [Parameter(Mandatory = $true)][string]$HostName,
    [Parameter(Mandatory = $true)][string]$Archive
)
$ErrorActionPreference = "Stop"

if (-not (Test-Path $Archive)) { throw "Archive introuvable : $Archive" }
$name = [System.IO.Path]::GetFileName($Archive) -replace '\.tar\.gz$', ''

Write-Host "Envoi de $name vers $HostName"
scp $Archive "${HostName}:/tmp/$name.tar.gz"
if ($LASTEXITCODE -ne 0) { throw "Échec de l'envoi (scp)" }

Write-Host "Installation (le mot de passe sudo du Pi peut être demandé)"
ssh -t $HostName "cd /tmp && tar xzf $name.tar.gz && sudo ./$name/install.sh && rm -rf /tmp/$name /tmp/$name.tar.gz"
if ($LASTEXITCODE -ne 0) { throw "Échec de l'installation" }
