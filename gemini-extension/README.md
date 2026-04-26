# fluxmirror extension for Gemini CLI

Audit every Gemini CLI tool call by logging it to a daily JSONL file
**and** the same SQLite database used by the Claude Code / Qwen Code
plugin and the `fluxmirror proxy` MCP relay — so all your agents share
one queryable history.

## What it does

Registers an **AfterTool** hook that fires after every tool
invocation. The wrapper layer execs `fluxmirror hook --kind gemini`
(the single Phase 1 binary; auto-downloaded on first invocation),
which:

1. Appends one JSON line to `~/.gemini/session-logs/YYYY-MM-DD.jsonl`
2. Writes one parameter-bound row into FluxMirror SQLite at the
   per-OS default path (macOS: `~/Library/Application
   Support/fluxmirror/events.db`; per-OS defaults documented in the
   root README), table `agent_events`, with `agent='gemini-cli'`.

JSONL line fields:

| Field     | Description                          |
|-----------|--------------------------------------|
| `ts`      | UTC timestamp (ISO 8601)             |
| `session` | Session ID                           |
| `tool`    | Tool name (Gemini snake_case: `read_file`, `run_shell_command`, …) |
| `detail`  | First 200 chars of the primary input |
| `cwd`     | Working directory at time of call    |

The Gemini extension uses `!{ ... }` shell interpolation in its
`commands/fluxmirror/*.toml` files; reports are produced by a small
`scripts/report-data.sh` shell script that wraps the corresponding
`fluxmirror` subcommands.

## Install

From the dedicated `gemini-extension-pkg` branch (auto-published by
`release.yml` on every tag, contains this directory's files plus the
shared `wrappers/` at the repo root so Gemini's installer can find
`gemini-extension.json`):

```bash
gemini extensions install https://github.com/OpenFluxGate/fluxmirror \
  --ref gemini-extension-pkg --consent
```

(Gemini's installer requires a full `https://` URL — the `owner/repo`
shorthand is not accepted.)

From a local clone (no network needed):

```bash
git clone https://github.com/OpenFluxGate/fluxmirror.git
gemini extensions install ./fluxmirror/gemini-extension --consent
```

Pinning a specific release: pass `--ref vX.Y.Z` to install at that tag.

## What it ships

| File | Role |
|---|---|
| `gemini-extension.json` | Extension manifest |
| `hooks/hooks.json` | AfterTool → `wrappers/router.sh` (until init picks an explicit shim) |
| `bin/` | Cache directory for the per-arch `fluxmirror` binary (auto-populated on first hook fire) |
| `commands/fluxmirror/*.toml` | `/fluxmirror:*` slash command surface |
| `scripts/report-data.sh` | Thin shell wrapper used by the slash commands |

## Wrapper selection

On first install, `hooks.json` points at `wrappers/router.sh` so the
very first hook fire works regardless of host. Run

```bash
fluxmirror wrapper probe        # show which engines are viable
fluxmirror wrapper set <kind>   # rewrite hooks.json to one shim
```

to lock the wrapper to a single engine (`bash` / `node` / `cmd`).
`fluxmirror init` does both probe and set in one go. See
[ADR-0002](../docs/adr/0002-cross-platform-wrapper.md) for the
rationale behind three shims.

## Requirements

For the wrapper layer, **one** of: `bash + curl` (macOS / Linux / WSL
/ Git-Bash), `node` ≥ 18 (any host with Node), or `cmd.exe` +
PowerShell (native Windows without Node). Network access on the first
hook fire — the wrapper downloads the per-arch `fluxmirror` binary
(~2.2 MB) from the latest GitHub release. Subsequent calls skip the
download.

## Configuration

Layered (CLI flags > env > project `.fluxmirror.toml` > user config >
defaults). Useful environment variables:

| Variable               | Effect                                              |
|------------------------|------------------------------------------------------|
| `FLUXMIRROR_DB`        | Override DB path                                    |
| `FLUXMIRROR_SKIP_SELF` | If `1`, combined with `FLUXMIRROR_SELF_REPO`, skips events that look like fluxmirror querying its own DB from inside its own repo. |
| `FLUXMIRROR_SELF_REPO` | Absolute path to the fluxmirror repo for the filter above. |

