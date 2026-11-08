#!/usr/bin/env bash
# Installs the scanners into /usr/local/bin, built from source by the Go module proxy
# (proxy.golang.org, checksums verified against sum.golang.org), not downloaded from GitHub release assets:
# their download servers answered 504 for hours during the 1.13.7 release, and the installer scripts piped
# into a root shell are gone too. Pinned versions, unchanged.
#   scripts/install-tools.sh syft [trivy]
set -euo pipefail
SYFT_VERSION=1.20.0
TRIVY_VERSION=0.70.0

install_one() { # <name> <version> <module path of the command> <ldflags> [env...]
  local name=$1 version=$2 pkg=$3 ldflags=$4 bin i
  shift 4
  if command -v "$name" >/dev/null && "$name" version 2>/dev/null | grep -qE "Version: *${version}$"; then
    return 0
  fi
  command -v go >/dev/null || { echo "Go est nécessaire pour installer $name"; exit 1; }
  bin=$(mktemp -d)
  # A newer Go toolchain, if the tool needs one, also comes from the module proxy (GOTOOLCHAIN=auto)
  for i in 1 2 3; do
    env "$@" GOBIN="$bin" GOTOOLCHAIN=auto go install -ldflags "$ldflags" "${pkg}@v${version}" && break
    [[ $i -lt 3 ]] || { rm -rf "$bin"; exit 1; }
    echo "Compilation de $name échouée, nouvel essai dans 30 s"; sleep 30
  done
  sudo install -m 0755 "$bin/$name" "/usr/local/bin/$name"
  rm -rf "$bin"
  "$name" version | grep -m1 Version
}

for tool in "$@"; do
  case "$tool" in
    syft) install_one syft "$SYFT_VERSION" github.com/anchore/syft/cmd/syft "-X main.version=${SYFT_VERSION}" ;;
    # trivy 0.70 builds with Go's experimental encoding/json/v2
    trivy) install_one trivy "$TRIVY_VERSION" github.com/aquasecurity/trivy/cmd/trivy \
             "-X github.com/aquasecurity/trivy/pkg/version/app.ver=${TRIVY_VERSION}" GOEXPERIMENT=jsonv2 ;;
    *) echo "Outil inconnu : $tool"; exit 1 ;;
  esac
done
