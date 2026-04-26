#!/bin/bash
# Emit FluxMirror SQLite data for a given period.
#
# Usage:
#   report-data.sh today | yesterday | week
#   report-data.sh doctor   # health table from `fluxmirror doctor`
#   report-data.sh config   # full resolved config from `fluxmirror config show`
#   report-data.sh agents   # per-agent 7-day quick stat
#
# Read by Gemini CLI custom commands (commands/*.toml) via
# !{...} interpolation. Output is plain text with "=== section ==="
# headers; the model parses it.
#
# Deps: bash, the `fluxmirror` binary (on $PATH or invoked through the
# auto-download wrapper).

set -euo pipefail

PERIOD="${1:-today}"

# ----------------------------------------------------------------------
# Resolve language / timezone / db path via the new fluxmirror binary,
# falling back to env vars + sane defaults if the binary is missing
# (older installs that haven't run the post-install upgrade yet).
# ----------------------------------------------------------------------
if command -v fluxmirror >/dev/null 2>&1; then
  USER_LANG=$(fluxmirror config get language 2>/dev/null || echo english)
  USER_TZ=$(fluxmirror config get timezone 2>/dev/null || echo UTC)
  DB=$(fluxmirror db-path 2>/dev/null || echo "${FLUXMIRROR_DB:-$HOME/Library/Application Support/fluxmirror/events.db}")
else
  USER_LANG=english
  USER_TZ="${TZ:-UTC}"
  DB="${FLUXMIRROR_DB:-$HOME/Library/Application Support/fluxmirror/events.db}"
fi
[ -z "$USER_LANG" ] && USER_LANG=english
[ -z "$USER_TZ" ] && USER_TZ=UTC
[ -z "$DB" ] && DB="${FLUXMIRROR_DB:-$HOME/Library/Application Support/fluxmirror/events.db}"

# Honour FLUXMIRROR_DB explicitly (used by tests and the regression
# suite) — overrides whatever `fluxmirror db-path` reported.
if [ -n "${FLUXMIRROR_DB:-}" ]; then
  DB="$FLUXMIRROR_DB"
fi

# ----------------------------------------------------------------------
# Mode dispatch for the new commands (doctor / config / agents).
# These are short and stand-alone — no window / SQL needed.
# ----------------------------------------------------------------------
case "$PERIOD" in
  doctor)
    if ! command -v fluxmirror >/dev/null 2>&1; then
      echo "fluxmirror binary not found on PATH. Install via the latest release."
      exit 0
    fi
    fluxmirror doctor
    exit 0
    ;;
  config)
    if ! command -v fluxmirror >/dev/null 2>&1; then
      echo "fluxmirror binary not found on PATH. Install via the latest release."
      exit 0
    fi
    fluxmirror config show
    exit 0
    ;;
  agents)
    if [ ! -f "$DB" ]; then
      echo "FluxMirror DB not found at: $DB"
      echo "Run an agent session first."
      exit 0
    fi
    if ! command -v fluxmirror >/dev/null 2>&1; then
      echo "fluxmirror binary not found on PATH. Install via the latest release."
      exit 0
    fi
    # Window: trailing 7 days in the user timezone.
    read WS WE START_UTC END_UTC START_MS END_MS <<EOF
$(fluxmirror window --tz "$USER_TZ" --period week)
EOF
    echo "Window: $START_UTC .. $END_UTC ($USER_TZ; $WS .. $WE)"
    echo ""
    echo "=== Per-agent 7-day totals ==="
    fluxmirror sqlite --db "$DB" "SELECT agent, COUNT(*) AS calls, COUNT(DISTINCT session) AS sessions, MIN(ts) AS first_ts, MAX(ts) AS last_ts FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' GROUP BY agent ORDER BY calls DESC"
    echo ""
    echo "=== Per-agent dominant tool (7d) ==="
    fluxmirror sqlite --db "$DB" "SELECT agent, tool, COUNT(*) AS n FROM agent_events WHERE ts >= '$START_UTC' AND ts < '$END_UTC' GROUP BY agent, tool ORDER BY agent, n DESC"
    exit 0
    ;;
esac

# ----------------------------------------------------------------------
# today / yesterday / week — the original data extraction flow.
# ----------------------------------------------------------------------
if [ ! -f "$DB" ]; then
  echo "FluxMirror DB not found at: $DB"
  echo "Run an agent session first."
  exit 0
