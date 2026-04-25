---
description: Single-agent filtered report from FluxMirror SQLite
argument-hint: <agent-name> [today|yesterday|week]
---

User arguments: $ARGUMENTS

## Step 0: Parse arguments

Split `$ARGUMENTS` into `AGENT_NAME` (first token) and `PERIOD` (second
token, optional). Default `PERIOD=today`. Allowed periods:
`today`, `yesterday`, `week`.

If `AGENT_NAME` is empty, list available agents and stop:

```bash
DB="${FLUXMIRROR_DB:-$HOME/Library/Application Support/fluxmirror/events.db}"
if [ ! -f "$DB" ]; then
  echo "FluxMirror DB not found. Run an agent session first."
  exit 0
fi
echo "Usage: /fluxmirror:agent <agent-name> [today|yesterday|week]"
echo ""
echo "Known agents (with call counts in last 7 days):"
sqlite3 "$DB" "SELECT agent, COUNT(*) FROM agent_events WHERE ts >= datetime('now','-7 days') GROUP BY agent ORDER BY COUNT(*) DESC"
exit 0
```

## Step 1: Load settings

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

echo "Settings: language=$USER_LANG timezone=$USER_TZ agent=$AGENT_NAME period=$PERIOD"
```

## Step 2: Resolve window

```bash
DB="${FLUXMIRROR_DB:-$HOME/Library/Application Support/fluxmirror/events.db}"

if [ ! -f "$DB" ]; then
  echo "FluxMirror DB not found. Run an agent session first."
  exit 0
fi

read RANGE_LABEL START_UTC END_UTC START_MS END_MS <<EOF
$(python3 -c "
from datetime import datetime, timedelta
from zoneinfo import ZoneInfo
tz=ZoneInfo('$USER_TZ')
now=datetime.now(tz)
period='$PERIOD'
if period == 'yesterday':
    end=now.replace(hour=0,minute=0,second=0,microsecond=0)
    start=end-timedelta(days=1)
    label=start.strftime('%Y-%m-%d')
elif period == 'week':
    end=(now+timedelta(days=1)).replace(hour=0,minute=0,second=0,microsecond=0)
    start=end-timedelta(days=7)
    label=start.strftime('%Y-%m-%d')+'..'+(end-timedelta(days=1)).strftime('%Y-%m-%d')
else:
    start=now.replace(hour=0,minute=0,second=0,microsecond=0)
    end=start+timedelta(days=1)
    label=start.strftime('%Y-%m-%d')
su=start.astimezone(ZoneInfo('UTC'))
eu=end.astimezone(ZoneInfo('UTC'))
print(label, su.strftime('%Y-%m-%dT%H:%M:%SZ'), eu.strftime('%Y-%m-%dT%H:%M:%SZ'), int(su.timestamp()*1000), int(eu.timestamp()*1000))
")
EOF

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
sqlite3 "$DB" "SELECT COUNT(*) AS calls, COUNT(DISTINCT session) AS sessions FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND agent = '$AGENT_NAME'"

echo ""
echo "=== Tool mix ==="
sqlite3 "$DB" "SELECT tool, COUNT(*) FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND agent = '$AGENT_NAME' GROUP BY tool ORDER BY COUNT(*) DESC"

echo ""
echo "=== Files written or edited ==="
sqlite3 "$DB" "SELECT detail, tool, COUNT(*) FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND agent = '$AGENT_NAME' AND tool IN $WRITE_TOOLS GROUP BY detail, tool ORDER BY COUNT(*) DESC LIMIT 20"

echo ""
echo "=== Files only read ==="
sqlite3 "$DB" "SELECT detail, COUNT(*) FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND agent = '$AGENT_NAME' AND tool IN $READ_TOOLS GROUP BY detail ORDER BY COUNT(*) DESC LIMIT 10"

echo ""
echo "=== Shell commands ==="
sqlite3 "$DB" "SELECT substr(ts, 12, 5) AS time_utc, detail FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND agent = '$AGENT_NAME' AND tool IN $SHELL_TOOLS ORDER BY ts LIMIT 50"

echo ""
echo "=== Working directories ==="
sqlite3 "$DB" "SELECT cwd, COUNT(*) FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND agent = '$AGENT_NAME' GROUP BY cwd ORDER BY COUNT(*) DESC"

echo ""
echo "=== Hour distribution (local) ==="
python3 -c "
import sqlite3
from datetime import datetime
from zoneinfo import ZoneInfo
db=sqlite3.connect('$DB')
rows=db.execute(\"SELECT ts FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND agent = '$AGENT_NAME'\").fetchall()
tz=ZoneInfo('$USER_TZ')
buckets={}
for (ts,) in rows:
    dt=datetime.strptime(ts.replace('Z','+0000'),'%Y-%m-%dT%H:%M:%S%z').astimezone(tz)
    buckets[dt.hour]=buckets.get(dt.hour,0)+1
for h in sorted(buckets):
    print(f'{h:02d}:00 {buckets[h]}')
"

echo ""
echo "=== Sessions ==="
sqlite3 "$DB" "SELECT session, COUNT(*) AS calls, MIN(ts) AS first, MAX(ts) AS last FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND agent = '$AGENT_NAME' GROUP BY session ORDER BY first"
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
