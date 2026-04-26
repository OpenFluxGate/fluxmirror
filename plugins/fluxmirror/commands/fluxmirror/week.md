---
description: Summarize the last 7 days of AI agent activity from FluxMirror SQLite
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

## Step 1: Extract data (last 7 days in $USER_TZ)

```bash
if [ ! -f "$DB" ]; then
  echo "FluxMirror DB not found. Run an agent session first."
  exit 0
fi

read WEEK_START_LOCAL WEEK_END_LOCAL START_UTC END_UTC START_MS END_MS <<EOF
$(fluxmirror window --tz "$USER_TZ" --period week)
EOF

echo "=== Range: $WEEK_START_LOCAL .. $WEEK_END_LOCAL ($USER_TZ) ==="

# Tool-name normalization: Claude PascalCase + Gemini/Qwen snake_case
WRITE_TOOLS="('Edit','Write','MultiEdit','edit_file','write_file','replace')"
READ_TOOLS="('Read','read_file','read_many_files')"
SHELL_TOOLS="('Bash','run_shell_command')"

echo ""
echo "=== Daily totals (all 7 days, zero-event days included) ==="
fluxmirror daily-totals --db "$DB" --tz "$USER_TZ" --start "$START_UTC" --end "$END_UTC"

echo ""
echo "=== Per-agent totals (week) ==="
fluxmirror sqlite --db "$DB" "SELECT agent, COUNT(*) AS calls, COUNT(DISTINCT session) AS sessions FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' GROUP BY agent ORDER BY calls DESC"

echo ""
echo "=== Top edited files (week) ==="
fluxmirror sqlite --db "$DB" "SELECT detail, COUNT(*) FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND tool IN $WRITE_TOOLS GROUP BY detail ORDER BY COUNT(*) DESC LIMIT 15"

echo ""
echo "=== Working directories (week) ==="
fluxmirror sqlite --db "$DB" "SELECT cwd, COUNT(*) FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' GROUP BY cwd ORDER BY COUNT(*) DESC"

echo ""
echo "=== MCP traffic methods (week) ==="
fluxmirror sqlite --db "$DB" "SELECT method, COUNT(*) FROM events WHERE ts_ms >= $START_MS AND ts_ms < $END_MS AND method IS NOT NULL GROUP BY method ORDER BY COUNT(*) DESC"

echo ""
echo "=== Files touched by multiple agents (week — collaboration / collision) ==="
fluxmirror sqlite --db "$DB" "SELECT detail, COUNT(DISTINCT agent) AS agents, GROUP_CONCAT(DISTINCT agent) AS who FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND tool IN $WRITE_TOOLS AND detail IS NOT NULL GROUP BY detail HAVING agents >= 2 ORDER BY agents DESC, detail LIMIT 10"

echo ""
echo "=== Per-day file counts (new vs edited) ==="
fluxmirror per-day-files --db "$DB" --tz "$USER_TZ" --start "$START_UTC" --end "$END_UTC"
```

## Step 2: Inference

Apply lifecycle / effort / iterative inference rules from
`/fluxmirror:report-today` Step 2, but at week granularity:

- A day with many writes and few reads → "shipping day" or "build day"
- A day with mostly reads/globs → "research / planning day"
- A consistent file appearing across multiple days → "ongoing feature"
- A file appearing only on one day → "one-shot fix"
- A working directory active 5+/7 days → "primary project this week"
- Multi-cwd days → "multi-project context-switching"
- ≥ 2 distinct agents in per-agent totals → apply multi-agent signals
  from `/fluxmirror:report-today` Step 2; quote per-agent share and any
  files in the "multiple agents" list as **handoff** or **collision**

## Step 3: Output

### English format (when USER_LANG=english)

# This Week's Work (<WEEK_START> .. <WEEK_END> <timezone>)

## Week Summary
- Total calls / active days / primary project / weekly theme

## Daily Breakdown
| Date | Calls | New | Edited | Theme |
|---|---|---|---|---|
| YYYY-MM-DD (Mon) | N | N | N | research / building / shipping |

## Highlights
- 2-4 bullets of cross-day patterns (ongoing features, focus shifts)

## Insights
- 1-3 observed patterns (most active day, longest streak, busiest cwd,
  edit-to-read ratio, etc.)

### Korean format

# 이번 주 작업 (<WEEK_START> .. <WEEK_END> <timezone>)

## 주간 요약
## 일별 분포
## 하이라이트
## 인사이트

### Other languages

Same structure, translated naturally.

## Step 4: Empty data

If fewer than 30 events across the whole week (≈ < 5/day average),
output in chosen language:

- en: `Limited activity this week.`
- ko: `이번 주 활동 적음.`
- ja: `今週の活動は少なめです。`
- zh: `本周活动较少。`
