#!/bin/bash
# Emit FluxMirror SQLite data for a given period.
# Usage: report-data.sh <today|yesterday|week>
#
# Read by Gemini CLI custom commands (commands/fluxmirror/*.toml) via
# !{…} interpolation. Output format is plain text with "=== section ==="
# headers; the model parses it.
#
# Deps: bash, python3 (zoneinfo), sqlite3 CLI.

set -euo pipefail

PERIOD="${1:-today}"
DB="${FLUXMIRROR_DB:-$HOME/Library/Application Support/fluxmirror/events.db}"

if [ ! -f "$DB" ]; then
  echo "FluxMirror DB not found at: $DB"
  echo "Run an agent session first."
  exit 0
fi

read LABEL START END START_MS END_MS <<EOF
$(python3 - "$PERIOD" <<'PY'
import json, os, sys
from datetime import datetime, timedelta
from zoneinfo import ZoneInfo

period = sys.argv[1]
cfg = os.path.expanduser('~/.fluxmirror/config.json')
tz_name = 'UTC'
try:
    with open(cfg) as f:
        tz_name = json.load(f).get('timezone', 'UTC')
except Exception:
    pass
tz = ZoneInfo(tz_name)
now = datetime.now(tz)

if period == 'yesterday':
    end = now.replace(hour=0, minute=0, second=0, microsecond=0)
    start = end - timedelta(days=1)
    label = start.strftime('%Y-%m-%d')
elif period == 'week':
    end = (now + timedelta(days=1)).replace(hour=0, minute=0, second=0, microsecond=0)
    start = end - timedelta(days=7)
    label = start.strftime('%Y-%m-%d') + '..' + (end - timedelta(days=1)).strftime('%Y-%m-%d')
else:  # today (default)
    start = now.replace(hour=0, minute=0, second=0, microsecond=0)
    end = start + timedelta(days=1)
    label = start.strftime('%Y-%m-%d')

su = start.astimezone(ZoneInfo('UTC'))
eu = end.astimezone(ZoneInfo('UTC'))
print(f'{label}|{tz_name}',
      su.strftime('%Y-%m-%dT%H:%M:%SZ'),
      eu.strftime('%Y-%m-%dT%H:%M:%SZ'),
      int(su.timestamp() * 1000),
      int(eu.timestamp() * 1000))
PY
)
EOF

WRITE_TOOLS="('Edit','Write','MultiEdit','edit_file','write_file','replace')"
READ_TOOLS="('Read','read_file','read_many_files')"
SHELL_TOOLS="('Bash','run_shell_command')"

echo "Period: $PERIOD ($LABEL)"
echo "Window UTC: $START .. $END"

echo ""
echo "=== Per-agent calls ==="
sqlite3 "$DB" "SELECT agent, COUNT(*) AS calls, COUNT(DISTINCT session) AS sessions FROM agent_events WHERE ts >= '$START' AND ts < '$END' GROUP BY agent ORDER BY calls DESC"

echo ""
echo "=== Files written or edited ==="
sqlite3 "$DB" "SELECT detail, tool, COUNT(*) FROM agent_events WHERE ts >= '$START' AND ts < '$END' AND tool IN $WRITE_TOOLS GROUP BY detail, tool ORDER BY COUNT(*) DESC LIMIT 20"

echo ""
echo "=== Files only read ==="
sqlite3 "$DB" "SELECT detail, COUNT(*) FROM agent_events WHERE ts >= '$START' AND ts < '$END' AND tool IN $READ_TOOLS GROUP BY detail ORDER BY COUNT(*) DESC LIMIT 10"

echo ""
echo "=== Shell commands ==="
sqlite3 "$DB" "SELECT substr(ts,12,5), tool, detail FROM agent_events WHERE ts >= '$START' AND ts < '$END' AND tool IN $SHELL_TOOLS ORDER BY ts LIMIT 50"

echo ""
echo "=== Working directories ==="
sqlite3 "$DB" "SELECT cwd, COUNT(*) FROM agent_events WHERE ts >= '$START' AND ts < '$END' GROUP BY cwd ORDER BY COUNT(*) DESC"

echo ""
echo "=== Files touched by multiple agents ==="
sqlite3 "$DB" "SELECT detail, COUNT(DISTINCT agent), GROUP_CONCAT(DISTINCT agent) FROM agent_events WHERE ts >= '$START' AND ts < '$END' AND tool IN $WRITE_TOOLS AND detail IS NOT NULL GROUP BY detail HAVING COUNT(DISTINCT agent) >= 2 ORDER BY 2 DESC LIMIT 10"

echo ""
echo "=== MCP traffic methods (events table from fluxmirror-proxy) ==="
sqlite3 "$DB" "SELECT method, COUNT(*) FROM events WHERE ts_ms >= $START_MS AND ts_ms < $END_MS AND method IS NOT NULL GROUP BY method ORDER BY COUNT(*) DESC"

if [ "$PERIOD" = "week" ]; then
  echo ""
  echo "=== Daily totals (all 7 days, zero-event days included) ==="
  python3 - "$START" "$END" <<'PYDAYS'
import sqlite3, sys, os
from datetime import datetime, timedelta
from zoneinfo import ZoneInfo
import json
db_path = os.environ.get('FLUXMIRROR_DB',
    os.path.expanduser('~/Library/Application Support/fluxmirror/events.db'))
cfg = os.path.expanduser('~/.fluxmirror/config.json')
tz_name = 'UTC'
try:
    with open(cfg) as f:
        tz_name = json.load(f).get('timezone', 'UTC')
except Exception:
    pass
tz = ZoneInfo(tz_name)
db = sqlite3.connect(db_path)
rows = db.execute("SELECT ts, agent FROM agent_events WHERE ts >= ? AND ts < ?",
                  (sys.argv[1], sys.argv[2])).fetchall()
now = datetime.now(tz)
end = (now + timedelta(days=1)).replace(hour=0, minute=0, second=0, microsecond=0)
start = end - timedelta(days=7)
days = []
cur = start
while cur < end:
    days.append(cur.strftime('%Y-%m-%d (%a)'))
    cur += timedelta(days=1)
by_day = {d: 0 for d in days}
agents_by_day = {d: set() for d in days}
for ts, agent in rows:
    dt = datetime.strptime(ts.replace('Z', '+0000'), '%Y-%m-%dT%H:%M:%S%z').astimezone(tz)
    d = dt.strftime('%Y-%m-%d (%a)')
    if d in by_day:
        by_day[d] += 1
        agents_by_day[d].add(agent)
for d in days:
    a = ','.join(sorted(agents_by_day[d])) if agents_by_day[d] else '-'
    print(f'{d} | calls={by_day[d]} | agents={a}')
print(f'WEEK TOTAL | calls={sum(by_day.values())} | active_days={sum(1 for v in by_day.values() if v > 0)}')
PYDAYS
fi
