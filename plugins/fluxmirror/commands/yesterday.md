---
description: Summarize yesterday's AI agent activity from FluxMirror SQLite
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

## Step 1: Extract data (yesterday in $USER_TZ)

```bash
DB="${FLUXMIRROR_DB:-$HOME/Library/Application Support/fluxmirror/events.db}"

if [ ! -f "$DB" ]; then
  echo "FluxMirror DB not found. Run an agent session first."
  exit 0
fi

read TARGET_LOCAL START_UTC END_UTC START_MS END_MS <<EOF
$(python3 -c "
from datetime import datetime, timedelta
from zoneinfo import ZoneInfo
tz=ZoneInfo('$USER_TZ')
now=datetime.now(tz)
end=now.replace(hour=0,minute=0,second=0,microsecond=0)
start=end-timedelta(days=1)
su=start.astimezone(ZoneInfo('UTC'))
eu=end.astimezone(ZoneInfo('UTC'))
print(start.strftime('%Y-%m-%d'), su.strftime('%Y-%m-%dT%H:%M:%SZ'), eu.strftime('%Y-%m-%dT%H:%M:%SZ'), int(su.timestamp()*1000), int(eu.timestamp()*1000))
")
EOF

echo "=== Range: $START_UTC to $END_UTC ($USER_TZ; local date: $TARGET_LOCAL) ==="

# Tool-name normalization: Claude PascalCase + Gemini/Qwen snake_case
WRITE_TOOLS="('Edit','Write','MultiEdit','edit_file','write_file','replace')"
READ_TOOLS="('Read','read_file','read_many_files')"
SHELL_TOOLS="('Bash','run_shell_command')"

echo ""
echo "=== Per-agent calls ==="
sqlite3 "$DB" "SELECT agent, COUNT(*) AS calls, COUNT(DISTINCT session) AS sessions FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' GROUP BY agent ORDER BY calls DESC"

echo ""
echo "=== Files touched by multiple agents (collaboration / collision) ==="
sqlite3 "$DB" "SELECT detail, COUNT(DISTINCT agent) AS agents, GROUP_CONCAT(DISTINCT agent) AS who FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND tool IN $WRITE_TOOLS AND detail IS NOT NULL GROUP BY detail HAVING agents >= 2 ORDER BY agents DESC, detail LIMIT 10"

echo ""
echo "=== Files written or edited ==="
sqlite3 "$DB" "SELECT detail, tool, COUNT(*) FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND tool IN $WRITE_TOOLS GROUP BY detail, tool ORDER BY COUNT(*) DESC LIMIT 20"

echo ""
echo "=== Files only read ==="
sqlite3 "$DB" "SELECT detail, COUNT(*) FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND tool IN $READ_TOOLS GROUP BY detail ORDER BY COUNT(*) DESC LIMIT 10"

echo ""
echo "=== Shell commands ==="
sqlite3 "$DB" "SELECT substr(ts, 12, 5) AS time_utc, detail FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND tool IN $SHELL_TOOLS ORDER BY ts"

echo ""
echo "=== Working directories ==="
sqlite3 "$DB" "SELECT cwd, COUNT(*) FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' GROUP BY cwd ORDER BY COUNT(*) DESC"

echo ""
echo "=== MCP traffic methods ==="
sqlite3 "$DB" "SELECT method, COUNT(*) FROM events WHERE ts_ms >= $START_MS AND ts_ms < $END_MS AND method IS NOT NULL GROUP BY method ORDER BY COUNT(*) DESC"

echo ""
echo "=== Hour distribution (local) ==="
python3 -c "
import sqlite3
from datetime import datetime
from zoneinfo import ZoneInfo
db=sqlite3.connect('$DB')
rows=db.execute(\"SELECT ts FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC'\").fetchall()
tz=ZoneInfo('$USER_TZ')
buckets={}
for (ts,) in rows:
    dt=datetime.strptime(ts.replace('Z','+0000'),'%Y-%m-%dT%H:%M:%S%z').astimezone(tz)
    buckets[dt.hour]=buckets.get(dt.hour,0)+1
for h in sorted(buckets):
    print(f'{h:02d}:00 {buckets[h]}')
"
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
  ratio, multi-project switches, new-file count, streak, etc.)

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
