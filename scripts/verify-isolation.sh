#!/bin/bash
# Verify that Claude / Gemini / Qwen session-logs are fully isolated
# (no session ID leaks across directories).
#
# Usage:
#   ./scripts/verify-isolation.sh              # check today's logs
#   ./scripts/verify-isolation.sh 2026-04-22   # check a specific date

set -e

# --- Dependencies ---
if ! command -v jq &>/dev/null; then
  echo "Error: jq is required but not found on PATH." >&2
  echo "Install it with: brew install jq" >&2
  exit 1
fi

DATE="${1:-$(date -u +%Y-%m-%d)}"
CLAUDE_LOG="$HOME/.claude/session-logs/$DATE.jsonl"
GEMINI_LOG="$HOME/.gemini/session-logs/$DATE.jsonl"
QWEN_LOG="$HOME/.qwen/session-logs/$DATE.jsonl"

echo "==============================================="
echo "fluxmirror isolation check — $DATE"
echo "==============================================="
echo ""

# --- Check if any logs exist at all ---
if [ ! -f "$CLAUDE_LOG" ] && [ ! -f "$GEMINI_LOG" ] && [ ! -f "$QWEN_LOG" ]; then
  echo "No session-log files found for $DATE."
  echo ""
  echo "Expected locations:"
  echo "  Claude: $CLAUDE_LOG"
  echo "  Gemini: $GEMINI_LOG"
  echo "  Qwen:   $QWEN_LOG"
  echo ""
  echo "If fluxmirror is not installed yet, see:"
  echo "  Claude — plugins/fluxmirror/README.md"
  echo "  Gemini — gemini-extension/README.md"
  echo "  Qwen   — uses the Claude plugin (see plugins/fluxmirror/README.md)"
  exit 0
fi

# --- 1. File presence and line counts ---
echo "## File presence and line counts"
for label in "Claude:$CLAUDE_LOG" "Gemini:$GEMINI_LOG" "Qwen:$QWEN_LOG"; do
  name="${label%%:*}"
  path="${label#*:}"
  if [ -f "$path" ]; then
    count=$(wc -l < "$path" | tr -d ' ')
    echo "  $name: $count lines  ($path)"
  else
    echo "  $name: NOT FOUND  ($path)"
  fi
done
echo ""

# --- 2. Unique session IDs per file ---
echo "## Unique session IDs per file"
for label in "Claude:$CLAUDE_LOG" "Gemini:$GEMINI_LOG" "Qwen:$QWEN_LOG"; do
  name="${label%%:*}"
  path="${label#*:}"
  if [ -f "$path" ]; then
    sessions=$(jq -r '.session' "$path" 2>/dev/null | sort -u)
    count=$(echo "$sessions" | grep -c .)
    echo "  $name: $count unique sessions"
    echo "$sessions" | sed 's/^/    /'
  fi
done
echo ""

# --- 3. Cross-contamination check ---
echo "## Cross-contamination check"

TOTAL_LEAKS=0

check_leak() {
  local from_name="$1"
  local from_path="$2"
  local to_name="$3"
  local to_path="$4"

  if [ ! -f "$from_path" ] || [ ! -f "$to_path" ]; then
    echo "  $from_name → $to_name: SKIP (one or both files missing)"
    return
  fi

  local leaks=0
  while IFS= read -r sid; do
    if [ -n "$sid" ]; then
      if grep -q "\"session\":\"$sid\"" "$to_path" 2>/dev/null; then
        leaks=$((leaks + 1))
        TOTAL_LEAKS=$((TOTAL_LEAKS + 1))
        echo "  ⚠️  LEAK: $from_name session $sid found in $to_name log"
      fi
    fi
  done < <(jq -r '.session' "$from_path" 2>/dev/null | sort -u)

  if [ $leaks -eq 0 ]; then
    echo "  ✓  $from_name → $to_name: clean (0 session IDs cross over)"
  fi
}

