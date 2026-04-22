# fluxmirror extension for Gemini CLI

Audit every Gemini CLI tool call by logging it to a daily JSONL file.

## What it does

Registers an **AfterTool** hook that fires after every tool invocation.
Each call is appended as a single JSON line to:

```
~/.gemini/session-logs/YYYY-MM-DD.jsonl
```

Each line contains:

| Field     | Description                          |
|-----------|--------------------------------------|
| `ts`      | UTC timestamp (ISO 8601)             |
| `session` | Session ID                           |
| `tool`    | Tool name                            |
| `detail`  | First 200 chars of the primary input |
| `cwd`     | Working directory at time of call    |

## Install

From a local clone:

```bash
gemini extensions install ./gemini-extension
```

Remote install (untested — may require the manifest at repo root):

```bash
gemini extensions install https://github.com/OpenFluxGate/fluxmirror
```

## Requirements

- `jq` must be on your PATH

## JSONL format compatibility

The output format is identical to the Claude Code plugin
(`plugins/fluxmirror/`). Logs are written to `~/.gemini/session-logs/`
instead of `~/.claude/session-logs/` so each agent's history stays
separate. Both can be merged or queried together since they share the
same JSON schema.

## License

MIT