Hook-side errors are appended to `~/.fluxmirror/hook-errors.log`. The
log auto-rotates at 5 MiB, keeping one backup as `hook-errors.log.1`.

The hook recognizes ~20 tool names across Claude Code (PascalCase) and
Gemini / Qwen Code (snake_case), and extracts the most informative
`detail` field per tool (e.g., `command` for shell, the file path for
read / write / edit, the URL for web fetch). Unknown tools fall back
to the first string-typed `tool_input` value.

## Tool naming convention

Gemini CLI emits tool names in **snake_case** (`read_file`,
`run_shell_command`, `write_file`, `replace`, …) — different from
Claude Code's PascalCase (`Read`, `Bash`, `Write`, `Edit`,
`MultiEdit`). The fluxmirror reports normalize across both naming
styles, so a single report covers all agents uniformly. The DB stores
the raw tool name as emitted, so original fidelity is preserved for
ad-hoc queries.

## Schema verification

The hook input fields (`session_id`, `cwd`, `tool_name`, `tool_input`,
`hook_event_name`) and the `AfterTool` event name were verified
against the official Gemini CLI hooks reference at
<https://geminicli.com/docs/hooks/reference/>.

## Troubleshooting

| Symptom | Resolution |
|---|---|
| Slash command prints "no events" | Trigger any tool call, then re-run. Confirm `fluxmirror doctor` shows `database ok`. A fresh `fluxmirror init` also seeds one synthetic `agent='setup'` row so the very first `/fluxmirror:today` is non-empty; remove it any time with `fluxmirror sqlite "DELETE FROM agent_events WHERE agent='setup'"`. |
| `Configuration file not found` on install | You omitted `--ref gemini-extension-pkg`. Add it. |
| `Install source not found.` | Use the full `https://github.com/OpenFluxGate/fluxmirror` URL — `owner/repo` shorthand is not accepted. |
| `Extension "fluxmirror" is already installed.` | `gemini extensions uninstall fluxmirror`, then re-install. |
| Hook never fires | Check `~/.fluxmirror/hook-errors.log`; restart `gemini`; verify `hooks.json` exists under the extension dir. |
| Binary download fails on first fire | The wrapper exits 0 silently to never break the calling agent. Check network and trigger another tool call. |
| `Gemini CLI is not running in a trusted directory.` | Set `GEMINI_CLI_TRUST_WORKSPACE=true` or pass `--skip-trust` (only when invoking `gemini`, not `gemini extensions install`). |
| `Extension "fluxmirror" already loaded` | An earlier install left a `*.backup.*` directory next to the live one. Remove it: `rm -rf ~/.gemini/extensions/fluxmirror.backup.*`. |
| Qwen install completes but `/fluxmirror:*` commands never surface | Confirm `~/.qwen/extensions/fluxmirror/qwen-extension.json` exists. If it is missing, the package was built before v0.6.0 — re-install from the latest release. |
| Gemini shows flat `/today` instead of `/fluxmirror:today` | The `/fluxmirror:` namespace prefix comes from the `commands/fluxmirror/` subdirectory. If your install has flat `commands/*.toml` files, the package shipped a flatten regression — re-install from the latest release. |
| Gemini ran `/fluxmirror:init` non-interactively (skipped questions) | The model raced past the question gate. The post-v0.6.0 `init.toml` keeps the "STEP 1 — ASK THE USER" block in front of any shell fence so the questions are answered first — update the extension and retry. |

## Isolation across agents

Each agent's JSONL output goes to a different directory
(`~/.claude/`, `~/.gemini/`, `~/.qwen/`). Run
`scripts/verify-isolation.sh` from the repo root to confirm session
IDs do not leak across them.

Qwen Code does not need this extension — it installs the Claude Code
plugin directly via `qwen extensions install
OpenFluxGate/fluxmirror:fluxmirror`. The shared hook auto-detects Qwen
at runtime and labels rows `agent='qwen-code'`.

## License

MIT
