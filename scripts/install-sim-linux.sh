#!/usr/bin/env bash
# Live install-simulation rehearsal.
#
# Stands up a fresh-user style install of fluxmirror under a sandbox
# $HOME=/tmp/fm-installsim-home so the rehearsal cannot disturb the
# developer's real ~/.gemini or ~/.qwen state.  Provisions the
# upstream npm CLIs (gemini-cli, qwen-code) if they are not already
# on PATH, drops the local repo's gemini-extension/ and
# plugins/fluxmirror/ into the layouts that each tool's loader
# expects, then exercises `extensions list` to confirm fluxmirror
# registered with at least 5 /fluxmirror:* commands.
#
# Designed to run inside the Ubuntu CI runner, and locally on Linux /
# WSL.  Do NOT run on macOS — Homebrew / system npm differences are
# not in scope until Phase 3 M8.
#
# Exit 0 on full pass, 1 on any failure.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

SANDBOX_HOME="/tmp/fm-installsim-home"
NPM_FALLBACK_PREFIX="/tmp/fmnpm"

pass_count=0
fail_count=0

emit_pass() {
    echo "PASS  $1"
    pass_count=$(( pass_count + 1 ))
}

emit_fail() {
    echo "FAIL  $1  [$2]"
    fail_count=$(( fail_count + 1 ))
}

# ---------------------------------------------------------------------
# 0) Sandbox / cleanup helpers.
# ---------------------------------------------------------------------

cleanup() {
    rm -rf "$SANDBOX_HOME" 2>/dev/null || true
}
trap cleanup EXIT

cleanup
mkdir -p "$SANDBOX_HOME"

# ---------------------------------------------------------------------
# 1) Provision gemini-cli + qwen-code via npm if absent.
#
# We try `npm install -g` first (works on CI runners with an
# unprivileged-writable /usr/local/lib/node_modules or a
# user-managed npm prefix).  If that fails (typical local Linux
# setup that demands sudo), we retry with `--prefix /tmp/fmnpm` and
# prepend $NPM_FALLBACK_PREFIX/bin to PATH for the rest of the
# script.
# ---------------------------------------------------------------------

ensure_npm_pkg() {
    local cli="$1"
    local pkg="$2"

    if command -v "$cli" >/dev/null 2>&1; then
        echo "ensure_npm_pkg: $cli already on PATH ($(command -v "$cli"))"
        return 0
    fi

    if ! command -v npm >/dev/null 2>&1; then
        echo "ensure_npm_pkg: npm not found on PATH; cannot install $pkg" >&2
        return 1
    fi

    echo "ensure_npm_pkg: installing $pkg globally"
    if npm install -g "$pkg" >/tmp/fm-npm-${cli}.log 2>&1; then
        echo "ensure_npm_pkg: $pkg installed via npm install -g"
        return 0
    fi

    echo "ensure_npm_pkg: global install failed, retrying with --prefix $NPM_FALLBACK_PREFIX"
    mkdir -p "$NPM_FALLBACK_PREFIX"
    if npm install --prefix "$NPM_FALLBACK_PREFIX" -g "$pkg" >>/tmp/fm-npm-${cli}.log 2>&1; then
        export PATH="$NPM_FALLBACK_PREFIX/bin:$PATH"
        echo "ensure_npm_pkg: $pkg installed via fallback prefix; PATH updated"
        return 0
    fi

    echo "ensure_npm_pkg: both install attempts failed for $pkg; tail of log:" >&2
    tail -n 30 "/tmp/fm-npm-${cli}.log" >&2 || true
    return 1
}

if ! ensure_npm_pkg gemini "@google/gemini-cli"; then
    emit_fail "provision: gemini-cli" "npm install failed (see /tmp/fm-npm-gemini.log)"
else
    emit_pass "provision: gemini-cli ($(command -v gemini || echo 'still missing'))"
fi

if ! ensure_npm_pkg qwen "@qwen-code/qwen-code"; then
    emit_fail "provision: qwen-code" "npm install failed (see /tmp/fm-npm-qwen.log)"
else
    emit_pass "provision: qwen-code ($(command -v qwen || echo 'still missing'))"
