# fluxmirror plugin for Claude Code (and Qwen Code)

Audit every Claude Code (or Qwen Code) tool call by logging it to a daily
JSONL file **and** a shared SQLite database used by the
`/fluxmirror:*` reporting commands.

## What it does

Registers a **PostToolUse** hook that fires after every tool invocation.
For each call, the hook:

1. Appends one JSON line to `~/<agent>/session-logs/YYYY-MM-DD.jsonl`
2. Writes one parameter-bound row into the FluxMirror SQLite database at
   `~/Library/Application Support/fluxmirror/events.db`, table
   `agent_events`, with the agent column set to either `claude-code` or
   `qwen-code` depending on which CLI is running.

JSONL line fields:

| Field     | Description                          |
|-----------|--------------------------------------|
| `ts`      | UTC timestamp (ISO 8601)             |
| `session` | Session ID                           |
| `tool`    | Tool name (Claude: `Read`/`Bash`/...; Qwen: `read_file`/`run_shell_command`/...) |
| `detail`  | First 200 chars of the primary input |
| `cwd`     | Working directory at time of call    |

## Install

```bash
/plugin marketplace add OpenFluxGate/fluxmirror
/plugin install fluxmirror@fluxmirror
```

## Also works on Qwen Code

Qwen Code accepts Claude marketplace plugins directly. The same plugin
installs and runs without modification:

```bash
qwen extensions install OpenFluxGate/fluxmirror:fluxmirror
```

The hook auto-detects whether it is running under Claude Code or Qwen
Code (via `$QWEN_CODE_NO_RELAUNCH` / `$QWEN_PROJECT_DIR` env signals
that Qwen sets at hook time) and labels rows / splits JSONL output
accordingly:

| Detected CLI | DB `agent` column | JSONL path                  |
|--------------|-------------------|------------------------------|
| Claude Code  | `claude-code`     | `~/.claude/session-logs/`   |
| Qwen Code    | `qwen-code`       | `~/.qwen/session-logs/`     |

## Requirements

- `bash` and `curl` on PATH (both universal on macOS / Linux / WSL)
- Network access on the first hook fire after install — the wrapper
  downloads the per-arch `fluxmirror-hook` Rust binary (~1.2 MB) from
  the latest GitHub release into `<plugin>/bin/` and execs it.
  Subsequent calls skip the download.

## Configuration (optional env vars)

| Variable               | Effect                                              |
|------------------------|------------------------------------------------------|
| `FLUXMIRROR_DB`        | Override DB path (default: `~/Library/Application Support/fluxmirror/events.db`) |
| `FLUXMIRROR_SKIP_SELF` | If `1`, combined with `FLUXMIRROR_SELF_REPO`, skips events that look like fluxmirror querying its own DB from inside its own repo (useful when self-developing fluxmirror). |
| `FLUXMIRROR_SELF_REPO` | Absolute path to the fluxmirror repo for the filter above. Anchored prefix match — adjacent dirs with similar names are not falsely filtered. |

Hook-side errors (e.g., DB locked, sqlite import failure) are appended
to `~/.fluxmirror/hook-errors.log` rather than swallowed silently — so
silent regressions become visible. The log auto-rotates at 5 MiB,
keeping one backup as `hook-errors.log.1`.

The hook recognizes ~20 tool names across Claude Code (PascalCase like
`Read`, `Bash`, `Edit`, `MultiEdit`, `WebFetch`, `WebSearch`, `Task`,
`TodoWrite`, `Glob`, `Grep`, `NotebookEdit`, `BashOutput`, `KillBash`,
`ExitPlanMode`) and Gemini/Qwen Code (snake_case like `read_file`,
`run_shell_command`, `write_file`, `replace`, `glob`,
`search_file_content`, `web_fetch`, `web_search`, `save_memory`,
`todo_write`, `read_many_files`, `kill_shell`) and extracts the most
informative `detail` field per tool (e.g., `command` for shell, the
file path for read/write/edit, the URL for web fetch). Unknown tools
fall back to the first string-typed `tool_input` value.

## Reporting

Once data is flowing, use the slash command surface inside Claude Code
(or Qwen Code):

```
/fluxmirror:about            # explainer + auto-discovered command list
/fluxmirror:today            # today's report
/fluxmirror:yesterday        # yesterday's report
/fluxmirror:week             # last 7 days
/fluxmirror:compare          # today vs yesterday
/fluxmirror:agent <name>     # single-agent filtered report
/fluxmirror:setup ...        # configure language/timezone
```

Reports filter by tool class using lists that cover both Claude
PascalCase (`Edit`/`Write`/`Read`/`Bash`) and Gemini/Qwen snake_case
(`edit_file`/`write_file`/`read_file`/`run_shell_command`).

## Extending to Claude Desktop (optional)

Claude Desktop uses stdio-based MCP servers (filesystem, Gmail, etc.) that
are not covered by this plugin. To audit Desktop's MCP traffic, install
the fluxmirror Rust proxy. Download the per-arch binary from the latest
GitHub release:

```bash
curl -L -o ~/fluxmirror-proxy \
  https://github.com/OpenFluxGate/fluxmirror/releases/latest/download/fluxmirror-proxy-darwin-arm64
chmod +x ~/fluxmirror-proxy
```

(Replace `darwin-arm64` with `darwin-x64`, `linux-x64`, `linux-arm64`, or
`windows-x64.exe` to match your machine.)

Then in Claude Desktop's configuration file
(`~/Library/Application Support/Claude/claude_desktop_config.json`),
wrap an existing MCP server with fluxmirror. Example — auditing the filesystem MCP server:

```json
{
  "mcpServers": {
    "fluxmirror-fs": {
      "command": "/Users/YOURNAME/fluxmirror-proxy",
      "args": [
        "--server-name", "fs",
        "--db", "/Users/YOURNAME/Library/Application Support/fluxmirror/events.db",
        "--capture-c2s", "/tmp/fm-c2s.raw",
        "--capture-s2c", "/tmp/fm-s2c.raw",
        "--",
        "/opt/homebrew/bin/npx", "-y", "@modelcontextprotocol/server-filesystem", "/path/to/watch"
      ]
    }
  }
}
```

Requires:
- Zero runtime dependencies (SQLite is statically linked into the binary)
- `~/Library/Application Support/fluxmirror/` directory created manually
  (or any other path you pass to `--db`)

Events land in the same SQLite database, queryable via:

```bash
sqlite3 "$HOME/Library/Application Support/fluxmirror/events.db" \
  "SELECT datetime(ts_ms/1000,'unixepoch','localtime') AS ts, method
   FROM events ORDER BY ts_ms DESC LIMIT 10"
```

## License

MIT
