#!/bin/bash
# Synthetic regression tests for the FluxMirror hook scripts.
# Runs locally (`./scripts/test-hooks.sh`) and in CI.
#
# Each test feeds a synthetic JSON payload to a hook and asserts the
# resulting SQLite row matches expectations. No real CLI required.

set -u

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CLAUDE_HOOK="$REPO_ROOT/plugins/fluxmirror/hooks/session-log.sh"
GEMINI_HOOK="$REPO_ROOT/gemini-extension/hooks/session-log.sh"

PASS=0
FAIL=0

run_test() {
  local name="$1" hook="$2" input="$3" expect_field="$4" expect_value="$5"
  shift 5
  local db
  db=$(mktemp -t fmtest.XXXX.db)
  echo "$input" | FLUXMIRROR_DB="$db" "$@" bash "$hook" >/dev/null 2>&1
  local actual
  actual=$(sqlite3 "$db" "SELECT $expect_field FROM agent_events ORDER BY id DESC LIMIT 1" 2>/dev/null)
  if [ "$actual" = "$expect_value" ]; then
    echo "  PASS  $name  ($expect_field='$actual')"
    PASS=$((PASS + 1))
  else
    echo "  FAIL  $name  ($expect_field expected '$expect_value', got '$actual')"
    FAIL=$((FAIL + 1))
  fi
  rm -f "$db"
}

assert_count() {
  local name="$1" db="$2" expected="$3"
  local actual
  actual=$(sqlite3 "$db" "SELECT COUNT(*) FROM agent_events" 2>/dev/null || echo 0)
  if [ "$actual" = "$expected" ]; then
    echo "  PASS  $name  (count=$actual)"
    PASS=$((PASS + 1))
  else
    echo "  FAIL  $name  (count expected $expected, got $actual)"
    FAIL=$((FAIL + 1))
  fi
}

echo "=== Tool-aware detail extraction ==="

# Bash: command field, not description
run_test "Bash → command" "$CLAUDE_HOOK" \
  '{"tool_name":"Bash","tool_input":{"description":"Listing","command":"ls -la"},"session_id":"t1","cwd":"/tmp"}' \
  detail "ls -la"

# Gemini run_shell_command: command field
run_test "run_shell_command → command" "$GEMINI_HOOK" \
  '{"tool_name":"run_shell_command","tool_input":{"description":"Print hi","command":"echo hi"},"session_id":"t2","cwd":"/tmp"}' \
  detail "echo hi"

# Read: file_path
run_test "Read → file_path" "$CLAUDE_HOOK" \
  '{"tool_name":"Read","tool_input":{"file_path":"/etc/hosts"},"session_id":"t3","cwd":"/tmp"}' \
  detail "/etc/hosts"

# Gemini read_file: absolute_path
run_test "read_file → absolute_path" "$GEMINI_HOOK" \
  '{"tool_name":"read_file","tool_input":{"absolute_path":"/etc/hosts"},"session_id":"t4","cwd":"/tmp"}' \
  detail "/etc/hosts"

# Edit: file_path
run_test "Edit → file_path" "$CLAUDE_HOOK" \
  '{"tool_name":"Edit","tool_input":{"file_path":"/x/file.md","old_string":"a","new_string":"b"},"session_id":"t5","cwd":"/tmp"}' \
  detail "/x/file.md"

# Glob: pattern
run_test "Glob → pattern" "$CLAUDE_HOOK" \
  '{"tool_name":"Glob","tool_input":{"pattern":"**/*.md"},"session_id":"t6","cwd":"/tmp"}' \
  detail "**/*.md"

# WebFetch: url
run_test "WebFetch → url" "$CLAUDE_HOOK" \
  '{"tool_name":"WebFetch","tool_input":{"url":"https://example.com","prompt":"summary"},"session_id":"t7","cwd":"/tmp"}' \
  detail "https://example.com"

# WebSearch: query
run_test "WebSearch → query" "$CLAUDE_HOOK" \
  '{"tool_name":"WebSearch","tool_input":{"query":"hello world"},"session_id":"t8","cwd":"/tmp"}' \
  detail "hello world"

# TodoWrite: bracketed count
run_test "TodoWrite → [N todos]" "$CLAUDE_HOOK" \
  '{"tool_name":"TodoWrite","tool_input":{"todos":[{"a":1},{"a":2},{"a":3}]},"session_id":"t9","cwd":"/tmp"}' \
  detail "[3 todos]"

# Unknown tool: fallback to first string value
run_test "Unknown tool fallback" "$CLAUDE_HOOK" \
  '{"tool_name":"BrandNewTool","tool_input":{"foo":"bar","num":42},"session_id":"t10","cwd":"/tmp"}' \
  detail "bar"

echo ""
echo "=== Agent labeling ==="

# Claude (no QWEN env)
run_test "Claude default label" "$CLAUDE_HOOK" \
  '{"tool_name":"Bash","tool_input":{"command":"x"},"session_id":"a1","cwd":"/tmp"}' \
  agent "claude-code"

# Qwen via NO_RELAUNCH env
run_test "Qwen via NO_RELAUNCH" "$CLAUDE_HOOK" \
  '{"tool_name":"run_shell_command","tool_input":{"command":"x"},"session_id":"a2","cwd":"/tmp"}' \
  agent "qwen-code" \
  env QWEN_CODE_NO_RELAUNCH=true

