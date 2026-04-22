#!/bin/bash
# AfterTool hook for Gemini CLI — appends one JSON line per tool call
# to ~/.gemini/session-logs/YYYY-MM-DD.jsonl.

INPUT=$(cat)

TS=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
TOOL=$(echo "$INPUT" | jq -r '.tool_name // empty')

if [ -z "$TOOL" ]; then
  exit 0
fi

DETAIL=$(echo "$INPUT" | jq -r '.tool_input | to_entries[0].value // empty' | head -c 200)
SESSION=$(echo "$INPUT" | jq -r '.session_id // "unknown"')
CWD=$(echo "$INPUT" | jq -r '.cwd // "unknown"')

LOG_DIR="$HOME/.gemini/session-logs"
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

exit 0