fi

# ---------------------------------------------------------------------
# 2) Stage the simulated installs under a sandbox HOME.
# ---------------------------------------------------------------------

export HOME="$SANDBOX_HOME"
mkdir -p "$HOME/.gemini/extensions/fluxmirror"
mkdir -p "$HOME/.qwen/extensions/fluxmirror"

# Gemini: copy gemini-extension/* — this matches what
# `gemini extensions install <repo>` produces on disk.
if cp -R "$REPO_ROOT/gemini-extension/." "$HOME/.gemini/extensions/fluxmirror/"; then
    emit_pass "stage: gemini-extension -> $HOME/.gemini/extensions/fluxmirror/"
else
    emit_fail "stage: gemini-extension -> $HOME/.gemini/extensions/fluxmirror/" "cp failed"
fi

# Qwen: copy plugins/fluxmirror/* — mirrors how the marketplace
# installer would land a Claude marketplace plugin under qwen's
# extensions dir.  Qwen's FileCommandLoader reads commands/
# directories with the same shape as Claude Code.
if cp -R "$REPO_ROOT/plugins/fluxmirror/." "$HOME/.qwen/extensions/fluxmirror/"; then
    emit_pass "stage: plugins/fluxmirror -> $HOME/.qwen/extensions/fluxmirror/"
else
    emit_fail "stage: plugins/fluxmirror -> $HOME/.qwen/extensions/fluxmirror/" "cp failed"
fi

# ---------------------------------------------------------------------
# 3) Assertions.
# ---------------------------------------------------------------------

assert_extensions_list() {
    local cli="$1"
    local label="$cli extensions list"

    if ! command -v "$cli" >/dev/null 2>&1; then
        emit_fail "$label" "$cli not on PATH after provision step"
        return
    fi

    local out
    if ! out="$("$cli" extensions list 2>&1)"; then
        emit_fail "$label" "exit code != 0; output: $(echo "$out" | head -n 5 | tr '\n' '|')"
        return
    fi

    if ! echo "$out" | grep -q 'fluxmirror'; then
        emit_fail "$label" "stdout did not mention fluxmirror"
        return
    fi

    local cmd_lines
    cmd_lines=$(echo "$out" | grep -cE '(^|[[:space:]])/fluxmirror:' || true)
    # `grep -c` returns the count even on no-match (just 0), but with
    # `set -o pipefail` and an empty pipe it can still propagate; the
    # `|| true` above guards that.
    if [ -z "$cmd_lines" ]; then cmd_lines=0; fi
    if [ "$cmd_lines" -lt 5 ]; then
        emit_fail "$label" "found only $cmd_lines /fluxmirror:* command lines (need >= 5)"
        return
    fi

    emit_pass "$label  (found $cmd_lines /fluxmirror:* commands)"
}

assert_extensions_list gemini
assert_extensions_list qwen

# 3c) No `*.backup.*` directories under either extensions dir.
backup_after_install=$(find "$HOME/.gemini/extensions" "$HOME/.qwen/extensions" \
    \( -type d -name '*.backup.*' -o -type d -name '*backup*' \) \
    2>/dev/null | sort -u)
if [ -z "$backup_after_install" ]; then
    emit_pass "no backup directories under sandbox extensions dirs"
else
    emit_fail "no backup directories under sandbox extensions dirs" \
              "found: $(echo "$backup_after_install" | tr '\n' ' ')"
fi

# 3d) Re-run the static repo-shape guard.  This re-asserts the four
# 2026-04-26 regression checks against the actual sources of truth.
echo ""
echo "--- delegating to scripts/check-extension-shape.sh ---"
if bash "$SCRIPT_DIR/check-extension-shape.sh"; then
    emit_pass "static repo-shape guard"
else
    emit_fail "static repo-shape guard" "see output above"
fi
echo "--- end delegation ---"
echo ""

# ---------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------

echo ""
echo "Results: $pass_count passed, $fail_count failed"

if [ "$fail_count" -gt 0 ]; then
    echo "install-sim-linux: $fail_count check(s) failed" >&2
    exit 1
fi

exit 0
