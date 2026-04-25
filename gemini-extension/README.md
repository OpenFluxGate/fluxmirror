# fluxmirror extension for Gemini CLI

Audit every Gemini CLI tool call by logging it to a daily JSONL file
**and** the same SQLite database used by the Claude Code plugin and the
Rust MCP proxy (`fluxmirror-proxy`) — so all your agents share one
queryable history.

## What it does

Registers an **AfterTool** hook that fires after every tool invocation.
Each call:

1. Appends one JSON line to `~/.gemini/session-logs/YYYY-MM-DD.jsonl`
2. Writes one parameter-bound row into FluxMirror SQLite at
   `~/Library/Application Support/fluxmirror/events.db`, table
   `agent_events`, with `agent='gemini-cli'`.

JSONL line fields:

| Field     | Description                          |
|-----------|--------------------------------------|
| `ts`      | UTC timestamp (ISO 8601)             |
| `session` | Session ID                           |
| `tool`    | Tool name (Gemini snake_case: `read_file`, `run_shell_command`, …) |
| `detail`  | First 200 chars of the primary input |
| `cwd`     | Working directory at time of call    |

## Install

From a local clone:

```bash
gemini extensions install ./gemini-extension
```

Remote install:

```bash
gemini extensions install https://github.com/OpenFluxGate/fluxmirror
```

## Requirements

- `jq` on PATH (`brew install jq`)
- `python3` on PATH (used by the shared `_dual_write.py` helper for
  parameter-bound SQLite writes that survive arbitrary input)

## Configuration (optional env vars)

| Variable               | Effect                                              |
|------------------------|------------------------------------------------------|
| `FLUXMIRROR_DB`        | Override DB path                                    |
| `FLUXMIRROR_SKIP_SELF` | If `1`, combined with `FLUXMIRROR_SELF_REPO`, skips events that look like fluxmirror querying its own DB from inside its own repo. |
| `FLUXMIRROR_SELF_REPO` | Absolute path to the fluxmirror repo for the filter above. |

Hook-side errors are appended to `~/.fluxmirror/hook-errors.log`. The
log auto-rotates at 5 MiB, keeping one backup as `hook-errors.log.1`.

The hook recognizes ~20 tool names across Claude Code (PascalCase) and
Gemini/Qwen Code (snake_case), and extracts the most informative
`detail` field per tool (e.g., `command` for shell, the file path for
read/write/edit, the URL for web fetch). Unknown tools fall back to the
first string-typed `tool_input` value.

## Tool naming convention

Gemini CLI emits tool names in **snake_case** (`read_file`,
`run_shell_command`, `write_file`, `replace`, …) — different from
Claude Code's PascalCase (`Read`, `Bash`, `Write`, `Edit`, `MultiEdit`).
The fluxmirror reports normalize across both naming styles, so a single
report covers all agents uniformly. The DB stores the raw tool name as
emitted, so original fidelity is preserved for ad-hoc queries.

## Schema verification

The hook input fields (`session_id`, `cwd`, `tool_name`, `tool_input`,
`hook_event_name`) and the `AfterTool` event name were verified against
the official Gemini CLI hooks reference at
<https://geminicli.com/docs/hooks/reference/>.

## Isolation across agents

Each agent's JSONL output goes to a different directory
(`~/.claude/`, `~/.gemini/`, `~/.qwen/`). Run
`scripts/verify-isolation.sh` from the repo root to confirm session IDs
do not leak across them.

Qwen Code does not need this extension — it installs the Claude plugin
directly via `qwen extensions install OpenFluxGate/fluxmirror:fluxmirror`.
The shared hook auto-detects Qwen at runtime and labels rows
`agent='qwen-code'`.

## License

MIT
