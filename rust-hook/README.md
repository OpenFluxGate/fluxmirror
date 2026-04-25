# fluxmirror-hook (Rust)

Single-binary replacement for the bash + jq + python hook chain.

## Why

The original hook required: `bash` + `jq` + `sqlite3` CLI + `python3` (4 deps).
This Rust binary needs: nothing at runtime (`rusqlite` ships SQLite statically).

Side benefits:
- ~10× faster per invocation (~5 ms vs ~50 ms)
- Cross-OS native (macOS / Linux / Windows — no WSL or shell needed)
- Single source of truth for hook logic (no DRY mirror dance)
- Memory-safe by construction

## Build

```bash
cd rust-hook
cargo build --release
# → target/release/fluxmirror-hook  (~1.3 MB on aarch64-apple-darwin)
```

## Usage

```bash
# Claude Code (default — auto-detects Qwen via env vars)
fluxmirror-hook < tool-call.json
fluxmirror-hook --kind claude < tool-call.json

# Gemini CLI (always labels as gemini-cli)
fluxmirror-hook --kind gemini < tool-call.json
```

The hook reads a tool-call JSON payload on stdin and writes:
1. One JSON line to `~/<agent>/session-logs/YYYY-MM-DD.jsonl`
2. One parameter-bound row into the FluxMirror SQLite DB at
   `~/Library/Application Support/fluxmirror/events.db`

## Environment variables

| Variable | Effect |
|---|---|
| `FLUXMIRROR_DB` | Override DB path |
| `FLUXMIRROR_SKIP_SELF` | If `1` + `FLUXMIRROR_SELF_REPO` set, skip self-noise |
| `FLUXMIRROR_SELF_REPO` | Absolute path to fluxmirror repo |
| `QWEN_CODE_NO_RELAUNCH` | Set by Qwen CLI — triggers `qwen-code` label |
| `QWEN_PROJECT_DIR` | Set by Qwen CLI — triggers `qwen-code` label |

## Errors

Any IO error is logged to `~/.fluxmirror/hook-errors.log` (auto-rotated
at 5 MiB, keeps one backup `hook-errors.log.1`). Process exit is always
0 — never breaks the calling agent over telemetry failure.

## Tests

```bash
cargo test --release          # 14 unit tests (extraction, time format)
../scripts/test-rust-hook.sh  # 20 black-box parity tests
```

## Wiring into hooks.json (deployment)

Each install package needs its own per-OS+arch binary copy:

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "${CLAUDE_PLUGIN_ROOT}/bin/fluxmirror-hook --kind claude"
          }
        ]
      }
    ]
  }
}
```

Cross-arch release builds will be added to `.github/workflows/release.yml`
in a follow-up commit, producing `bin/fluxmirror-hook-{darwin-arm64,
darwin-x64, linux-x64, linux-arm64, windows-x64.exe}` and a thin shim
that picks the right one at install time.

## Status

- Logic complete and parity-verified vs the bash hook
- Local arch (aarch64-apple-darwin) build only
- Cross-arch CI matrix and hooks.json switchover are next