# Qwen via PROJECT_DIR env
run_test "Qwen via PROJECT_DIR" "$CLAUDE_HOOK" \
  '{"tool_name":"read_file","tool_input":{"absolute_path":"/x"},"session_id":"a3","cwd":"/tmp"}' \
  agent "qwen-code" \
  env QWEN_PROJECT_DIR=/tmp

# Gemini hook is hardcoded gemini-cli
run_test "Gemini fixed label" "$GEMINI_HOOK" \
  '{"tool_name":"run_shell_command","tool_input":{"command":"x"},"session_id":"a4","cwd":"/tmp"}' \
  agent "gemini-cli"

echo ""
echo "=== Self-noise filter (anchored cwd) ==="

REPO_TMPROOT=$(mktemp -d -t fluxmirror-test.XXXX)
ADJ_TMPROOT=$(mktemp -d -t fluxmirror-test-adj.XXXX)

# In-repo + sqlite query, SKIP_SELF=1, SELF_REPO set → skipped
db=$(mktemp -t skip1.XXXX.db)
echo '{"tool_name":"Bash","tool_input":{"command":"sqlite3 events.db SELECT 1"},"session_id":"s1","cwd":"'"$REPO_TMPROOT"'"}' \
  | FLUXMIRROR_DB="$db" FLUXMIRROR_SKIP_SELF=1 FLUXMIRROR_SELF_REPO="$REPO_TMPROOT" bash "$CLAUDE_HOOK" >/dev/null 2>&1
assert_count "in-repo+sqlite → skipped" "$db" 0
rm -f "$db"

# In-repo + sqlite, but SKIP_SELF=0 → recorded
db=$(mktemp -t skip2.XXXX.db)
echo '{"tool_name":"Bash","tool_input":{"command":"sqlite3 events.db SELECT 1"},"session_id":"s2","cwd":"'"$REPO_TMPROOT"'"}' \
  | FLUXMIRROR_DB="$db" FLUXMIRROR_SELF_REPO="$REPO_TMPROOT" bash "$CLAUDE_HOOK" >/dev/null 2>&1
assert_count "default OFF → recorded" "$db" 1
rm -f "$db"

# Adjacent dir (similar prefix) — must NOT be falsely skipped
db=$(mktemp -t skip3.XXXX.db)
echo '{"tool_name":"Bash","tool_input":{"command":"sqlite3 events.db SELECT 1"},"session_id":"s3","cwd":"'"$ADJ_TMPROOT"'"}' \
  | FLUXMIRROR_DB="$db" FLUXMIRROR_SKIP_SELF=1 FLUXMIRROR_SELF_REPO="$REPO_TMPROOT" bash "$CLAUDE_HOOK" >/dev/null 2>&1
assert_count "adjacent dir not falsely skipped" "$db" 1
rm -f "$db"

# In-repo but Edit (not shell) → recorded
db=$(mktemp -t skip4.XXXX.db)
echo '{"tool_name":"Edit","tool_input":{"file_path":"/x"},"session_id":"s4","cwd":"'"$REPO_TMPROOT"'"}' \
  | FLUXMIRROR_DB="$db" FLUXMIRROR_SKIP_SELF=1 FLUXMIRROR_SELF_REPO="$REPO_TMPROOT" bash "$CLAUDE_HOOK" >/dev/null 2>&1
assert_count "in-repo + Edit → recorded" "$db" 1
rm -f "$db"

rm -rf "$REPO_TMPROOT" "$ADJ_TMPROOT"

echo ""
echo "=== Round-trip (raw_json byte preservation) ==="
db=$(mktemp -t roundtrip.XXXX.db)
INPUT='{"tool_name":"Bash","tool_input":{"command":"echo hi"},"session_id":"rt1","cwd":"/tmp"}'
echo "$INPUT" | FLUXMIRROR_DB="$db" bash "$CLAUDE_HOOK" >/dev/null 2>&1
ACTUAL=$(sqlite3 "$db" "SELECT raw_json FROM agent_events WHERE session='rt1'")
if [ "$ACTUAL" = "$INPUT" ]; then
  echo "  PASS  raw_json simple round-trip"
  PASS=$((PASS + 1))
else
  echo "  FAIL  raw_json simple (expected $(echo $INPUT | wc -c)b, got $(echo $ACTUAL | wc -c)b)"
  FAIL=$((FAIL + 1))
fi
rm -f "$db"

# Single quote in cwd (the historical sed-only escape failure mode)
db=$(mktemp -t roundtrip2.XXXX.db)
INPUT="{\"tool_name\":\"Bash\",\"tool_input\":{\"command\":\"x\"},\"session_id\":\"rt2\",\"cwd\":\"/path/with'quote\"}"
echo "$INPUT" | FLUXMIRROR_DB="$db" bash "$CLAUDE_HOOK" >/dev/null 2>&1
CWD_ACTUAL=$(sqlite3 "$db" "SELECT cwd FROM agent_events WHERE session='rt2'")
if [ "$CWD_ACTUAL" = "/path/with'quote" ]; then
  echo "  PASS  single-quote in cwd preserved"
  PASS=$((PASS + 1))
else
  echo "  FAIL  single-quote (got '$CWD_ACTUAL')"
  FAIL=$((FAIL + 1))
fi
rm -f "$db"

echo ""
echo "==============================================="
echo "  PASS: $PASS"
echo "  FAIL: $FAIL"
echo "==============================================="
[ "$FAIL" -eq 0 ]
