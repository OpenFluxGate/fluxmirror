#!/bin/bash
# FluxMirror PostToolUse entrypoint (Claude Code / Qwen Code).
#
# Prefers the per-arch Rust binary (bin/fluxmirror-hook-<os>-<arch>),
# then a generic local-dev binary (bin/fluxmirror-hook), and finally
# falls back to the pure-bash session-log.sh — so the hook always works
# regardless of whether the Rust binary is installed.
#
# Stdin (the JSON tool-call payload) is forwarded unchanged.

PLUGIN_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64)   ARCH=darwin-arm64  ;;
  Darwin-x86_64)  ARCH=darwin-x64    ;;
  Linux-x86_64)   ARCH=linux-x64     ;;
  Linux-aarch64)  ARCH=linux-arm64   ;;
  *)              ARCH=              ;;
esac

# Try (in order):
#   1. arch-specific binary as shipped by GitHub release
#   2. generic name (used by `make install-bin` and local builds)
candidates=()
[ -n "$ARCH" ] && candidates+=("$PLUGIN_ROOT/bin/fluxmirror-hook-$ARCH")
candidates+=("$PLUGIN_ROOT/bin/fluxmirror-hook")

for candidate in "${candidates[@]}"; do
  if [ -x "$candidate" ]; then
    exec "$candidate" --kind claude
  fi
done

# Fallback: pure-bash + jq + python3 implementation
exec "$PLUGIN_ROOT/hooks/session-log.sh"
