# fluxmirror plugin for Claude Code

Audit every Claude Code tool call by logging it to a daily JSONL file.

## What it does

Registers a **PostToolUse** hook that fires after every tool invocation.
Each call is appended as a single JSON line to:

```
~/.claude/session-logs/YYYY-MM-DD.jsonl
```

Each line contains:

| Field     | Description                          |
|-----------|--------------------------------------|
| `ts`      | UTC timestamp (ISO 8601)             |
| `session` | Claude Code session ID               |
| `tool`    | Tool name (Read, Write, Bash, etc.)  |
| `detail`  | First 200 chars of the primary input |
| `cwd`     | Working directory at time of call    |

## Install

```bash
/plugin marketplace add OpenFluxGate/fluxmirror
/plugin install fluxmirror@fluxmirror
```

## Requirements

- `jq` must be on your PATH (pre-installed on macOS with Homebrew)

## Extending to Claude Desktop (optional)

Claude Desktop uses stdio-based MCP servers (filesystem, Gmail, etc.) that
are not covered by this plugin. To audit Desktop's MCP traffic, build and
install the fluxmirror Java proxy from the same repo:

```bash
git clone https://github.com/OpenFluxGate/fluxmirror.git
cd fluxmirror
./gradlew shadowJar
# Output: build/libs/fluxmirror-all.jar
```

Then in Claude Desktop's configuration file
(`~/Library/Application Support/Claude/claude_desktop_config.json`),
wrap an existing MCP server with fluxmirror. Example — auditing the filesystem MCP server:

```json
{
  "mcpServers": {
    "fluxmirror-fs": {
      "command": "/path/to/java",
      "args": [
        "-jar", "/absolute/path/to/fluxmirror-all.jar",
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
- Java 21 (tested with Zulu 21.0.10)
- `~/Library/Application Support/fluxmirror/` directory created manually

Events land in a SQLite database, queryable via:

```bash
sqlite3 "$HOME/Library/Application Support/fluxmirror/events.db" \
  "SELECT datetime(ts_ms/1000,'unixepoch','localtime') AS ts, method
   FROM events ORDER BY ts_ms DESC LIMIT 10"
```

## License

MIT
