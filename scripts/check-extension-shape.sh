#!/usr/bin/env bash
# Static repo-shape guard.
#
# Asserts the worktree ships the right files in the right places so the
# four 2026-04-26 install regressions can never re-merge silently.
#
# Checks (each prints PASS / FAIL with the path):
#
#   a) Manifests exist (Claude / Qwen / Gemini) AND each has both a
#      `"name": "fluxmirror"` line and a `"version": "<semver>"` line.
#   b) Nested `commands/fluxmirror/` directories exist for both
#      Claude/Qwen and Gemini surfaces.
#   c) No flat slash-command files at the parent of `commands/`
#      (.md for plugins, .toml for gemini-extension).
#   d) No backup directories (`*.backup.*` / `*backup*`) checked into
#      the worktree under plugins/ or gemini-extension/.
#   e) `init.toml` and `init.md` interactive gate: the first
#      "STEP 1" / "Step 1" line in the prompt body sits BEFORE any
#      triple-backtick line.  Catches the failure mode where Gemini's
#      model raced past the "ask user" step and ran the non-interactive
#      shell anyway.
#   f) Both init files reference `wrapper probe` somewhere in their
#      body so the wrapper-engine question is grounded in real probe
#      output and not a hard-coded list.
#
# Usage:
#   bash scripts/check-extension-shape.sh
#
# Exit 0 on all-pass, 1 if any check fails.
# POSIX-friendly bash 3.2+.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

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
# (a) Manifests exist with name + version fields.
# ---------------------------------------------------------------------

MANIFESTS="
plugins/fluxmirror/.claude-plugin/plugin.json
plugins/fluxmirror/qwen-extension.json
gemini-extension/gemini-extension.json
"

# Match a quoted semver string: digits.dots, optional pre-release / build.
VERSION_PAT='"version"[[:space:]]*:[[:space:]]*"[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?"'
NAME_PAT='"name"[[:space:]]*:[[:space:]]*"fluxmirror"'

for rel_path in $MANIFESTS; do
    label="manifest: $rel_path"
    if [ ! -f "$rel_path" ]; then
        emit_fail "$label" "manifest missing"
        continue
    fi
    if ! grep -qE "$NAME_PAT" "$rel_path"; then
        emit_fail "$label" "missing \"name\": \"fluxmirror\""
        continue
    fi
    if ! grep -qE "$VERSION_PAT" "$rel_path"; then
        emit_fail "$label" "missing \"version\": \"<semver>\""
        continue
    fi
    emit_pass "$label"
done

# ---------------------------------------------------------------------
# (b) Nested commands/fluxmirror/ subdirectories exist.
# ---------------------------------------------------------------------

NESTED_DIRS="
plugins/fluxmirror/commands/fluxmirror
gemini-extension/commands/fluxmirror
"

for rel_path in $NESTED_DIRS; do
    label="nested commands dir: $rel_path"
    if [ -d "$rel_path" ]; then
        emit_pass "$label"
    else
        emit_fail "$label" "directory not found"
    fi
done

# ---------------------------------------------------------------------
# (c) No flat .md / .toml command files directly under commands/.
# ---------------------------------------------------------------------

flat_md=$(find plugins/fluxmirror/commands -maxdepth 1 -type f -name '*.md' 2>/dev/null)
if [ -z "$flat_md" ]; then
    emit_pass "no flat *.md under plugins/fluxmirror/commands/"
else
    emit_fail "no flat *.md under plugins/fluxmirror/commands/" "found: $(echo "$flat_md" | tr '\n' ' ')"
fi

flat_toml=$(find gemini-extension/commands -maxdepth 1 -type f -name '*.toml' 2>/dev/null)
if [ -z "$flat_toml" ]; then
    emit_pass "no flat *.toml under gemini-extension/commands/"
else
    emit_fail "no flat *.toml under gemini-extension/commands/" "found: $(echo "$flat_toml" | tr '\n' ' ')"
fi

# ---------------------------------------------------------------------
# (d) No backup directories under plugins/ or gemini-extension/.
# Match either `*.backup.*` or any directory whose basename contains
# "backup" anywhere.  Excludes node_modules-style virtual dirs.
# ---------------------------------------------------------------------

