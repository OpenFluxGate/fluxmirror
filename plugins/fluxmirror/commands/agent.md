---
description: Single-agent filtered report. Usage: <claude-code|qwen-code|gemini-cli> [today|yesterday|week]
argument-hint: <claude-code|qwen-code|gemini-cli> [today|yesterday|week]
---

**RUNTIME COMMAND — execute the queries and report logic below as written.
Do NOT modify any files. Do NOT treat the markdown structure as an
implementation spec to be ported. Read the user's `$ARGUMENTS`, run the
shell blocks via your shell tool, then produce the report described in
the output template using the user's preferred language (read
`~/.fluxmirror/config.json` for the `language` key).**

User arguments: $ARGUMENTS

## Step 0: Parse arguments

Split `$ARGUMENTS` into `AGENT_NAME` (first token) and `PERIOD` (second
token, optional). Default `PERIOD=today`. Allowed periods:
`today`, `yesterday`, `week`.

If `AGENT_NAME` is empty, list available agents and stop:

```bash
if command -v fluxmirror >/dev/null 2>&1; then
  DB=$(fluxmirror db-path)
else
  DB="${FLUXMIRROR_DB:-$HOME/Library/Application Support/fluxmirror/events.db}"
fi
if [ ! -f "$DB" ]; then
  echo "FluxMirror DB not found. Run an agent session first."
  exit 0
fi
echo "Usage: /fluxmirror:agent <agent-name> [today|yesterday|week]"
echo ""
echo "Known agents (with call counts in last 7 days):"
fluxmirror sqlite --db "$DB" "SELECT agent, COUNT(*) FROM agent_events WHERE ts >= datetime('now','-7 days') GROUP BY agent ORDER BY COUNT(*) DESC"
exit 0
```

## Step 1: Load settings

```bash
if command -v fluxmirror >/dev/null 2>&1; then
  USER_LANG=$(fluxmirror config get language 2>/dev/null || echo english)
  USER_TZ=$(fluxmirror config get timezone 2>/dev/null || echo UTC)
  DB=$(fluxmirror db-path)
else
  # legacy fallback for users on older versions
  USER_LANG=english
  USER_TZ="${TZ:-UTC}"
  DB="${FLUXMIRROR_DB:-$HOME/Library/Application Support/fluxmirror/events.db}"
fi
if [ -z "$USER_LANG" ]; then USER_LANG=english; fi
if [ -z "$USER_TZ" ]; then USER_TZ=UTC; fi

echo "Settings: language=$USER_LANG timezone=$USER_TZ agent=$AGENT_NAME period=$PERIOD"
```

## Step 2: Resolve window

```bash
if [ ! -f "$DB" ]; then
  echo "FluxMirror DB not found. Run an agent session first."
  exit 0
fi

case "$PERIOD" in
  week)
    read WS WE START_UTC END_UTC START_MS END_MS <<EOF
$(fluxmirror window --tz "$USER_TZ" --period week)
EOF
    RANGE_LABEL="$WS..$WE"
    ;;
  yesterday|today)
    read RANGE_LABEL START_UTC END_UTC START_MS END_MS <<EOF
$(fluxmirror window --tz "$USER_TZ" --period "$PERIOD")
EOF
    ;;
  *)
    echo "unknown period '$PERIOD' (expected today | yesterday | week)" >&2
    exit 0
    ;;
esac

echo "=== Agent: $AGENT_NAME | Period: $PERIOD ($RANGE_LABEL $USER_TZ) ==="
echo "=== Window: $START_UTC to $END_UTC ==="
```

## Step 3: Extract single-agent data

