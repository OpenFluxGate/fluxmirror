#!/usr/bin/env python3
"""Shared SQLite writer for FluxMirror hooks.

Reads raw_json from stdin and metadata fields from argv. Writes one row
into the agent_events table using parameter binding (no escape pitfalls).

Usage (called from session-log.sh):
    printf '%s' "$INPUT" | python3 _dual_write.py \
        "$DB_PATH" "$TS" "$AGENT" "$SESSION" "$TOOL" "$DETAIL" "$CWD"

Errors are appended to ~/.fluxmirror/hook-errors.log so silent failures
become visible without polluting the agent's stdout/stderr.
"""
import os
import sqlite3
import sys
import traceback
from datetime import datetime, timezone
from pathlib import Path


SCHEMA = """
CREATE TABLE IF NOT EXISTS agent_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts TEXT NOT NULL,
  agent TEXT NOT NULL,
  session TEXT,
  tool TEXT,
  detail TEXT,
  cwd TEXT,
  raw_json TEXT
)
"""

INSERT = (
    "INSERT INTO agent_events "
    "(ts, agent, session, tool, detail, cwd, raw_json) "
    "VALUES (?, ?, ?, ?, ?, ?, ?)"
)


ERR_LOG_MAX_BYTES = 5 * 1024 * 1024  # 5 MiB before rotation


def _rotate_if_needed(err_log: Path) -> None:
    """If err_log exceeds the size cap, rename it to err_log.1 (overwriting
    any previous backup). Single-file rotation — keeps disk usage bounded
    at ~10 MiB worst case (current + .1)."""
    try:
        if err_log.exists() and err_log.stat().st_size >= ERR_LOG_MAX_BYTES:
            backup = err_log.with_suffix(err_log.suffix + ".1")
            # Path.replace is atomic and overwrites the target.
            err_log.replace(backup)
    except Exception:
        # Rotation is best-effort. If it fails, we still try to write below.
        pass


def log_error(msg: str) -> None:
    err_dir = Path.home() / ".fluxmirror"
    err_dir.mkdir(parents=True, exist_ok=True)
    err_log = err_dir / "hook-errors.log"
    _rotate_if_needed(err_log)
    ts = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    try:
        with err_log.open("a") as fh:
            fh.write(f"[{ts}] {msg}\n")
    except Exception:
        # If even the error log fails, give up silently — never break the
        # agent's tool call because of telemetry.
        pass


def main() -> int:
    if len(sys.argv) != 8:
        log_error(
            f"_dual_write.py expected 7 args, got {len(sys.argv) - 1}: "
            f"{sys.argv[1:]}"
        )
        return 0

    db_path, ts, agent, session, tool, detail, cwd = sys.argv[1:8]
    raw_json = sys.stdin.read()

    try:
        os.makedirs(os.path.dirname(db_path), exist_ok=True)
        conn = sqlite3.connect(db_path, timeout=5)
        try:
            conn.execute(SCHEMA)
            conn.execute(
                INSERT,
                (ts, agent, session, tool, detail, cwd, raw_json),
            )
            conn.commit()
        finally:
            conn.close()
    except Exception as e:
        log_error(f"insert failed for agent={agent} tool={tool}: {e}")
        log_error(traceback.format_exc().rstrip())
        return 0  # never propagate failure to the caller hook

    return 0


if __name__ == "__main__":
    sys.exit(main())
