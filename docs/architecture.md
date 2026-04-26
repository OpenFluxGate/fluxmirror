# FluxMirror Architecture

## Overview

FluxMirror is a single self-contained `fluxmirror` binary, a thin
cross-shell wrapper layer that fronts it, and a local SQLite store that
collects every tool call and (optionally) every MCP JSON-RPC message
from the agent CLIs that integrate with it. The binary uses a
kubectl-style subcommand surface (`fluxmirror hook`, `fluxmirror proxy`,
`fluxmirror today`, `fluxmirror doctor`, …) and replaces the previous
two-binary layout (`fluxmirror-hook` + `fluxmirror-proxy`). Rationale
in [ADR-0001](./adr/0001-single-binary.md).

## Layered model

The system has eight layers. Lower layers do not depend on higher ones.

```
L8  Docs                   docs/architecture.md, docs/adr/000{1..4}.md, READMEs
L7  CI / Release           .github/workflows/{test,release,rust-release}.yml
L6  Reports                window / histogram / daily-totals / per-day-files / sqlite
                           + /fluxmirror:* slash commands
L5  Proxy                  fluxmirror-proxy: stdio MCP relay (NDJSON framer + bridge)
L4  Storage                fluxmirror-store: EventStore trait + SqliteStore + migrations
L3  Domain                 fluxmirror-core: Event types, normalize, Config, paths, tz
L2  Binary entry           fluxmirror-cli: clap-derive subcommand dispatch
L1  Wrapper layer          wrappers/{shim.sh, shim.mjs, shim.cmd, router.sh}
```

Data flow at runtime:

```
                                                         /fluxmirror:* slash command
                                                                       │
                                                                       ▼
agent CLI (Claude / Qwen / Gemini)                            fluxmirror window
        │                                                     fluxmirror histogram
        │ PostToolUse / AfterTool                             fluxmirror daily-totals
        ▼                                                     fluxmirror sqlite
wrappers/router.sh ──▶ shim.sh / shim.mjs / shim.cmd                   │
                                  │                                    │
                                  ▼                                    ▼
                       fluxmirror hook --kind <kind>           SqliteStore (read)
                                  │                                    │
                                  ▼                                    ▼
                       SqliteStore (write)  ──────▶  events.db ──▶ stdout (TSV)
                                                          ▲                │
                                                          │                ▼
Claude Desktop ◀── stdio ──▶ fluxmirror proxy ─── NDJSON framer        human report
                                                          │
                                                          ▼
                                              MCP server (child process)
```

`agent_events` rows come in via L1→L2→L3→L4. `events` rows come in via
L5→L4. Both tables live in the same `events.db` SQLite file. The L6
report subcommands and slash commands read both.

## Crate map

| Crate | Role | Key types / functions |
|---|---|---|
| `fluxmirror-core` | Pure domain — no IO besides config files | `AgentEvent`, `ProxyEvent`, `AgentId`, `ToolKind`, `ToolClass`, `normalize()`, `extract_detail()`, `Config::load()`, `paths::default_db_path()`, `tz::infer_default_tz()` |
| `fluxmirror-store` | Storage abstraction + concrete SQLite impl | `trait EventStore`, `SqliteStore::open()`, `migrate()`, `write_agent_event()`, `write_proxy_event()` |
| `fluxmirror-proxy` | Long-running stdio MCP relay (lib) | NDJSON `framer`, child supervision, `c2s`/`s2c` bridge, proxy-side `EventStore` |
| `fluxmirror-cli` | Single `[[bin]]` named `fluxmirror`; clap-derive dispatcher | `cmd::{hook, proxy, init, config, wrapper, doctor, db_path, window, histogram, daily_totals, per_day_files, sqlite}` |

Only `fluxmirror-cli` produces an executable. The other three are
libraries, depended on by `fluxmirror-cli` and (where appropriate) by
each other (`store` depends on `core`; `proxy` depends on `core`; `cli`
depends on all three).

## Wrapper engine selection flow

`fluxmirror init` probes the host environment, picks one of the three
wrapper engines, and rewrites every plugin's `hooks.json` to point at
the chosen shim. The probe is deterministic and runs in this order:

