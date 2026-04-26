# ADR-0002: Cross-platform wrapper layer (three shims, one router)

## Status

Accepted (Phase 1, 2026-04).

## Context

The agent CLIs that integrate with fluxmirror invoke a shell command on
every tool call — Claude via `PostToolUse` hooks, Gemini via
`AfterTool` hooks. That command must (a) ensure the right per-arch
`fluxmirror` binary is cached locally, (b) pipe the agent's tool-call
JSON to it on stdin, and (c) never fail in a way that breaks the
calling agent. The hosts run any of: macOS bash, Linux bash, WSL bash,
Git-Bash on Windows, native PowerShell, native cmd.exe. There is no
single shell language that runs natively on all of those.

## Decision

Ship three thin shims and one router:

- `wrappers/shim.sh` — bash; covers macOS / Linux / WSL / Git-Bash.
- `wrappers/shim.mjs` — Node ≥ 18; covers any host with Node,
  including a PowerShell-only Windows install.
- `wrappers/shim.cmd` — cmd.exe; falls back to PowerShell's
  `Invoke-WebRequest` on a Node-less Windows.
- `wrappers/router.sh` — tries `shim.sh`, then `shim.mjs`, used as the
  pre-init default in every plugin's `hooks.json` so first-fire works
  before the user has run `fluxmirror init`.

`fluxmirror init` probes the host (bash / curl / node / pwsh / OS /
shell kind via `MSYSTEM`, `WSL_DISTRO_NAME`, `PSModulePath`),
recommends one of the three engines, and atomically rewrites every
plugin's `hooks.json` to point at the chosen shim. `fluxmirror wrapper
set <kind>` re-runs the rewrite at any time.

## Consequences

- Each user only ever depends on a single runtime they already have
  installed for that OS.
- One shim per language stays small enough (~40-60 LOC each) to audit
  by reading. A single all-in-one bash with cygwin-coverage branches
  was rejected as harder to read and harder to test.
- The probe / set / hooks.json-rewrite UX gives the user a sensible
  default but lets them override at any time via
  `fluxmirror wrapper set`.
- We deliberately do NOT ship a Rust wrapper — it would be a
  chicken-and-egg with the binary it is supposed to download.
