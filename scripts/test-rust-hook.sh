#!/bin/bash
# Parity tests for the Rust binary (target/release/fluxmirror).
# Mirrors scripts/test-hooks.sh case-by-case against the Rust impl so that
# any divergence between the bash hook and the Rust hook is caught early.
#
# Build the binary first (from repo root):
#   cargo build --release --workspace
#
# Then:
#   ./scripts/test-rust-hook.sh

set -u

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$REPO_ROOT/target/release/fluxmirror"

if [ ! -x "$BIN" ]; then
  echo "Binary not found at $BIN"
  echo "Build with: cargo build --release --workspace"
  exit 1
fi

PASS=0
FAIL=0

run_test() {
  local name="$1" kind="$2" input="$3" expect_field="$4" expect_value="$5"
  shift 5
  local db
  db=$(mktemp -t fmrt.XXXX.db)
  echo "$input" | FLUXMIRROR_DB="$db" "$@" "$BIN" hook --kind "$kind" >/dev/null 2>&1
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

run_test "Bash → command" claude \
  '{"tool_name":"Bash","tool_input":{"description":"Listing","command":"ls -la"},"session_id":"t1","cwd":"/tmp"}' \
  detail "ls -la"

run_test "run_shell_command → command" gemini \
  '{"tool_name":"run_shell_command","tool_input":{"description":"Print hi","command":"echo hi"},"session_id":"t2","cwd":"/tmp"}' \
  detail "echo hi"

run_test "Read → file_path" claude \
  '{"tool_name":"Read","tool_input":{"file_path":"/etc/hosts"},"session_id":"t3","cwd":"/tmp"}' \
  detail "/etc/hosts"

run_test "read_file → absolute_path" gemini \
  '{"tool_name":"read_file","tool_input":{"absolute_path":"/etc/hosts"},"session_id":"t4","cwd":"/tmp"}' \
  detail "/etc/hosts"

run_test "Edit → file_path" claude \
  '{"tool_name":"Edit","tool_input":{"file_path":"/x/file.md","old_string":"a","new_string":"b"},"session_id":"t5","cwd":"/tmp"}' \
  detail "/x/file.md"

run_test "Glob → pattern" claude \
  '{"tool_name":"Glob","tool_input":{"pattern":"**/*.md"},"session_id":"t6","cwd":"/tmp"}' \
  detail "**/*.md"

run_test "WebFetch → url" claude \
  '{"tool_name":"WebFetch","tool_input":{"url":"https://example.com","prompt":"summary"},"session_id":"t7","cwd":"/tmp"}' \
  detail "https://example.com"

run_test "WebSearch → query" claude \
  '{"tool_name":"WebSearch","tool_input":{"query":"hello world"},"session_id":"t8","cwd":"/tmp"}' \
  detail "hello world"

run_test "TodoWrite → [N todos]" claude \
  '{"tool_name":"TodoWrite","tool_input":{"todos":[{"a":1},{"a":2},{"a":3}]},"session_id":"t9","cwd":"/tmp"}' \
  detail "[3 todos]"

run_test "Unknown tool fallback" claude \
  '{"tool_name":"BrandNewTool","tool_input":{"foo":"bar","num":42},"session_id":"t10","cwd":"/tmp"}' \
  detail "bar"

echo ""
echo "=== Agent labeling ==="

run_test "Claude default label" claude \
  '{"tool_name":"Bash","tool_input":{"command":"x"},"session_id":"a1","cwd":"/tmp"}' \
  agent "claude-code"

run_test "Qwen via NO_RELAUNCH" claude \
  '{"tool_name":"run_shell_command","tool_input":{"command":"x"},"session_id":"a2","cwd":"/tmp"}' \
  agent "qwen-code" \
  env QWEN_CODE_NO_RELAUNCH=true

run_test "Qwen via PROJECT_DIR" claude \
  '{"tool_name":"read_file","tool_input":{"absolute_path":"/x"},"session_id":"a3","cwd":"/tmp"}' \
  agent "qwen-code" \
  env QWEN_PROJECT_DIR=/tmp

run_test "Gemini fixed label" gemini \
  '{"tool_name":"run_shell_command","tool_input":{"command":"x"},"session_id":"a4","cwd":"/tmp"}' \
  agent "gemini-cli"

echo ""
echo "=== Self-noise filter (anchored cwd) ==="

REPO_TMPROOT=$(mktemp -d -t fluxmirror-rust-test.XXXX)
ADJ_TMPROOT=$(mktemp -d -t fluxmirror-rust-test-adj.XXXX)

# In-repo + sqlite query, SKIP_SELF=1 → skipped
db=$(mktemp -t skip1.XXXX.db)
echo '{"tool_name":"Bash","tool_input":{"command":"sqlite3 events.db SELECT 1"},"session_id":"s1","cwd":"'"$REPO_TMPROOT"'"}' \
  | FLUXMIRROR_DB="$db" FLUXMIRROR_SKIP_SELF=1 FLUXMIRROR_SELF_REPO="$REPO_TMPROOT" "$BIN" hook --kind claude >/dev/null 2>&1
assert_count "in-repo+sqlite → skipped" "$db" 0
rm -f "$db"

# In-repo + sqlite, SKIP_SELF unset → recorded
db=$(mktemp -t skip2.XXXX.db)
echo '{"tool_name":"Bash","tool_input":{"command":"sqlite3 events.db SELECT 1"},"session_id":"s2","cwd":"'"$REPO_TMPROOT"'"}' \
  | FLUXMIRROR_DB="$db" FLUXMIRROR_SELF_REPO="$REPO_TMPROOT" "$BIN" hook --kind claude >/dev/null 2>&1
assert_count "default OFF → recorded" "$db" 1
rm -f "$db"

# Adjacent dir (similar prefix) — must NOT be falsely skipped
db=$(mktemp -t skip3.XXXX.db)
echo '{"tool_name":"Bash","tool_input":{"command":"sqlite3 events.db SELECT 1"},"session_id":"s3","cwd":"'"$ADJ_TMPROOT"'"}' \
  | FLUXMIRROR_DB="$db" FLUXMIRROR_SKIP_SELF=1 FLUXMIRROR_SELF_REPO="$REPO_TMPROOT" "$BIN" hook --kind claude >/dev/null 2>&1
assert_count "adjacent dir not falsely skipped" "$db" 1
rm -f "$db"

# In-repo but Edit (not shell) → recorded
db=$(mktemp -t skip4.XXXX.db)
echo '{"tool_name":"Edit","tool_input":{"file_path":"/x"},"session_id":"s4","cwd":"'"$REPO_TMPROOT"'"}' \
  | FLUXMIRROR_DB="$db" FLUXMIRROR_SKIP_SELF=1 FLUXMIRROR_SELF_REPO="$REPO_TMPROOT" "$BIN" hook --kind claude >/dev/null 2>&1
assert_count "in-repo + Edit → recorded" "$db" 1
rm -f "$db"

rm -rf "$REPO_TMPROOT" "$ADJ_TMPROOT"

echo ""
echo "=== Round-trip (raw_json byte preservation) ==="
db=$(mktemp -t roundtrip.XXXX.db)
INPUT='{"tool_name":"Bash","tool_input":{"command":"echo hi"},"session_id":"rt1","cwd":"/tmp"}'
echo "$INPUT" | FLUXMIRROR_DB="$db" "$BIN" hook --kind claude >/dev/null 2>&1
ACTUAL=$(sqlite3 "$db" "SELECT raw_json FROM agent_events WHERE session='rt1'")
if [ "$ACTUAL" = "$INPUT" ]; then
  echo "  PASS  raw_json simple round-trip"
  PASS=$((PASS + 1))
else
  echo "  FAIL  raw_json simple"
  echo "    expected: $INPUT"
  echo "    got:      $ACTUAL"
  FAIL=$((FAIL + 1))
fi
rm -f "$db"

db=$(mktemp -t roundtrip2.XXXX.db)
INPUT="{\"tool_name\":\"Bash\",\"tool_input\":{\"command\":\"x\"},\"session_id\":\"rt2\",\"cwd\":\"/path/with'quote\"}"
echo "$INPUT" | FLUXMIRROR_DB="$db" "$BIN" hook --kind claude >/dev/null 2>&1
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
echo "  Rust binary parity: PASS=$PASS  FAIL=$FAIL"
echo "==============================================="
[ "$FAIL" -eq 0 ]