fi

if ! command -v fluxmirror >/dev/null 2>&1; then
  echo "fluxmirror binary not found on PATH. Install via the latest release."
  exit 0
fi

# Resolve the time window via `fluxmirror window`. Today/yesterday emit
# 5 fields; week emits 6 (with a leading WEEK_END_LOCAL label too).
case "$PERIOD" in
  today|yesterday)
    read LABEL START END START_MS END_MS <<EOF
$(fluxmirror window --tz "$USER_TZ" --period "$PERIOD")
EOF
    LABEL="$LABEL|$USER_TZ"
    ;;
  week)
    read WS WE START END START_MS END_MS <<EOF
$(fluxmirror window --tz "$USER_TZ" --period week)
EOF
    LABEL="$WS..$WE|$USER_TZ"
    ;;
  *)
    echo "report-data.sh: unknown period '$PERIOD' (expected today|yesterday|week|doctor|config|agents)" >&2
    exit 2
    ;;
esac

WRITE_TOOLS="('Edit','Write','MultiEdit','edit_file','write_file','replace')"
READ_TOOLS="('Read','read_file','read_many_files')"
SHELL_TOOLS="('Bash','run_shell_command')"

echo "Period: $PERIOD ($LABEL)"
echo "Window UTC: $START .. $END"

echo ""
echo "=== Per-agent calls ==="
fluxmirror sqlite --db "$DB" "SELECT agent, COUNT(*) AS calls, COUNT(DISTINCT session) AS sessions FROM agent_events WHERE ts >= '$START' AND ts < '$END' GROUP BY agent ORDER BY calls DESC"

echo ""
echo "=== Files written or edited ==="
fluxmirror sqlite --db "$DB" "SELECT detail, tool, COUNT(*) FROM agent_events WHERE ts >= '$START' AND ts < '$END' AND tool IN $WRITE_TOOLS GROUP BY detail, tool ORDER BY COUNT(*) DESC LIMIT 20"

echo ""
echo "=== Files only read ==="
fluxmirror sqlite --db "$DB" "SELECT detail, COUNT(*) FROM agent_events WHERE ts >= '$START' AND ts < '$END' AND tool IN $READ_TOOLS GROUP BY detail ORDER BY COUNT(*) DESC LIMIT 10"

echo ""
echo "=== Shell commands ==="
fluxmirror sqlite --db "$DB" "SELECT substr(ts,12,5), tool, detail FROM agent_events WHERE ts >= '$START' AND ts < '$END' AND tool IN $SHELL_TOOLS ORDER BY ts LIMIT 50"

echo ""
echo "=== Working directories ==="
fluxmirror sqlite --db "$DB" "SELECT cwd, COUNT(*) FROM agent_events WHERE ts >= '$START' AND ts < '$END' GROUP BY cwd ORDER BY COUNT(*) DESC"

echo ""
echo "=== Files touched by multiple agents ==="
fluxmirror sqlite --db "$DB" "SELECT detail, COUNT(DISTINCT agent), GROUP_CONCAT(DISTINCT agent) FROM agent_events WHERE ts >= '$START' AND ts < '$END' AND tool IN $WRITE_TOOLS AND detail IS NOT NULL GROUP BY detail HAVING COUNT(DISTINCT agent) >= 2 ORDER BY 2 DESC LIMIT 10"

echo ""
echo "=== MCP traffic methods (events table from fluxmirror-proxy) ==="
fluxmirror sqlite --db "$DB" "SELECT method, COUNT(*) FROM events WHERE ts_ms >= $START_MS AND ts_ms < $END_MS AND method IS NOT NULL GROUP BY method ORDER BY COUNT(*) DESC"

echo ""
echo "=== Hour distribution (local) ==="
fluxmirror histogram --db "$DB" --tz "$USER_TZ" --start "$START" --end "$END"

if [ "$PERIOD" = "week" ]; then
  echo ""
  echo "=== Daily totals (all 7 days, zero-event days included) ==="
  fluxmirror daily-totals --db "$DB" --tz "$USER_TZ" --start "$START" --end "$END"

  echo ""
  echo "=== Per-day file counts (new vs edited) ==="
  fluxmirror per-day-files --db "$DB" --tz "$USER_TZ" --start "$START" --end "$END"
fi