backup_dirs=$(find plugins gemini-extension \
    \( -type d -name '*.backup.*' -o -type d -name '*backup*' \) \
    2>/dev/null | sort -u)
if [ -z "$backup_dirs" ]; then
    emit_pass "no backup directories under plugins/ or gemini-extension/"
else
    emit_fail "no backup directories under plugins/ or gemini-extension/" \
              "found: $(echo "$backup_dirs" | tr '\n' ' ')"
fi

# ---------------------------------------------------------------------
# (e) init.toml / init.md interactive gate.
#
# For init.toml: Find the start of the prompt body.  The TOML uses
#   prompt = """
#   ...body...
#   """
# We start scanning the line AFTER the opening `prompt = """`.
#
# For init.md: the whole file body counts (after frontmatter); but for
# the purpose of the check we just scan the file from line 1.
#
# Then walk the body line-by-line.  If we hit a triple-backtick line
# BEFORE the first "STEP 1" / "Step 1" line, the check fails.  If we
# hit "STEP 1" / "Step 1" first, the check passes.  If we never hit
# either, the check fails (the gate text is gone).
# ---------------------------------------------------------------------

check_gate() {
    local file="$1"
    local prompt_start_pat="$2"   # awk regex matching the opening prompt fence; "" for plain files
    local label="interactive gate: $file"

    if [ ! -f "$file" ]; then
        emit_fail "$label" "file not found"
        return
    fi

    # Use awk to walk the file and answer: which appears first on a line
    # all by itself (after trimming) — a triple-backtick or a "STEP 1"
    # marker?  Returns "ok" / "fence-before-step1" / "no-step1".
    local result
    result=$(awk -v start_pat="$prompt_start_pat" '
        BEGIN {
            in_prompt = (start_pat == "" ? 1 : 0)
            decided = 0
        }
        {
            if (decided) next
            if (!in_prompt) {
                if (match($0, start_pat)) {
                    in_prompt = 1
                    next
                }
                next
            }
            line = $0
            # Match "STEP 1" or "Step 1" anywhere in the body line.
            if (line ~ /STEP 1/ || line ~ /Step 1/) {
                print "ok"
                decided = 1
                next
            }
            # Match a line that contains a triple-backtick.
            if (line ~ /```/) {
                print "fence-before-step1:" NR
                decided = 1
                next
            }
        }
        END {
            if (!decided) print "no-step1"
        }
    ' "$file")

    case "$result" in
        ok)
            emit_pass "$label"
            ;;
        fence-before-step1:*)
            emit_fail "$label" "shell fence appears at line ${result#fence-before-step1:} before any STEP 1 / Step 1 marker"
            ;;
        no-step1|"")
            emit_fail "$label" "no STEP 1 / Step 1 marker found in prompt body"
            ;;
        *)
            emit_fail "$label" "unexpected gate-check result: $result"
            ;;
    esac
}

# init.toml: prompt body opens with `prompt = """`.
check_gate "gemini-extension/commands/fluxmirror/init.toml" '^prompt[[:space:]]*=[[:space:]]*"""'

# init.md: scan from line 1; the YAML frontmatter does not contain
# triple-backticks or STEP markers, so it's harmless.
check_gate "plugins/fluxmirror/commands/fluxmirror/init.md" ''

# ---------------------------------------------------------------------
# (f) Both init files reference `wrapper probe`.
# ---------------------------------------------------------------------

INIT_FILES="
gemini-extension/commands/fluxmirror/init.toml
plugins/fluxmirror/commands/fluxmirror/init.md
"

for rel_path in $INIT_FILES; do
    label="wrapper probe reference: $rel_path"
    if [ ! -f "$rel_path" ]; then
        emit_fail "$label" "file not found"
        continue
    fi
    if grep -q 'wrapper probe' "$rel_path"; then
        emit_pass "$label"
    else
        emit_fail "$label" "string 'wrapper probe' not found"
    fi
done

# ---------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------

echo ""
echo "Results: $pass_count passed, $fail_count failed"

if [ "$fail_count" -gt 0 ]; then
    echo "check-extension-shape: $fail_count check(s) failed" >&2
    exit 1
fi

exit 0
