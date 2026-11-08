#!/usr/bin/env bash
# Installs syft (SBOM generator) into /usr/local/bin, built from source by the Go module proxy
# (proxy.golang.org, checksums verified against sum.golang.org), not downloaded from GitHub release assets:
# their download servers answered 504 for an hour during the 1.13.7 release, and the installer script
# piped into a root shell is gone too. Same pinned version as before.
set -euo pipefail
SYFT_VERSION=1.20.0
if command -v syft >/dev/null && syft version 2>/dev/null | grep -q "Version: *${SYFT_VERSION}$"; then
  exit 0
fi
command -v go >/dev/null || { echo "Go est nécessaire pour installer syft"; exit 1; }
bin=$(mktemp -d)
trap 'rm -rf "$bin"' EXIT
# A newer Go toolchain, if syft needs one, also comes from the module proxy (GOTOOLCHAIN=auto)
for i in 1 2 3; do
  GOBIN="$bin" GOTOOLCHAIN=auto go install -ldflags "-X main.version=${SYFT_VERSION}" \
    "github.com/anchore/syft/cmd/syft@v${SYFT_VERSION}" && break
  [[ $i -lt 3 ]] || exit 1
  echo "Compilation de syft échouée, nouvel essai dans 30 s"; sleep 30
done
sudo install -m 0755 "$bin/syft" /usr/local/bin/syft
syft version | grep -m1 Version
