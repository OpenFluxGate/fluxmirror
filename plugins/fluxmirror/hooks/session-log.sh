#!/bin/bash
# PostToolUse hook for Claude Code (and Qwen Code, which reuses this plugin).
# - Appends one JSON line per tool call to ~/<agent>/session-logs/YYYY-MM-DD.jsonl
# - Writes a parameter-bound row into the FluxMirror SQLite agent_events table
#   via the shared helper _dual_write.py
#
# Required: jq, python3
#
# Optional env:
#   FLUXMIRROR_DB         override DB path (default: ~/Library/Application Support/fluxmirror/events.db)
#   FLUXMIRROR_SKIP_SELF  if "1", combined with FLUXMIRROR_SELF_REPO, skips the
#                         hook when fluxmirror is querying its own DB from inside
#                         its own repo (avoids self-noise in reports).
#   FLUXMIRROR_SELF_REPO  absolute path to the fluxmirror repo for the filter above.

INPUT=$(cat)
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

TS=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
TOOL=$(echo "$INPUT" | jq -r '.tool_name // empty')

if [ -z "$TOOL" ]; then
  exit 0
fi

# Tool-aware detail extraction. Generic to_entries[0] picked the wrong field
# for tools whose first key is a description (e.g., Gemini run_shell_command
# may put a natural-language summary before the actual command). Map each
# known Claude/Gemini/Qwen tool to its primary field; fall back to first
# scalar value for unknown tools.
case "$TOOL" in
  # --- shell ---
  Bash)
    DETAIL=$(echo "$INPUT" | jq -r '.tool_input.command // empty') ;;
  run_shell_command)
    DETAIL=$(echo "$INPUT" | jq -r '.tool_input.command // .tool_input.cmd // empty') ;;
  BashOutput|KillBash|kill_shell)
    DETAIL=$(echo "$INPUT" | jq -r '.tool_input.bash_id // .tool_input.shell_id // empty') ;;

  # --- file IO ---
  Read|Write|Edit|MultiEdit|NotebookEdit)
    DETAIL=$(echo "$INPUT" | jq -r '.tool_input.file_path // .tool_input.notebook_path // empty') ;;
  read_file|write_file|edit_file|replace|read_many_files)
    DETAIL=$(echo "$INPUT" | jq -r '.tool_input.absolute_path // .tool_input.path // .tool_input.file_path // empty') ;;

  # --- search / glob ---
  Grep|search_file_content)
    DETAIL=$(echo "$INPUT" | jq -r '.tool_input.pattern // .tool_input.query // empty') ;;
  Glob|glob)
    DETAIL=$(echo "$INPUT" | jq -r '.tool_input.pattern // empty') ;;

  # --- web ---
  WebFetch|web_fetch)
    DETAIL=$(echo "$INPUT" | jq -r '.tool_input.url // empty') ;;
  WebSearch|web_search|google_web_search)
    DETAIL=$(echo "$INPUT" | jq -r '.tool_input.query // empty') ;;

  # --- task / planning / memory ---
  Task)
    DETAIL=$(echo "$INPUT" | jq -r '.tool_input.description // .tool_input.prompt // empty') ;;
  TodoWrite|todo_write)
    DETAIL=$(echo "$INPUT" | jq -r 'if .tool_input.todos then "[" + (.tool_input.todos | length | tostring) + " todos]" else empty end') ;;
  ExitPlanMode)
    DETAIL=$(echo "$INPUT" | jq -r '.tool_input.plan // empty') ;;
  save_memory)
    DETAIL=$(echo "$INPUT" | jq -r '.tool_input.fact // .tool_input.content // empty') ;;

  # --- fallback ---
  *)
    DETAIL=$(echo "$INPUT" | jq -r '.tool_input | to_entries[]? | select(.value | type=="string") | .value' | head -1) ;;
esac
DETAIL=$(printf '%s' "$DETAIL" | head -c 200)

SESSION=$(echo "$INPUT" | jq -r '.session_id // "unknown"')
CWD=$(echo "$INPUT" | jq -r '.cwd // "unknown"')

# --- Agent detection ---
# Qwen Code installs this same plugin under ~/.qwen/extensions/. Qwen exposes
# QWEN_CODE_NO_RELAUNCH and QWEN_PROJECT_DIR in the hook environment (verified
# via runtime probe). Note: CLAUDECODE / CLAUDE_CODE_* may also be present even
# under Qwen if Qwen was launched from a Claude Code shell, so we use POSITIVE
# Qwen evidence rather than absence of Claude markers.
if [ -n "$QWEN_CODE_NO_RELAUNCH" ] || [ -n "$QWEN_PROJECT_DIR" ]; then
  AGENT="qwen-code"
  LOG_BASE="$HOME/.qwen"
else
  AGENT="claude-code"
  LOG_BASE="$HOME/.claude"
fi

# --- Opt-in self-noise filter ---
# Only skips when BOTH FLUXMIRROR_SKIP_SELF=1 and FLUXMIRROR_SELF_REPO=/path.
# Path comparison is anchored (canonical prefix), so "/x/fluxmirror-notes" does
# not falsely match "/x/fluxmirror".
TOOL_IS_SHELL=0
case "$TOOL" in
  Bash|run_shell_command) TOOL_IS_SHELL=1 ;;
esac

if [ "${FLUXMIRROR_SKIP_SELF:-0}" = "1" ] && [ -n "$FLUXMIRROR_SELF_REPO" ] && [ "$TOOL_IS_SHELL" = "1" ]; then
  CWD_REAL=$(cd "$CWD" 2>/dev/null && pwd -P)
  REPO_REAL=$(cd "$FLUXMIRROR_SELF_REPO" 2>/dev/null && pwd -P)
  if [ -n "$CWD_REAL" ] && [ -n "$REPO_REAL" ]; then
    case "$CWD_REAL/" in
      "$REPO_REAL/"*)
        if echo "$DETAIL" | grep -qE 'sqlite3.*events\.db|fluxmirror.*\.db'; then
          exit 0
        fi
        ;;
    esac
  fi
fi

# --- JSONL write ---
LOG_DIR="$LOG_BASE/session-logs"
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

# --- SQLite dual-write via shared helper (parameter-bound) ---
DB_PATH="${FLUXMIRROR_DB:-$HOME/Library/Application Support/fluxmirror/events.db}"

printf '%s' "$INPUT" | python3 "$SCRIPT_DIR/_dual_write.py" \
  "$DB_PATH" "$TS" "$AGENT" "$SESSION" "$TOOL" "$DETAIL" "$CWD"

exit 0
