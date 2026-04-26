#!/usr/bin/env bash
# smoke-test-reports.sh — end-to-end reproducer for all seven fluxmirror reports.
#
# Usage:
#   bash scripts/smoke-test-reports.sh
#
# Behaviour:
#   1. Builds the debug binary if the target/debug/fluxmirror binary is absent.
#   2. Creates a fresh temp DB, runs `fluxmirror init --non-interactive` with a
#      demo row so the window is non-empty.
#   3. Runs all seven report subcommands plus the HTML card export.
#   4. Prints PASS/FAIL per report.
#   5. Returns 0 if all pass, 1 otherwise.
#
# All variable expansions are quoted. No inline comments after commands.
# Loop tokens contain no spaces or escapes.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$REPO_ROOT/target/debug/fluxmirror"
TEST_DB="$(mktemp /tmp/fm-smoke-XXXXXX.db)"
HTML_OUT="$(mktemp /tmp/fm-smoke-card-XXXXXX.html)"

cleanup() {
    rm -f "$TEST_DB" "$HTML_OUT"
}
trap cleanup EXIT

pass=0
fail=0

check() {
    local label="$1"
    shift
    if "$@" >/dev/null 2>&1; then
        echo "PASS  $label"
        pass=$((pass + 1))
    else
        echo "FAIL  $label"
        fail=$((fail + 1))
    fi
}

if [ ! -x "$BIN" ]; then
    echo "Binary not found — building debug binary..."
    cd "$REPO_ROOT"
    cargo build 2>&1
fi

FLUXMIRROR_DB="$TEST_DB" "$BIN" init \
    --non-interactive \
    --language english \
    --timezone UTC \
    >/dev/null 2>&1

check "today (english, UTC)" \
    env FLUXMIRROR_DB="$TEST_DB" "$BIN" today --tz UTC --lang english

check "today (korean, Asia/Seoul)" \
    env FLUXMIRROR_DB="$TEST_DB" "$BIN" today --tz Asia/Seoul --lang korean

check "yesterday (english, UTC)" \
    env FLUXMIRROR_DB="$TEST_DB" "$BIN" yesterday --tz UTC --lang english

check "week (english, UTC)" \
    env FLUXMIRROR_DB="$TEST_DB" "$BIN" week --tz UTC --lang english

check "week --format html (card to file)" \
    env FLUXMIRROR_DB="$TEST_DB" "$BIN" week --tz UTC --lang english --format html --out "$HTML_OUT"

check "agents (english, UTC)" \
    env FLUXMIRROR_DB="$TEST_DB" "$BIN" agents --tz UTC --lang english

check "agent setup (today)" \
    env FLUXMIRROR_DB="$TEST_DB" "$BIN" agent setup --tz UTC --lang english

check "compare (english, UTC)" \
    env FLUXMIRROR_DB="$TEST_DB" "$BIN" compare --tz UTC --lang english

check "about (english, no --tz)" \
    env FLUXMIRROR_DB="$TEST_DB" "$BIN" about --lang english

check "about --tz Asia/Seoul --lang korean (uniform flag acceptance)" \
    env FLUXMIRROR_DB="$TEST_DB" "$BIN" about --tz Asia/Seoul --lang korean

echo ""
echo "Results: $pass passed, $fail failed"

if [ "$fail" -gt 0 ]; then
    exit 1
fi
exit 0
