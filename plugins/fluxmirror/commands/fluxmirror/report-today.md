---
description: Summarize today's AI agent activity from FluxMirror SQLite
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

# Fallback: system locale
if [ -z "$USER_LANG" ]; then
  SYS=$(echo "${LANG:-en_US.UTF-8}" | cut -d_ -f1)
  case "$SYS" in
    ko) USER_LANG="korean" ;;
    ja) USER_LANG="japanese" ;;
    zh) USER_LANG="chinese" ;;
    *)  USER_LANG="english" ;;
  esac
fi

# Fallback: /etc/localtime
if [ -z "$USER_TZ" ]; then
  USER_TZ=$(readlink /etc/localtime 2>/dev/null | sed 's|.*/zoneinfo/||')
  [ -z "$USER_TZ" ] && USER_TZ="UTC"
fi

echo "Settings: language=$USER_LANG timezone=$USER_TZ"
```

## Step 1: Extract data

```bash
DB="${FLUXMIRROR_DB:-$HOME/Library/Application Support/fluxmirror/events.db}"

if [ ! -f "$DB" ]; then
  echo "FluxMirror DB not found. Run an agent session first."
  exit 0
fi

read TODAY_LOCAL START_UTC END_UTC START_MS END_MS <<EOF
$(python3 -c "
from datetime import datetime, timedelta
from zoneinfo import ZoneInfo
tz=ZoneInfo('$USER_TZ')
now=datetime.now(tz)
start=now.replace(hour=0,minute=0,second=0,microsecond=0)
end=start+timedelta(days=1)
su=start.astimezone(ZoneInfo('UTC'))
eu=end.astimezone(ZoneInfo('UTC'))
print(start.strftime('%Y-%m-%d'), su.strftime('%Y-%m-%dT%H:%M:%SZ'), eu.strftime('%Y-%m-%dT%H:%M:%SZ'), int(su.timestamp()*1000), int(eu.timestamp()*1000))
")
EOF

echo "=== Range: $START_UTC to $END_UTC ($USER_TZ; local date: $TODAY_LOCAL) ==="

# Tool-name normalization: cover Claude PascalCase + Gemini/Qwen snake_case.
# Used as in-clause sentinels for tool-class filters below.
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
echo "=== Tool mix ==="
sqlite3 "$DB" "SELECT tool, COUNT(*) FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' GROUP BY tool ORDER BY COUNT(*) DESC"

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

echo ""
echo "=== Streak (past 14 days, days with >5 calls) ==="
python3 -c "
import sqlite3
from datetime import datetime, timedelta
from zoneinfo import ZoneInfo
db=sqlite3.connect('$DB')
tz=ZoneInfo('$USER_TZ')
now=datetime.now(tz)
end=now.replace(hour=0,minute=0,second=0,microsecond=0)+timedelta(days=1)
start=end-timedelta(days=14)
su=start.astimezone(ZoneInfo('UTC')).strftime('%Y-%m-%dT%H:%M:%SZ')
eu=end.astimezone(ZoneInfo('UTC')).strftime('%Y-%m-%dT%H:%M:%SZ')
rows=db.execute(\"SELECT ts FROM agent_events WHERE ts >= ? AND ts < ?\", (su,eu)).fetchall()
counts={}
for (ts,) in rows:
    dt=datetime.strptime(ts.replace('Z','+0000'),'%Y-%m-%dT%H:%M:%S%z').astimezone(tz)
    d=dt.strftime('%Y-%m-%d')
    counts[d]=counts.get(d,0)+1
active=sorted(d for d,c in counts.items() if c>5)
print('active_days:', ','.join(active))
streak=0
day=now.strftime('%Y-%m-%d')
cur=now.replace(hour=0,minute=0,second=0,microsecond=0)
while cur.strftime('%Y-%m-%d') in counts and counts[cur.strftime('%Y-%m-%d')]>5:
    streak+=1
    cur-=timedelta(days=1)
print('current_streak_days:', streak)
"

echo ""
echo "=== First-touch files (today only, not seen in past 30 days) ==="
python3 -c "
import sqlite3
from datetime import datetime, timedelta
from zoneinfo import ZoneInfo
db=sqlite3.connect('$DB')
tz=ZoneInfo('$USER_TZ')
hist_start=(datetime.now(tz).replace(hour=0,minute=0,second=0,microsecond=0)-timedelta(days=30)).astimezone(ZoneInfo('UTC')).strftime('%Y-%m-%dT%H:%M:%SZ')
touch_tools=\"('Edit','Write','MultiEdit','Read','edit_file','write_file','replace','read_file','read_many_files')\"
today_files=set(r[0] for r in db.execute(f\"SELECT DISTINCT detail FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' AND tool IN {touch_tools} AND detail IS NOT NULL\").fetchall())
prior_files=set(r[0] for r in db.execute(f\"SELECT DISTINCT detail FROM agent_events WHERE ts >= ? AND ts < '$START_UTC' AND tool IN {touch_tools} AND detail IS NOT NULL\", (hist_start,)).fetchall())
new=sorted(today_files - prior_files)
for f in new[:20]:
    print(f)
"
```

## Step 2: Inference

Look at the data and infer **"what work was done"**. Do NOT just list facts.

### Feature lifecycle stage (read tool mix and Bash patterns)

- Lots of Read + Glob, few Edit → **research / exploration**
- Many Edit on a small set of related files → **active implementation**
- Edit + Bash with test/build commands (`cargo test`, `npm test`,
  `pytest`, `go test`, `make`) → **testing / iteration**
- `git tag` + `git push origin v...` + `curl .../releases/...` → **shipping**
- New file `Write` followed by repeated `Edit`s on the same path →
  **stabilizing a new module**

### Effort estimation (read time clustering)

- Single dense window with edits on the same file group, > 60 min
  uninterrupted → call it a **deep focus session** and quote the
  duration ("90 min deep focus")
- Multiple short bursts (< 10 min each) across different cwds → call
  it **context-switching day**
- One isolated < 10 min burst → **quick fix**
- Long stretch dominated by Read + Glob → **investigation session**

### Iterative refinement signals

- Same file edited > 5 times in one day → **iterative refinement**;
  quote the count ("edited X 7 times")
- Two files edited in alternation (e.g., a header ↔ its impl, or
  schema ↔ migration) → **paired refactor** or **dual-write integration**
- Repeated Bash invocations of the same build / test command → quote
  cycle count ("4 cargo cycles", "6 pytest runs")

### Multi-agent signals (when ≥ 2 distinct agents in per-agent table)

- One agent dominates (≥ 80% of calls) → label as **primary driver**, the
  other(s) as **observers** or **side runs**
- Per-agent tool mix differs sharply (e.g., Claude does Edit-heavy, Gemini
  does Read-only) → call out **division of labor**
- Same file appears in the "Files touched by multiple agents" output →
  **handoff** (sequential) or **collision** (overlapping windows); check
  timestamps in `Bash commands` and `Hour distribution` to disambiguate
- Distinct cwd per agent → **agent-per-project split**

### Common file-pair shortcuts

- `Cargo.toml` + `src/**/*.rs` together → Rust module work
- `marketplace.json` + `plugin.json` together → version sync
- `commands/*.md` + `.claude-plugin/plugin.json` → plugin command surface change
- `hooks/*.sh` + `bin/*` → install / wrapper change

### Time clustering

Group events by gaps > 30 min. Each cluster is one task group; label
each by its dominant lifecycle stage and effort signature.

## Step 3: Output

Output in `USER_LANG`. **What was done for what purpose** is the key.

Weave the lifecycle / effort / iterative cues from Step 2 directly into
each Key Activities bullet — don't list them separately. A good bullet
reads like:

> **SQLite integration** — Deep focus session (90 min). Iteratively
> refined `store.rs` (7 edits) and `bridge.rs` (5 edits) until the
> writer queue and framer interplay stabilized. Built and tested via
> 5 cargo cycles.

A weak bullet reads like:

> **SQLite integration** — `store.rs` was modified, `bridge.rs` was
> modified.

### Insights generation rules

After the stats table, emit 1–3 **observed** insights. Only mention
patterns the data clearly supports — never fabricate. Examples:

- "Most productive hour: 02:00 KST (38 calls)"
- "5 new files added today, all under `plugins/fluxmirror/commands/`"
- "First time touching `src/security/guard.py` in 30 days"
- "Edit-to-read ratio: 0.4 (heavier exploration than usual)"
- "Multi-project day: switched between fluxmirror and discord-claude-bot 4 times"
- "3rd consecutive day of fluxmirror activity"

Streak rule: use the `current_streak_days` value from the streak query.
Only mention a streak if it is ≥ 2 days. First-touch rule: only mention
files that appear in the "First-touch files" output. Multi-project
rule: only mention if ≥ 2 cwds each had ≥ 5 calls today.

### Korean format (when USER_LANG=korean)

# 오늘의 작업 (YYYY-MM-DD <timezone>)

## 핵심 작업

- **[goal]** — [lifecycle/effort 한 줄] [files/area + 반복 횟수 또는
  사이클 수가 있으면 함께]

(1-2 activities is fine. Don't pad.)

## 활동 통계

| 에이전트 | 호출 | 세션 |
|---|---|---|
| (per-agent calls 결과의 모든 행 — 단일 에이전트면 한 줄, 멀티면 여러 줄) |

(에이전트 ≥ 2면 바로 아래에 "## 다중 에이전트" 섹션 추가:
- 도구 분포 차이, 공유 파일, 시간대 분리 등 위 Multi-agent signals 규칙 적용)

## 인사이트

- 1–3 lines of observed patterns (data-supported only)

## 시간대

[Active windows in user timezone]

### English format (when USER_LANG=english)

# Today's Work (YYYY-MM-DD <timezone>)

## Key Activities

- **[Objective]** — [lifecycle / effort one-liner] [files/area with edit
  counts or build/test cycle counts when present]

## Activity Stats

| Agent | Calls | Sessions |
|---|---|---|
| (one row per agent from the per-agent query — single row if solo, multiple if ≥2) |

(If ≥ 2 agents present, add a "## Multi-Agent" section right after this
table: tool-mix differences, shared files, time-window separation —
apply the Multi-agent signals rules above.)

## Insights

- 1–3 lines of observed patterns (data-supported only)

## Active Hours

[User-timezone-based]

### Other languages (japanese, chinese)

Same structure, translated naturally. Use `## インサイト` for Japanese
and `## 洞察` for Chinese.

## Step 4: Empty data

If less than 5 events, output in chosen language:

- ko: `오늘 활동 적음.`
- en: `Limited activity today.`
- ja: `本日の活動は少なめです。`
- zh: `今日活动较少。`
