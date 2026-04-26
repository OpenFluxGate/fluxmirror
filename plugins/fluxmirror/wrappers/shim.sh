#!/bin/bash
# FluxMirror cross-shell hook entry point (bash variant).
#
# Usage:
#   shim.sh <kind>                  # kind = claude | gemini
#   FLUXMIRROR_KIND=<kind> shim.sh  # env override (lower priority than $1)
#
# Auto-downloads the per-arch fluxmirror binary from the latest GitHub
# release on first invocation, caches it under FLUXMIRROR_CACHE
# (default ~/.fluxmirror/cache), then execs `fluxmirror hook --kind <kind>`
# with stdin forwarded unchanged.
#
# Required: bash + curl (both universal on macOS / Linux / WSL).
#
# IMPORTANT: any failure in detection, download, or exec must NOT propagate
# to the calling agent. We always exit 0.

KIND="${1:-${FLUXMIRROR_KIND:-claude}}"

CACHE_DIR="${FLUXMIRROR_CACHE:-$HOME/.fluxmirror/cache}"
mkdir -p "$CACHE_DIR" 2>/dev/null

case "$(uname -s)" in
  Darwin) OS=darwin ;;
  Linux)  OS=linux  ;;
  *)      exit 0 ;;
esac

case "$(uname -m)" in
  arm64|aarch64) ARCH=arm64 ;;
  x86_64|amd64)  ARCH=x64   ;;
  *)             exit 0 ;;
esac

ASSET="fluxmirror-${OS}-${ARCH}"
BIN="$CACHE_DIR/$ASSET"

if [ ! -x "$BIN" ]; then
  command -v curl >/dev/null 2>&1 || exit 0
  url="https://github.com/OpenFluxGate/fluxmirror/releases/latest/download/$ASSET"
  if ! curl -fsSL --max-time 15 -o "$BIN.tmp" "$url" 2>/dev/null; then
    rm -f "$BIN.tmp"
    exit 0
  fi
  mv "$BIN.tmp" "$BIN" 2>/dev/null || { rm -f "$BIN.tmp"; exit 0; }
  chmod +x "$BIN" 2>/dev/null
fi

exec "$BIN" hook --kind "$KIND"
exit 0