check_leak "Claude" "$CLAUDE_LOG" "Gemini" "$GEMINI_LOG"
check_leak "Claude" "$CLAUDE_LOG" "Qwen"   "$QWEN_LOG"
check_leak "Gemini" "$GEMINI_LOG" "Claude" "$CLAUDE_LOG"
check_leak "Gemini" "$GEMINI_LOG" "Qwen"   "$QWEN_LOG"
check_leak "Qwen"   "$QWEN_LOG"   "Claude" "$CLAUDE_LOG"
check_leak "Qwen"   "$QWEN_LOG"   "Gemini" "$GEMINI_LOG"
echo ""

# --- 4. Tool name distribution ---
echo "## Tool name distribution per agent"
for label in "Claude:$CLAUDE_LOG" "Gemini:$GEMINI_LOG" "Qwen:$QWEN_LOG"; do
  name="${label%%:*}"
  path="${label#*:}"
  if [ -f "$path" ]; then
    echo "  $name:"
    jq -r '.tool' "$path" 2>/dev/null | sort | uniq -c | sort -rn | sed 's/^/    /'
  fi
done
echo ""

# --- 5. SQLite agent_events isolation (DB layer) ---
echo "## SQLite agent_events isolation (DB layer)"
DB_PATH="${FLUXMIRROR_DB:-$HOME/Library/Application Support/fluxmirror/events.db}"
if ! command -v sqlite3 &>/dev/null; then
  echo "  SKIP — sqlite3 not on PATH"
elif [ ! -f "$DB_PATH" ]; then
  echo "  SKIP — DB not found at $DB_PATH"
else
  # Range filter: same date as JSONL check, in UTC
  RANGE_START="${DATE}T00:00:00Z"
  RANGE_END=$(python3 -c "
from datetime import datetime, timedelta
d = datetime.strptime('$DATE', '%Y-%m-%d') + timedelta(days=1)
print(d.strftime('%Y-%m-%dT%H:%M:%SZ'))
")
  echo "  Window: $RANGE_START to $RANGE_END"

  # Per-agent session ID counts
  echo "  Sessions per agent:"
  sqlite3 "$DB_PATH" "
    SELECT '    ' || agent || ': ' || COUNT(DISTINCT session) || ' unique sessions, ' || COUNT(*) || ' rows'
    FROM agent_events
    WHERE ts >= '$RANGE_START' AND ts < '$RANGE_END'
    GROUP BY agent
    ORDER BY agent" 2>/dev/null

  # Cross-agent session leak check
  LEAKS=$(sqlite3 "$DB_PATH" "
    SELECT session, GROUP_CONCAT(DISTINCT agent) AS agents, COUNT(DISTINCT agent) AS n
    FROM agent_events
    WHERE ts >= '$RANGE_START' AND ts < '$RANGE_END' AND session IS NOT NULL AND session != 'unknown'
    GROUP BY session
    HAVING n >= 2" 2>/dev/null)

  if [ -n "$LEAKS" ]; then
    echo "$LEAKS" | while IFS='|' read -r sid agents n; do
      echo "  ⚠️  DB LEAK: session $sid shared by $n agents ($agents)"
      TOTAL_LEAKS=$((TOTAL_LEAKS + 1))
    done
    # Re-count for accurate total since the subshell pipe doesn't propagate
    DB_LEAK_COUNT=$(echo "$LEAKS" | grep -c .)
    TOTAL_LEAKS=$((TOTAL_LEAKS + DB_LEAK_COUNT))
  else
    echo "  ✓  DB layer: clean (no session_id shared across agents)"
  fi
fi
echo ""

echo "==============================================="
if [ "$TOTAL_LEAKS" -gt 0 ]; then
  echo "FAIL: $TOTAL_LEAKS session ID leak(s) detected."
  echo "==============================================="
  exit 1
else
  echo "PASS: all logs are cleanly isolated."
  echo "==============================================="
  exit 0
fi
