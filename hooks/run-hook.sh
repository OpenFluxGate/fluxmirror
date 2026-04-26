#!/bin/bash
# FluxMirror AfterTool entrypoint (Gemini CLI).
#
# Auto-downloads the per-arch fluxmirror-hook binary from the latest
# GitHub release on first invocation (one-time ~1.2 MB), then execs it.
# Subsequent calls skip the download and exec the cached binary.
#
# Required: bash + curl (both universal).

PLUGIN_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$PLUGIN_ROOT/bin" 2>/dev/null

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64)   ASSET=fluxmirror-hook-darwin-arm64 ;;
  Darwin-x86_64)  ASSET=fluxmirror-hook-darwin-x64 ;;
  Linux-x86_64)   ASSET=fluxmirror-hook-linux-x64 ;;
  Linux-aarch64)  ASSET=fluxmirror-hook-linux-arm64 ;;
  *)
    exit 0
    ;;
esac

BIN="$PLUGIN_ROOT/bin/$ASSET"

if [ ! -x "$BIN" ]; then
  url="https://github.com/OpenFluxGate/fluxmirror/releases/latest/download/$ASSET"
  if ! curl -fsSL --max-time 15 -o "$BIN.tmp" "$url" 2>/dev/null; then
    rm -f "$BIN.tmp"
    exit 0
  fi
  mv "$BIN.tmp" "$BIN"
  chmod +x "$BIN"
fi

exec "$BIN" hook --kind gemini
