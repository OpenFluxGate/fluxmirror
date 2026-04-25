#!/bin/bash
# today-report.sh — summarize today's AI activity from FluxMirror SQLite
set -euo pipefail

DB_PATH="${FLUXMIRROR_DB:-$HOME/Library/Application Support/fluxmirror/events.db}"
FORMAT="text"
DATE=$(date -u +%Y-%m-%d)

while [[ $# -gt 0 ]]; do
  case "$1" in
    --toml)  FORMAT="toml"; shift ;;
    --date)  DATE="$2"; shift 2 ;;
    *)       echo "Usage: today-report.sh [--toml] [--date YYYY-MM-DD]" >&2; exit 1 ;;
  esac
done

if [ ! -f "$DB_PATH" ]; then
  echo "No FluxMirror database found at: $DB_PATH"
  echo "Run an agent session or the MCP proxy first."
  exit 0
fi

# --- Agent summary: one row per agent (calls, sessions) ---
AGENT_SUMMARY=$(sqlite3 "$DB_PATH" -separator '|' \
  "SELECT agent, COUNT(*), COUNT(DISTINCT session)
   FROM agent_events
   WHERE ts LIKE '${DATE}%'
   GROUP BY agent
   ORDER BY agent" 2>/dev/null || true)

# --- Agent tools: one row per agent+tool ---
AGENT_TOOLS=$(sqlite3 "$DB_PATH" -separator '|' \
  "SELECT agent, tool, COUNT(*)
   FROM agent_events
   WHERE ts LIKE '${DATE}%'
   GROUP BY agent, tool
   ORDER BY agent, COUNT(*) DESC" 2>/dev/null || true)

# --- MCP proxy summary ---
MCP_TOTAL=$(sqlite3 "$DB_PATH" \
  "SELECT COUNT(*) FROM events
   WHERE ts_ms >= (strftime('%s', '${DATE}T00:00:00', 'utc') * 1000)
     AND ts_ms <  (strftime('%s', '${DATE}T00:00:00', 'utc') * 1000 + 86400000)" 2>/dev/null || echo "0")

MCP_SESSIONS=$(sqlite3 "$DB_PATH" \
  "SELECT COUNT(DISTINCT server_name) FROM events
   WHERE ts_ms >= (strftime('%s', '${DATE}T00:00:00', 'utc') * 1000)
     AND ts_ms <  (strftime('%s', '${DATE}T00:00:00', 'utc') * 1000 + 86400000)" 2>/dev/null || echo "0")

MCP_METHODS=$(sqlite3 "$DB_PATH" -separator '|' \
  "SELECT method, COUNT(*) FROM events
   WHERE ts_ms >= (strftime('%s', '${DATE}T00:00:00', 'utc') * 1000)
     AND ts_ms <  (strftime('%s', '${DATE}T00:00:00', 'utc') * 1000 + 86400000)
     AND method IS NOT NULL
   GROUP BY method
   ORDER BY COUNT(*) DESC" 2>/dev/null || true)

# --- Helper: build comma-separated tool list for an agent ---
tools_for_agent() {
  local target="$1"
  local result=""
  while IFS='|' read -r agent tool count; do
    [ -z "$agent" ] && continue
    [ "$agent" != "$target" ] && continue
    if [ -n "$result" ]; then result="$result, "; fi
    result="$result$tool ($count)"
  done <<< "$AGENT_TOOLS"
  echo "$result"
}

# --- Output ---
if [ "$FORMAT" = "toml" ]; then
  echo "[report]"
  echo "date = \"$DATE\""
  echo ""

  while IFS='|' read -r agent calls sessions; do
    [ -z "$agent" ] && continue
    echo "[[agents]]"
    echo "name = \"$agent\""
    echo "calls = $calls"
    echo "sessions = $sessions"
    # Build tools inline table
    TOOLS_TOML="{"
    FIRST=true
    while IFS='|' read -r a tool count; do
      [ "$a" != "$agent" ] && continue
      if $FIRST; then FIRST=false; else TOOLS_TOML="$TOOLS_TOML, "; fi
      TOOLS_TOML="$TOOLS_TOML$tool = $count"
    done <<< "$AGENT_TOOLS"
    TOOLS_TOML="$TOOLS_TOML}"
    echo "tools = $TOOLS_TOML"
    echo ""
  done <<< "$AGENT_SUMMARY"

  if [ "$MCP_TOTAL" -gt 0 ]; then
    echo "[[agents]]"
    echo "name = \"claude-desktop\""
    echo "calls = $MCP_TOTAL"
    echo "sessions = $MCP_SESSIONS"
    METHODS_TOML="{"
    FIRST=true
    while IFS='|' read -r method count; do
      [ -z "$method" ] && continue
      if $FIRST; then FIRST=false; else METHODS_TOML="$METHODS_TOML, "; fi
      METHODS_TOML="$METHODS_TOML\"$method\" = $count"
    done <<< "$MCP_METHODS"
    METHODS_TOML="$METHODS_TOML}"
    echo "methods = $METHODS_TOML"
    echo ""
  fi

else
  echo "fluxmirror — today's AI activity ($DATE UTC)"
  echo ""

  HAS_DATA=false

  while IFS='|' read -r agent calls sessions; do
    [ -z "$agent" ] && continue
    HAS_DATA=true
    printf "%-16s %d calls in %s sessions\n" "${agent}:" "$calls" "$sessions"
    echo "  tools: $(tools_for_agent "$agent")"
    echo ""
  done <<< "$AGENT_SUMMARY"

  if [ "$MCP_TOTAL" -gt 0 ]; then
    HAS_DATA=true
    printf "%-16s %d calls in %s sessions  (from MCP proxy)\n" "claude-desktop:" "$MCP_TOTAL" "$MCP_SESSIONS"
    METHODS_LINE=""
    while IFS='|' read -r method count; do
      [ -z "$method" ] && continue
      if [ -n "$METHODS_LINE" ]; then METHODS_LINE="$METHODS_LINE, "; fi
      METHODS_LINE="$METHODS_LINE$method ($count)"
    done <<< "$MCP_METHODS"
    echo "  methods: $METHODS_LINE"
    echo ""
  fi

  if [ "$HAS_DATA" = false ]; then
    echo "  No activity recorded."
  fi
fi
