#!/bin/bash
# PostToolUse hook for Claude Code — appends one JSON line per tool call
# to ~/.claude/session-logs/YYYY-MM-DD.jsonl, and writes to FluxMirror SQLite.

INPUT=$(cat)

TS=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
TOOL=$(echo "$INPUT" | jq -r '.tool_name // empty')

if [ -z "$TOOL" ]; then
  exit 0
fi

DETAIL=$(echo "$INPUT" | jq -r '.tool_input | to_entries[0].value // empty' | head -c 200)
SESSION=$(echo "$INPUT" | jq -r '.session_id // "unknown"')
CWD=$(echo "$INPUT" | jq -r '.cwd // "unknown"')

# --- JSONL write (original behavior) ---
LOG_DIR="$HOME/.claude/session-logs"
mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/$(date -u +%Y-%m-%d).jsonl"

jq -cn \
  --arg ts "$TS" \
  --arg session "$SESSION" \
  --arg tool "$TOOL" \
  --arg detail "$DETAIL" \
  --arg cwd "$CWD" \
  '{ts: $ts, session: $session, tool: $tool, detail: $detail, cwd: $cwd}' \
  >> "$LOG_FILE"

# --- SQLite dual-write to FluxMirror ---
AGENT="claude-code"
DB_PATH="${FLUXMIRROR_DB:-$HOME/Library/Application Support/fluxmirror/events.db}"
mkdir -p "$(dirname "$DB_PATH")"

sqlite3 "$DB_PATH" \
  -cmd ".parameter set :ts '$TS'" \
  -cmd ".parameter set :agent '$AGENT'" \
  -cmd ".parameter set :session '$SESSION'" \
  -cmd ".parameter set :tool '$TOOL'" \
  -cmd ".parameter set :detail '$(echo "$DETAIL" | sed "s/'/''/g")'" \
  -cmd ".parameter set :cwd '$CWD'" \
  -cmd ".parameter set :raw '$(echo "$INPUT" | sed "s/'/''/g")'" \
  "CREATE TABLE IF NOT EXISTS agent_events (
     id INTEGER PRIMARY KEY AUTOINCREMENT,
     ts TEXT NOT NULL,
     agent TEXT NOT NULL,
     session TEXT,
     tool TEXT,
     detail TEXT,
     cwd TEXT,
     raw_json TEXT
   );
   INSERT INTO agent_events (ts, agent, session, tool, detail, cwd, raw_json)
   VALUES (:ts, :agent, :session, :tool, :detail, :cwd, :raw)" \
  2>/dev/null || true

exit 0
