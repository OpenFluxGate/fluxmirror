# fluxmirror plugin for Claude Code (and Qwen Code)

Audit every Claude Code (or Qwen Code) tool call by logging it to a
daily JSONL file **and** a shared SQLite database used by the
`/fluxmirror:*` reporting commands.

## What it does

Registers a **PostToolUse** hook that fires after every tool
invocation. For each call, the wrapper layer execs `fluxmirror hook
--kind claude` (the single Phase 1 binary; auto-downloaded on first
invocation), which:

1. Appends one JSON line to `~/<agent>/session-logs/YYYY-MM-DD.jsonl`
2. Writes one parameter-bound row into the FluxMirror SQLite database
   (default `~/Library/Application Support/fluxmirror/events.db` on
   macOS; per-OS defaults documented in the root README), table
   `agent_events`, with the agent column set to either `claude-code`
   or `qwen-code` depending on which CLI is running.

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
accordingly.

## What it ships

| File | Role |
|---|---|
| `hooks/hooks.json` | PostToolUse → `wrappers/router.sh` (until init picks an explicit shim) |
| `bin/` | Cache directory for the per-arch `fluxmirror` binary (auto-populated on first hook fire) |
| `commands/*.md` | `/fluxmirror:*` slash command surface |

The `hooks.json` and the wrapper-engine choice are managed by the
`fluxmirror init` and `fluxmirror wrapper set <kind>` subcommands —
see the root README and [ADR-0002](../../docs/adr/0002-cross-platform-wrapper.md).

## Wrapper selection

On first install, `hooks.json` points at `wrappers/router.sh` so the
very first hook fire works regardless of host. Run

```bash
fluxmirror wrapper probe        # show which engines are viable
fluxmirror wrapper set <kind>   # rewrite hooks.json to one shim
```

to lock the wrapper to a single engine (`bash` / `node` / `cmd`).
`fluxmirror init` does both probe and set in one go on first run.

## Requirements

For the wrapper layer, **one** of the following per host: `bash + curl`
(macOS / Linux / WSL / Git-Bash), `node` ≥ 18 (any host with Node), or
`cmd.exe` + PowerShell (native Windows without Node). Network access
on the first hook fire — the wrapper downloads the per-arch
`fluxmirror` binary (~2.2 MB) from the latest GitHub release into
`<plugin>/bin/` and execs it. Subsequent calls skip the download.

## Configuration

Layered (CLI flags > env > project `.fluxmirror.toml` > user config >
defaults). Useful environment variables:

| Variable               | Effect                                              |
|------------------------|------------------------------------------------------|
| `FLUXMIRROR_DB`        | Override DB path                                    |
| `FLUXMIRROR_SKIP_SELF` | If `1`, combined with `FLUXMIRROR_SELF_REPO`, skips events that look like fluxmirror querying its own DB from inside its own repo (useful when self-developing fluxmirror). |
| `FLUXMIRROR_SELF_REPO` | Absolute path to the fluxmirror repo for the filter above. Anchored prefix match. |

Hook-side errors (e.g., DB locked) are appended to
`~/.fluxmirror/hook-errors.log`. The log auto-rotates at 5 MiB,
keeping one backup as `hook-errors.log.1`.

The hook recognizes ~20 tool names across Claude Code (PascalCase like
`Read`, `Bash`, `Edit`, `MultiEdit`, `WebFetch`, `WebSearch`, `Task`,
`TodoWrite`, `Glob`, `Grep`, `NotebookEdit`, `BashOutput`, `KillBash`,
`ExitPlanMode`) and Gemini / Qwen Code (snake_case like `read_file`,
`run_shell_command`, `write_file`, `replace`, `glob`,
`search_file_content`, `web_fetch`, `web_search`, `save_memory`,
`todo_write`, `read_many_files`, `kill_shell`) and extracts the most
informative `detail` field per tool. Unknown tools fall back to the
first string-typed `tool_input` value.

## Reporting

```
/fluxmirror:about            explainer + auto-discovered command list
/fluxmirror:today            today's report
/fluxmirror:yesterday        yesterday
/fluxmirror:week             last 7 days, daily breakdown
/fluxmirror:compare          today vs yesterday side-by-side
/fluxmirror:agent <name>     single-agent filtered report
/fluxmirror:agents           per-agent 7-day totals + dominant tools
/fluxmirror:setup            configure language and timezone
/fluxmirror:config           show / get / set / explain config
/fluxmirror:doctor           5-component health table
```

Reports normalize tool names across Claude PascalCase and Gemini /
Qwen snake_case, so a single report covers all agents uniformly.

## Troubleshooting

| Symptom | Resolution |
|---|---|
| Slash command prints "no events" | Trigger any tool call, then re-run. Confirm `fluxmirror doctor` shows `database ok`. |
| `fluxmirror: command not found` from a slash command | Re-run `fluxmirror init`; it ensures the binary is on `PATH` for the slash-command shell. |
| Wrong wrapper engine picked | `fluxmirror wrapper set <bash\|node\|cmd>` to override; init's probe table shows what is viable. |
| Hook never fires | Check `~/.fluxmirror/hook-errors.log`; restart the agent CLI; verify `hooks.json` exists under `<plugin>/hooks/`. |
| Binary download fails on first fire | The wrapper exits 0 silently to never break the calling agent. Check network / GitHub availability and trigger another tool call. |

## Extending to Claude Desktop (optional)

Claude Desktop uses stdio-based MCP servers (filesystem, Gmail, etc.)
that are not covered by this plugin. To audit Desktop's MCP traffic,
install the same `fluxmirror` binary and use its `proxy` subcommand.
Download the per-arch binary from the latest GitHub release:

```bash
curl -L -o ~/fluxmirror \
  https://github.com/OpenFluxGate/fluxmirror/releases/latest/download/fluxmirror-darwin-arm64
chmod +x ~/fluxmirror
```

(Replace `darwin-arm64` with `darwin-x64`, `linux-x64`, `linux-arm64`,
or `windows-x64.exe` to match your machine.)

Then in Claude Desktop's configuration file
(`~/Library/Application Support/Claude/claude_desktop_config.json`),
wrap an existing MCP server with fluxmirror. Example — auditing the
filesystem MCP server:

```json
{
  "mcpServers": {
    "fluxmirror-fs": {
      "command": "/Users/YOURNAME/fluxmirror",
      "args": [
        "proxy",
        "--server-name", "fs",
        "--db", "/Users/YOURNAME/Library/Application Support/fluxmirror/events.db",
        "--",
        "/opt/homebrew/bin/npx", "-y", "@modelcontextprotocol/server-filesystem", "/path/to/watch"
      ]
    }
  }
}
```

Events land in the same SQLite database, queryable via:

```bash
fluxmirror sqlite --db "$(fluxmirror db-path)" \
  "SELECT datetime(ts_ms/1000,'unixepoch','localtime') AS ts, method
   FROM events ORDER BY ts_ms DESC LIMIT 10"
```

## License

MIT