1. **bash + curl present** → `wrappers/shim.sh` (smallest install
   surface; covers macOS, Linux, WSL, Git-Bash on Windows).
2. **node ≥ 18 present** → `wrappers/shim.mjs` (covers any host with
   Node, including PowerShell-only Windows).
3. **Windows cmd shell only** → `wrappers/shim.cmd` (uses
   `Invoke-WebRequest` via PowerShell when Node is absent).

If multiple engines are viable, init shows a one-line table and
defaults to the recommended pick. With `--non-interactive` the default
wins. With `--advanced` the user is also asked about retention,
self-noise filters, and per-agent toggles. See
[ADR-0002](./adr/0002-cross-platform-wrapper.md) for why three shims
and not one universal one.

`fluxmirror wrapper set <kind>` re-runs the rewrite at any time. Both
plugin manifests (`plugins/fluxmirror/hooks/hooks.json` and
`gemini-extension/hooks/hooks.json`) are rewritten atomically (write to
`*.tmp`, fsync, rename). The pre-init default is `wrappers/router.sh`
which simply tries each shim in priority order until one succeeds — so
the very first hook fire works even before the user has run `init`.

## Schema versioning

Schema state lives in a small `schema_meta(version INTEGER PRIMARY KEY,
applied_at TEXT)` table. `SqliteStore::open()` calls `migrate()`
automatically on every open. The migration is purely additive:

- Brand-new DB → `CREATE TABLE` with the full v1 schema for both
  `agent_events` and `events`, plus indexes; insert
  `schema_meta(1, now)`.
- Pre-v1 (legacy) DB → for each missing column on `agent_events`
  (`tool_canonical`, `tool_class`, `host`, `user`, `schema_version`),
  run a single `ALTER TABLE … ADD COLUMN`. Indexes ensured. Insert
  `schema_meta(1, now)`. Existing rows keep their `NULL`s; the
  `schema_version` column has a `DEFAULT 1` so they read back as 1.
- Already-v1 DB → no-op (the `INSERT OR IGNORE` is the only write).

All ALTERs and the `schema_meta` insert run inside one transaction, so
a crash mid-migration cannot leave the DB in a half-migrated state.
STEP 3 of the Phase 1 plan moved the writes out of `cmd/hook.rs` and
into `SqliteStore`, so `cmd/hook.rs` no longer carries any `CREATE
TABLE IF NOT EXISTS` of its own. Rationale in
[ADR-0003](./adr/0003-storage-trait.md).

## Phase 2 / 3 / 4 / 5 roadmap

**Phase 2 — daemon mode and remote forwarding.** A new `fluxmirror
serve` subcommand will keep one long-running process per host, accept
hook events over a Unix socket (Windows: named pipe), and optionally
forward to a remote sink. This is also the integration point for
**FluxGate** — per-agent call rate control without making FluxGate a
runtime dependency of the binary today.

**Phase 3 — policy engine and anomaly rules.** The *Anomaly alerts*
output line item from the project's mission statement is still owed.
Phase 3 adds a small rule language (path globs + tool-class predicates)
plus an evaluator that runs at hook time and can warn or block. The
rule files live under `~/.fluxmirror/policies/`.

**Phase 4 — pluggable EventStore implementations.** The `EventStore`
trait shipped in Phase 1 exists so we can add `PostgresStore` and
`ClickhouseStore` later for users who want centralized aggregation
across machines. SQLite stays the default and the only store the
binary requires.

**Phase 5 — read-only web dashboard.** Lives in a separate repo so the
binary stays small. Connects to the same `events.db` (or, in Phase 4,
to a Postgres / ClickHouse mirror). The binary never grows an HTTP
server.

## Out of scope (Phase 1)

Mirrors `spec.md`:

- Daemon mode (`fluxmirror serve`) — Phase 2
- Remote forwarding — Phase 2
- Postgres / ClickHouse stores — Phase 4
- Policy engine / anomaly rules — Phase 3
- Web dashboard — Phase 5
- HTTP+SSE MCP transport — out (existing project constraint)
- Encryption / multi-device sync — out (existing project constraint)
- Redaction of sensitive data — out (existing project constraint)
