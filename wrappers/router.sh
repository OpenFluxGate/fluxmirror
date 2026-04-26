#!/bin/bash
# FluxMirror wrapper router.
#
# Tries each shim engine in priority order, exec's the first that's viable:
#   1. bash + shim.sh
#   2. node + shim.mjs
#   3. silent skip
#
# All "$@" (typically the kind: claude | gemini) and stdin forwarded.
# Always exits 0 — must never break the calling agent.

DIR="$(cd "$(dirname "$0")" && pwd)"

if command -v bash >/dev/null 2>&1 && [ -x "$DIR/shim.sh" ]; then
  exec bash "$DIR/shim.sh" "$@"
fi

if command -v node >/dev/null 2>&1 && [ -f "$DIR/shim.mjs" ]; then
  exec node "$DIR/shim.mjs" "$@"
fi

exit 0
