---
description: Summarize yesterday's AI agent activity from FluxMirror SQLite
---

**RUNTIME COMMAND — execute the queries and report logic below as written.
Do NOT modify any files. Do NOT treat the markdown structure as an
implementation spec to be ported. Read the user's `$ARGUMENTS`, run the
shell blocks via your shell tool, then produce the report described in
the output template using the user's preferred language (read
`~/.fluxmirror/config.json` for the `language` key).**

## Step 0: Load settings

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

echo "Settings: language=$USER_LANG timezone=$USER_TZ"
```

## Step 1: Extract data (yesterday in $USER_TZ)

```bash
if [ ! -f "$DB" ]; then
  echo "FluxMirror DB not found. Run an agent session first."
  exit 0
fi

read TARGET_LOCAL START_UTC END_UTC START_MS END_MS <<EOF
$(fluxmirror window --tz "$USER_TZ" --period yesterday)
EOF

echo "=== Range: $START_UTC to $END_UTC ($USER_TZ; local date: $TARGET_LOCAL) ==="

# Tool-name normalization: Claude PascalCase + Gemini/Qwen snake_case
WRITE_TOOLS="('Edit','Write','MultiEdit','edit_file','write_file','replace')"
READ_TOOLS="('Read','read_file','read_many_files')"
SHELL_TOOLS="('Bash','run_shell_command')"

echo ""
echo "=== Per-agent calls ==="
fluxmirror sqlite --db "$DB" "SELECT agent, COUNT(*) AS calls, COUNT(DISTINCT session) AS sessions FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' GROUP BY agent ORDER BY calls DESC"

echo ""
echo "=== Files touched by multiple agents (collaboration / collision) ==="
fluxmirror sqlite --db "$DB" "SELECT detail, COUNT(DISTINCT agent) AS agents, GROUP_CONCAT(DISTINCT agent) AS who FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND tool IN $WRITE_TOOLS AND detail IS NOT NULL GROUP BY detail HAVING agents >= 2 ORDER BY agents DESC, detail LIMIT 10"

echo ""
echo "=== Files written or edited ==="
fluxmirror sqlite --db "$DB" "SELECT detail, tool, COUNT(*) FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND tool IN $WRITE_TOOLS GROUP BY detail, tool ORDER BY COUNT(*) DESC LIMIT 20"

echo ""
echo "=== Files only read ==="
fluxmirror sqlite --db "$DB" "SELECT detail, COUNT(*) FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND tool IN $READ_TOOLS GROUP BY detail ORDER BY COUNT(*) DESC LIMIT 10"

echo ""
echo "=== Shell commands ==="
fluxmirror sqlite --db "$DB" "SELECT substr(ts, 12, 5) AS time_utc, detail FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND tool IN $SHELL_TOOLS ORDER BY ts"

echo ""
echo "=== Working directories ==="
fluxmirror sqlite --db "$DB" "SELECT cwd, COUNT(*) FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' GROUP BY cwd ORDER BY COUNT(*) DESC"

echo ""
echo "=== MCP traffic methods ==="
fluxmirror sqlite --db "$DB" "SELECT method, COUNT(*) FROM events WHERE ts_ms >= $START_MS AND ts_ms < $END_MS AND method IS NOT NULL GROUP BY method ORDER BY COUNT(*) DESC"

echo ""
echo "=== Hour distribution (local) ==="
fluxmirror histogram --db "$DB" --tz "$USER_TZ" --start "$START_UTC" --end "$END_UTC"
```

## Step 2: Inference

Apply the same inference rules as `/fluxmirror:report-today` Step 2
(lifecycle stage, effort estimation, iterative patterns, multi-agent
signals). The label should reference yesterday's date in the user's
timezone.

## Step 3: Output

Use the same structure as `/fluxmirror:report-today` Step 3 but with
yesterday's date in the title.

### English format (when USER_LANG=english)

# Yesterday's Work (YYYY-MM-DD <timezone>)

## Key Activities
- ...

## Activity Stats
| Agent | Calls | Sessions |
|---|---|---|
| (one row per agent — single row if solo, multiple if ≥2; add a "## Multi-Agent" section after this when ≥2) |

## Insights
- 1-3 lines of observed patterns (most active hour, edit-to-read
  ratio, multi-project switches, new-file count, etc.)

## Active Hours
[user-timezone-based]

### Korean format (when USER_LANG=korean)

# 어제의 작업 (YYYY-MM-DD <timezone>)

## 핵심 작업
## 활동 통계
## 인사이트
## 시간대

### Other languages

Same structure, translated naturally.

## Step 4: Empty data

If fewer than 5 events, output in chosen language:

- en: `No activity yesterday.`
- ko: `어제 활동 없음.`
- ja: `昨日の活動はありませんでした。`
- zh: `昨日无活动。`