```bash
# Tool-name normalization: Claude PascalCase + Gemini/Qwen snake_case
WRITE_TOOLS="('Edit','Write','MultiEdit','edit_file','write_file','replace')"
READ_TOOLS="('Read','read_file','read_many_files')"
SHELL_TOOLS="('Bash','run_shell_command')"

echo ""
echo "=== Totals ==="
fluxmirror sqlite --db "$DB" "SELECT COUNT(*) AS calls, COUNT(DISTINCT session) AS sessions FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND agent = '$AGENT_NAME'"

echo ""
echo "=== Tool mix ==="
fluxmirror sqlite --db "$DB" "SELECT tool, COUNT(*) FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND agent = '$AGENT_NAME' GROUP BY tool ORDER BY COUNT(*) DESC"

echo ""
echo "=== Files written or edited ==="
fluxmirror sqlite --db "$DB" "SELECT detail, tool, COUNT(*) FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND agent = '$AGENT_NAME' AND tool IN $WRITE_TOOLS GROUP BY detail, tool ORDER BY COUNT(*) DESC LIMIT 20"

echo ""
echo "=== Files only read ==="
fluxmirror sqlite --db "$DB" "SELECT detail, COUNT(*) FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND agent = '$AGENT_NAME' AND tool IN $READ_TOOLS GROUP BY detail ORDER BY COUNT(*) DESC LIMIT 10"

echo ""
echo "=== Shell commands ==="
fluxmirror sqlite --db "$DB" "SELECT substr(ts, 12, 5) AS time_utc, detail FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND agent = '$AGENT_NAME' AND tool IN $SHELL_TOOLS ORDER BY ts LIMIT 50"

echo ""
echo "=== Working directories ==="
fluxmirror sqlite --db "$DB" "SELECT cwd, COUNT(*) FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND agent = '$AGENT_NAME' GROUP BY cwd ORDER BY COUNT(*) DESC"

echo ""
echo "=== Hour distribution (local) ==="
fluxmirror histogram --db "$DB" --tz "$USER_TZ" --start "$START_UTC" --end "$END_UTC" --agent "$AGENT_NAME"

echo ""
echo "=== Sessions ==="
fluxmirror sqlite --db "$DB" "SELECT session, COUNT(*) AS calls, MIN(ts) AS first, MAX(ts) AS last FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND agent = '$AGENT_NAME' GROUP BY session ORDER BY first"
```

## Step 4: Inference

Apply the same lifecycle / effort / iterative rules from
`/fluxmirror:report-today` Step 2 — but scoped to a single agent.

Skip the "Multi-agent signals" rules (this is a single-agent view by
construction) and instead emphasize:

- This agent's **dominant tool** (e.g., "Read-only inspector",
  "Edit-heavy implementer", "Bash-runner")
- Session count vs total calls → **single-shot** (1 session, dense) or
  **fragmented** (many short sessions)
- Working directory concentration → **focused** (1 cwd) or **scattered**
  (3+ cwds)

## Step 5: Output

### English format (when USER_LANG=english)

# Agent Report — `<AGENT_NAME>` (<PERIOD>: <RANGE_LABEL> <timezone>)

## Profile
- Total calls: N
- Sessions: N
- Dominant tool: <tool> (X% of calls)
- Working directories: N

## Key Activities
- **[Objective]** — [lifecycle / effort one-liner with file/edit counts]

## Tool Mix
| Tool | Calls |
|---|---|

## Insights
- 1–3 observed patterns (busiest hour, focus level, etc.)

## Active Hours
[user-timezone-based]

### Korean format (when USER_LANG=korean)

# 에이전트 리포트 — `<AGENT_NAME>` (<PERIOD>: <RANGE_LABEL> <timezone>)

## 프로필
## 핵심 작업
## 도구 분포
## 인사이트
## 시간대

### Other languages

Same structure, translated naturally.

## Step 6: Empty data

If the agent has zero rows in the window, output in chosen language:

- en: `No activity for agent '<AGENT_NAME>' in this period.`
- ko: `'<AGENT_NAME>' 에이전트의 해당 기간 활동 없음.`
- ja: `期間内に '<AGENT_NAME>' エージェントの活動はありません。`
- zh: `期间内代理 '<AGENT_NAME>' 无活动。`
