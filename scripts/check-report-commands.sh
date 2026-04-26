#!/usr/bin/env bash
# Verify that the 14 migrated report slash-command files stay in the
# post-M1 wrapper shape:
#   - <= 20 lines
#   - no embedded shell loops or SQL
#   - at least one `fluxmirror <subcommand>` invocation
#
# Usage:
#   bash scripts/check-report-commands.sh
#
# Exit 0 on all-pass, 1 on any failure.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

REPORT_FILES="
plugins/fluxmirror/commands/fluxmirror/about.md
plugins/fluxmirror/commands/fluxmirror/agent.md
plugins/fluxmirror/commands/fluxmirror/agents.md
plugins/fluxmirror/commands/fluxmirror/compare.md
plugins/fluxmirror/commands/fluxmirror/report-today.md
plugins/fluxmirror/commands/fluxmirror/week.md
plugins/fluxmirror/commands/fluxmirror/yesterday.md
gemini-extension/commands/fluxmirror/about.toml
gemini-extension/commands/fluxmirror/agent.toml
gemini-extension/commands/fluxmirror/agents.toml
gemini-extension/commands/fluxmirror/compare.toml
gemini-extension/commands/fluxmirror/today.toml
gemini-extension/commands/fluxmirror/week.toml
gemini-extension/commands/fluxmirror/yesterday.toml
"

# Regex patterns (POSIX ERE via grep -E).
#
# Shell-loop / SQL tokens must appear as standalone words: they must be
# preceded by a non-alphanumeric/non-underscore character (or start-of-line)
# AND followed by a space or end-of-line.  This prevents false positives on
# words like "forward", "agents" (contains "sed"), or description prose like
# "for the past 7 days".
#
# Strategy: run two separate checks so each pattern is independently anchored.
#
# Use semantically tight patterns that match shell constructs but not prose:
#   for  - only as a shell for-in loop: "for <word> in "
#   while - only as a shell while loop: "while [", "while (", "while read",
#           "while true", "while false"
#   awk/sed - at start-of-line (with optional leading spaces) or after a
#             shell metacharacter (;|&()`)
#   sqlite  - the sqlite3 CLI command
FORBIDDEN_LOOP_PAT='(^|[;|&(`])[[:space:]]*(awk|sed)[[:space:]]|[[:space:]]for[[:space:]]+[^[:space:]]+[[:space:]]+in[[:space:]]|(^|[;|&(`])[[:space:]]*for[[:space:]]+[^[:space:]]+[[:space:]]+in[[:space:]]|while[[:space:]]+(\[|\(|read|true|false)'
FORBIDDEN_SQL_PAT='(^|[^[:alnum:]_])(sqlite3?[[:space:]]|sqlite3?$|fluxmirror[[:space:]]+sqlite)'
REQUIRED_PAT='fluxmirror[[:space:]]+(today|yesterday|week|compare|agents|agent|about)'

pass_count=0
fail_count=0

for rel_path in $REPORT_FILES; do
    file="$REPO_ROOT/$rel_path"
    result="PASS"
    reason=""

    if [ ! -f "$file" ]; then
        result="FAIL"
        reason="file not found"
        echo "FAIL  $rel_path  [$reason]"
        fail_count=$(( fail_count + 1 ))
        continue
    fi

    # Check line count (<= 20).
    line_count="$(wc -l < "$file")"
    # wc -l counts newlines; a file with no trailing newline may read one less.
    # Add 1 to be safe for files without a trailing newline.
    if [ "$line_count" -gt 20 ]; then
        result="FAIL"
        reason="too many lines ($line_count > 20)"
    fi

    # Check for forbidden patterns (two separate passes to keep anchoring clean).
    if grep -qE "$FORBIDDEN_LOOP_PAT" "$file" || grep -qE "$FORBIDDEN_SQL_PAT" "$file"; then
        result="FAIL"
        if [ -n "$reason" ]; then
            reason="$reason; contains forbidden shell/SQL"
        else
            reason="contains forbidden shell/SQL"
        fi
    fi

    # Check for required binary invocation.
    if ! grep -qE "$REQUIRED_PAT" "$file"; then
        result="FAIL"
        if [ -n "$reason" ]; then
            reason="$reason; missing fluxmirror <subcommand> invocation"
        else
            reason="missing fluxmirror <subcommand> invocation"
        fi
    fi

    if [ "$result" = "PASS" ]; then
        echo "PASS  $rel_path"
        pass_count=$(( pass_count + 1 ))
    else
        echo "FAIL  $rel_path  [$reason]"
        fail_count=$(( fail_count + 1 ))
    fi
done

echo ""
echo "Results: $pass_count passed, $fail_count failed"

if [ "$fail_count" -gt 0 ]; then
    echo "check-report-commands: $fail_count file(s) failed shape check" >&2
    exit 1
fi

exit 0
