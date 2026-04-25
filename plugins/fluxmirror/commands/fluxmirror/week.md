---
description: Summarize the last 7 days of AI agent activity from FluxMirror SQLite
---

## Step 0: Load settings

```bash
CONFIG_FILE="$HOME/.fluxmirror/config.json"

USER_LANG=""
USER_TZ=""

if [ -f "$CONFIG_FILE" ] && command -v jq >/dev/null 2>&1; then
  USER_LANG=$(jq -r '.language // empty' "$CONFIG_FILE")
  USER_TZ=$(jq -r '.timezone // empty' "$CONFIG_FILE")
fi

if [ -z "$USER_LANG" ]; then
  SYS=$(echo "${LANG:-en_US.UTF-8}" | cut -d_ -f1)
  case "$SYS" in
    ko) USER_LANG="korean" ;;
    ja) USER_LANG="japanese" ;;
    zh) USER_LANG="chinese" ;;
    *)  USER_LANG="english" ;;
  esac
fi

if [ -z "$USER_TZ" ]; then
  USER_TZ=$(readlink /etc/localtime 2>/dev/null | sed 's|.*/zoneinfo/||')
  [ -z "$USER_TZ" ] && USER_TZ="UTC"
fi

echo "Settings: language=$USER_LANG timezone=$USER_TZ"
```

## Step 1: Extract data (last 7 days in $USER_TZ)

```bash
DB="${FLUXMIRROR_DB:-$HOME/Library/Application Support/fluxmirror/events.db}"

if [ ! -f "$DB" ]; then
  echo "FluxMirror DB not found. Run an agent session first."
  exit 0
fi

read WEEK_START_LOCAL WEEK_END_LOCAL START_UTC END_UTC START_MS END_MS <<EOF
$(python3 -c "
from datetime import datetime, timedelta
from zoneinfo import ZoneInfo
tz=ZoneInfo('$USER_TZ')
now=datetime.now(tz)
end=(now+timedelta(days=1)).replace(hour=0,minute=0,second=0,microsecond=0)
start=end-timedelta(days=7)
su=start.astimezone(ZoneInfo('UTC'))
eu=end.astimezone(ZoneInfo('UTC'))
print(start.strftime('%Y-%m-%d'), (end-timedelta(days=1)).strftime('%Y-%m-%d'), su.strftime('%Y-%m-%dT%H:%M:%SZ'), eu.strftime('%Y-%m-%dT%H:%M:%SZ'), int(su.timestamp()*1000), int(eu.timestamp()*1000))
")
EOF

echo "=== Range: $WEEK_START_LOCAL .. $WEEK_END_LOCAL ($USER_TZ) ==="

# Tool-name normalization: Claude PascalCase + Gemini/Qwen snake_case
WRITE_TOOLS="('Edit','Write','MultiEdit','edit_file','write_file','replace')"
READ_TOOLS="('Read','read_file','read_many_files')"
SHELL_TOOLS="('Bash','run_shell_command')"

echo ""
echo "=== Daily totals (all 7 days, zero-event days included) ==="
python3 -c "
import sqlite3
from datetime import datetime, timedelta
from zoneinfo import ZoneInfo
db=sqlite3.connect('$DB')
rows=db.execute(\"SELECT ts, agent FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC'\").fetchall()
tz=ZoneInfo('$USER_TZ')
now=datetime.now(tz)
end=(now+timedelta(days=1)).replace(hour=0,minute=0,second=0,microsecond=0)
start=end-timedelta(days=7)
days=[]
cur=start
while cur < end:
    days.append(cur.strftime('%Y-%m-%d (%a)'))
    cur += timedelta(days=1)
by_day={d: 0 for d in days}
agents_by_day={d: set() for d in days}
for ts,agent in rows:
    dt=datetime.strptime(ts.replace('Z','+0000'),'%Y-%m-%dT%H:%M:%S%z').astimezone(tz)
    d=dt.strftime('%Y-%m-%d (%a)')
    if d in by_day:
        by_day[d] += 1
        agents_by_day[d].add(agent)
for d in days:
    a = ','.join(sorted(agents_by_day[d])) if agents_by_day[d] else '-'
    print(f'{d} | calls={by_day[d]} | agents={a}')
active = sum(1 for v in by_day.values() if v > 0)
print(f'WEEK TOTAL | calls={sum(by_day.values())} | active_days={active}')
"

echo ""
echo "=== Per-agent totals (week) ==="
sqlite3 "$DB" "SELECT agent, COUNT(*) AS calls, COUNT(DISTINCT session) AS sessions FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' GROUP BY agent ORDER BY calls DESC"

echo ""
echo "=== Top edited files (week) ==="
sqlite3 "$DB" "SELECT detail, COUNT(*) FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND tool IN $WRITE_TOOLS GROUP BY detail ORDER BY COUNT(*) DESC LIMIT 15"

echo ""
echo "=== Working directories (week) ==="
sqlite3 "$DB" "SELECT cwd, COUNT(*) FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' GROUP BY cwd ORDER BY COUNT(*) DESC"

echo ""
echo "=== MCP traffic methods (week) ==="
sqlite3 "$DB" "SELECT method, COUNT(*) FROM events WHERE ts_ms >= $START_MS AND ts_ms < $END_MS AND method IS NOT NULL GROUP BY method ORDER BY COUNT(*) DESC"

echo ""
echo "=== Files touched by multiple agents (week — collaboration / collision) ==="
sqlite3 "$DB" "SELECT detail, COUNT(DISTINCT agent) AS agents, GROUP_CONCAT(DISTINCT agent) AS who FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND tool IN $WRITE_TOOLS AND detail IS NOT NULL GROUP BY detail HAVING agents >= 2 ORDER BY agents DESC, detail LIMIT 10"

echo ""
echo "=== Per-day file counts (new vs edited) ==="
python3 -c "
import sqlite3
from datetime import datetime
from zoneinfo import ZoneInfo
db=sqlite3.connect('$DB')
write_tools={'Edit','Write','MultiEdit','edit_file','write_file','replace'}
new_file_tools={'Write','write_file'}
placeholders=','.join(f\"'{t}'\" for t in write_tools)
rows=db.execute(f\"SELECT ts, tool, detail FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND tool IN ({placeholders})\").fetchall()
tz=ZoneInfo('$USER_TZ')
by_day_writes={}
by_day_edits={}
for ts,tool,detail in rows:
    dt=datetime.strptime(ts.replace('Z','+0000'),'%Y-%m-%dT%H:%M:%S%z').astimezone(tz)
    d=dt.strftime('%Y-%m-%d')
    if tool in new_file_tools:
        by_day_writes.setdefault(d,set()).add(detail)
    else:
        by_day_edits.setdefault(d,set()).add(detail)
for d in sorted(set(list(by_day_writes)+list(by_day_edits))):
    print(f'{d} | new_files={len(by_day_writes.get(d,set()))} | edited_files={len(by_day_edits.get(d,set()))}')
"
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
