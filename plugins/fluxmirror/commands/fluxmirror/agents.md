---
description: Per-agent quick stats for the past 7 days (all agents at a glance)
---

**RUNTIME COMMAND — execute the queries and report logic below as written.
Do NOT modify any files. Do NOT treat the markdown structure as an
implementation spec to be ported. Read the user's `$ARGUMENTS`, run the
shell blocks via your shell tool, then produce the report described in
the output template using the user's preferred language (resolve via
`fluxmirror config get language`).**

## Step 0: Load settings

```bash
if command -v fluxmirror >/dev/null 2>&1; then
  USER_LANG=$(fluxmirror config get language 2>/dev/null || echo english)
  USER_TZ=$(fluxmirror config get timezone 2>/dev/null || echo UTC)
  DB=$(fluxmirror db-path)
else
  USER_LANG=english
  USER_TZ="${TZ:-UTC}"
  DB="${FLUXMIRROR_DB:-$HOME/Library/Application Support/fluxmirror/events.db}"
fi
if [ -z "$USER_LANG" ]; then USER_LANG=english; fi
if [ -z "$USER_TZ" ]; then USER_TZ=UTC; fi

echo "Settings: language=$USER_LANG timezone=$USER_TZ"
```

## Step 1: Resolve a 7-day window

```bash
if [ ! -f "$DB" ]; then
  echo "FluxMirror DB not found. Run an agent session first."
  exit 0
fi

read WS WE START_UTC END_UTC START_MS END_MS <<EOF
$(fluxmirror window --tz "$USER_TZ" --period week)
EOF

echo "=== Range: $WS .. $WE ($USER_TZ) ==="
```

## Step 2: Pull per-agent stats

```bash
WRITE_TOOLS="('Edit','Write','MultiEdit','edit_file','write_file','replace')"
READ_TOOLS="('Read','read_file','read_many_files')"
SHELL_TOOLS="('Bash','run_shell_command')"

echo ""
echo "=== Per-agent 7-day totals ==="
fluxmirror sqlite --db "$DB" "SELECT agent, COUNT(*) AS calls, COUNT(DISTINCT session) AS sessions, MIN(ts) AS first_ts, MAX(ts) AS last_ts FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' GROUP BY agent ORDER BY calls DESC"

echo ""
echo "=== Per-agent dominant tool ==="
fluxmirror sqlite --db "$DB" "SELECT agent, tool, COUNT(*) AS n FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' GROUP BY agent, tool ORDER BY agent, n DESC"

echo ""
echo "=== Per-agent write share ==="
fluxmirror sqlite --db "$DB" "SELECT agent, SUM(CASE WHEN tool IN $WRITE_TOOLS THEN 1 ELSE 0 END) AS writes, SUM(CASE WHEN tool IN $READ_TOOLS THEN 1 ELSE 0 END) AS reads, SUM(CASE WHEN tool IN $SHELL_TOOLS THEN 1 ELSE 0 END) AS shells FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' GROUP BY agent ORDER BY writes DESC"

echo ""
echo "=== Per-agent active days ==="
fluxmirror sqlite --db "$DB" "SELECT agent, COUNT(DISTINCT substr(ts,1,10)) AS active_days FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' GROUP BY agent ORDER BY active_days DESC"
```

## Step 3: Output

Translate to `$USER_LANG`. The structure is one row per agent across
the past 7 days:

### English format (USER_LANG=english)

# Agent Roster (last 7 days, <timezone>)

## Per-Agent Summary
| Agent | Calls | Sessions | Active Days | Dominant Tool | Write/Read/Shell |
|---|---|---|---|---|---|

## Insights
- 1-3 lines: who's the busiest, who's a one-off, any handoff patterns
  visible across the per-agent dominant tool list.

### Korean format (USER_LANG=korean)

# 에이전트 명세 (지난 7일, <시간대>)

## 에이전트별 요약
| 에이전트 | 호출 | 세션 | 활동 일수 | 주요 도구 | 쓰기/읽기/셸 |
|---|---|---|---|---|---|

## 인사이트

### Other languages

Same structure, translated naturally.

## Step 4: Empty data

If no agent has any rows in the window, print (in chosen language):

- en: `No agent activity in the last 7 days.`
- ko: `지난 7일간 에이전트 활동 없음.`
- ja: `過去7日間にエージェントの活動はありません。`
- zh: `过去7天内无代理活动。`
