---
description: Compare today vs yesterday side-by-side from FluxMirror SQLite
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

## Step 1: Extract both windows

```bash
if [ ! -f "$DB" ]; then
  echo "FluxMirror DB not found. Run an agent session first."
  exit 0
fi

read TODAY_LOCAL TODAY_START TODAY_END TODAY_START_MS TODAY_END_MS <<EOF
$(fluxmirror window --tz "$USER_TZ" --period today)
EOF

read YEST_LOCAL YEST_START YEST_END YEST_START_MS YEST_END_MS <<EOF
$(fluxmirror window --tz "$USER_TZ" --period yesterday)
EOF

echo "=== Today ($TODAY_LOCAL): $TODAY_START to $TODAY_END ==="
echo "=== Yesterday ($YEST_LOCAL): $YEST_START to $YEST_END ==="

# Tool-name normalization: Claude PascalCase + Gemini/Qwen snake_case
WRITE_TOOLS="('Edit','Write','MultiEdit','edit_file','write_file','replace')"

echo ""
echo "=== TODAY: per-agent calls ==="
fluxmirror sqlite --db "$DB" "SELECT agent, COUNT(*) FROM agent_events WHERE ts >= '$TODAY_START' AND ts < '$TODAY_END' GROUP BY agent ORDER BY COUNT(*) DESC"

echo ""
echo "=== YESTERDAY: per-agent calls ==="
fluxmirror sqlite --db "$DB" "SELECT agent, COUNT(*) FROM agent_events WHERE ts >= '$YEST_START' AND ts < '$YEST_END' GROUP BY agent ORDER BY COUNT(*) DESC"

echo ""
echo "=== TODAY: edited files ==="
fluxmirror sqlite --db "$DB" "SELECT detail, COUNT(*) FROM agent_events WHERE ts >= '$TODAY_START' AND ts < '$TODAY_END' AND tool IN $WRITE_TOOLS GROUP BY detail ORDER BY COUNT(*) DESC LIMIT 15"

echo ""
echo "=== YESTERDAY: edited files ==="
fluxmirror sqlite --db "$DB" "SELECT detail, COUNT(*) FROM agent_events WHERE ts >= '$YEST_START' AND ts < '$YEST_END' AND tool IN $WRITE_TOOLS GROUP BY detail ORDER BY COUNT(*) DESC LIMIT 15"

echo ""
echo "=== Continued vs new (today's edited files seen yesterday too) ==="
fluxmirror sqlite --db "$DB" "WITH today_files AS (SELECT DISTINCT detail FROM agent_events WHERE ts >= '$TODAY_START' AND ts < '$TODAY_END' AND tool IN $WRITE_TOOLS), yest_files AS (SELECT DISTINCT detail FROM agent_events WHERE ts >= '$YEST_START' AND ts < '$YEST_END' AND tool IN $WRITE_TOOLS) SELECT 'continued: ' || detail FROM today_files WHERE detail IN (SELECT detail FROM yest_files) UNION ALL SELECT 'new today: ' || detail FROM today_files WHERE detail NOT IN (SELECT detail FROM yest_files) UNION ALL SELECT 'dropped: '   || detail FROM yest_files  WHERE detail NOT IN (SELECT detail FROM today_files) ORDER BY 1"

echo ""
echo "=== Working directories: today ==="
fluxmirror sqlite --db "$DB" "SELECT cwd, COUNT(*) FROM agent_events WHERE ts >= '$TODAY_START' AND ts < '$TODAY_END' GROUP BY cwd ORDER BY COUNT(*) DESC"

echo ""
echo "=== Working directories: yesterday ==="
fluxmirror sqlite --db "$DB" "SELECT cwd, COUNT(*) FROM agent_events WHERE ts >= '$YEST_START' AND ts < '$YEST_END' GROUP BY cwd ORDER BY COUNT(*) DESC"

echo ""
echo "=== TODAY: MCP traffic methods ==="
fluxmirror sqlite --db "$DB" "SELECT method, COUNT(*) FROM events WHERE ts_ms >= $TODAY_START_MS AND ts_ms < $TODAY_END_MS AND method IS NOT NULL GROUP BY method ORDER BY COUNT(*) DESC"

echo ""
echo "=== YESTERDAY: MCP traffic methods ==="
fluxmirror sqlite --db "$DB" "SELECT method, COUNT(*) FROM events WHERE ts_ms >= $YEST_START_MS AND ts_ms < $YEST_END_MS AND method IS NOT NULL GROUP BY method ORDER BY COUNT(*) DESC"

echo ""
echo "=== TODAY: files touched by multiple agents ==="
fluxmirror sqlite --db "$DB" "SELECT detail, COUNT(DISTINCT agent) AS agents, GROUP_CONCAT(DISTINCT agent) AS who FROM agent_events WHERE ts >= '$TODAY_START' AND ts < '$TODAY_END' AND tool IN $WRITE_TOOLS AND detail IS NOT NULL GROUP BY detail HAVING agents >= 2 ORDER BY agents DESC LIMIT 10"

echo ""
echo "=== YESTERDAY: files touched by multiple agents ==="
fluxmirror sqlite --db "$DB" "SELECT detail, COUNT(DISTINCT agent) AS agents, GROUP_CONCAT(DISTINCT agent) AS who FROM agent_events WHERE ts >= '$YEST_START' AND ts < '$YEST_END' AND tool IN $WRITE_TOOLS AND detail IS NOT NULL GROUP BY detail HAVING agents >= 2 ORDER BY agents DESC LIMIT 10"
```

## Step 2: Inference

Use the lifecycle / effort signals from `/fluxmirror:report-today` Step
2 on each day independently. Then compare:

- Files in **continued** → ongoing work, multi-day feature
- Files in **new today** → today's fresh focus
- Files in **dropped** → finished or paused
- cwd shift (different primary cwd today vs yesterday) → context switch
- Call-count delta (today >> yesterday or vice versa) → effort shift
- **Agent shift**: agent set differs across days, or share rebalances
  (e.g., yesterday all Claude, today 50/50 Claude/Gemini) → call out
  who joined / dropped and on which files
- **Cross-day handoff**: a file in "continued" was edited by agent A
  yesterday and agent B today → flag as handoff

## Step 3: Output

### English format

# Today vs Yesterday (<TODAY> vs <YEST> <timezone>)

## Side-by-side
| | Yesterday (<date>) | Today (<date>) |
|---|---|---|
| Calls | N | N |
| Sessions | N | N |
| Agents | comma-list | comma-list |
| Primary cwd | … | … |
| Top files | … | … |
| MCP methods | top-3 or "—" | top-3 or "—" |

## What's new today
- file or theme not present yesterday

## What continued
- multi-day items

## What was dropped
- yesterday-only items

## Insights
- 1-3 observed patterns (effort shift, cwd switch, lifecycle change)

### Korean format

# 오늘 vs 어제 (<TODAY> vs <YEST> <timezone>)

## 비교표
## 오늘 새로 시작
## 이어진 작업
## 마무리되거나 멈춘 작업
## 인사이트

### Other languages

Same structure, translated naturally.

## Step 4: Empty data

If either day has fewer than 5 events, note that in the chosen
language and only display the populated side.
