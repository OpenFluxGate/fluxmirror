#!/bin/bash
# FluxMirror AfterTool entrypoint (Gemini CLI).
#
# Prefers the per-arch Rust binary (bin/fluxmirror-hook-<os>-<arch>),
# then a generic local-dev binary (bin/fluxmirror-hook), and finally
# falls back to the pure-bash session-log.sh — so the hook always works
# regardless of whether the Rust binary is installed.

PLUGIN_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64)   ARCH=darwin-arm64  ;;
  Darwin-x86_64)  ARCH=darwin-x64    ;;
  Linux-x86_64)   ARCH=linux-x64     ;;
  Linux-aarch64)  ARCH=linux-arm64   ;;
  *)              ARCH=              ;;
esac

candidates=()
[ -n "$ARCH" ] && candidates+=("$PLUGIN_ROOT/bin/fluxmirror-hook-$ARCH")
candidates+=("$PLUGIN_ROOT/bin/fluxmirror-hook")

for candidate in "${candidates[@]}"; do
  if [ -x "$candidate" ]; then
    exec "$candidate" --kind gemini
  fi
done

exec "$PLUGIN_ROOT/hooks/session-log.sh"
