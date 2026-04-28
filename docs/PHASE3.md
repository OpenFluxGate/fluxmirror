# Phase 3 Plan — fluxmirror

> Branch: `feature/phase3/base` (integration) + `feature/phase3/<milestone>` per slice
> Goal: take fluxmirror from "I run a daily report" to
> "**this is the tab I open when I want to think about my AI coding.**"
> Live state (untracked, machine-local) lives at
> `.omc/autopilot/{spec,plan,progress}.md`. This file is the
> branch-visible summary.

## Why Phase 3

Phase 1 captured. Phase 2 reported. The data sits in SQLite, the
weekly HTML card looks decent, but there's still no surface where you
naturally *spend time* with your own activity. You run a slash
command, read a paragraph, close it.

Phase 3 turns the corner: the dataset gets a place to live in your
browser. The single load-bearing concept is **provenance per file** —
every file in your repo carries a queryable AI contact history. From
that primitive you get: a daily dashboard, a per-file timeline, a
time-machine replay of any day, and auto-named work sessions.

Phase 3 also re-runs the external beta that Phase 2 deferred (no
candidate at the time). To make that safe, Phase 3 ships a redaction
layer for any output surface and a real self-update path so the
person on the other end isn't pinned to a stale binary.

## What ships

| # | Output | Phase 3 status |
|---|---|---|
| Local web dashboard at `localhost:7090` | new | studio binary, opt-in |
| `/file/<path>` provenance view | new | every file's AI history |
| `/replay/<date>` time-machine | new | scrubbable timeline of a day |
| Auto-named work sessions | new | heuristic; LLM optional |
| Estimated API cost overlay | new | closes the README 🟡 |
| Redaction layer (.env, AWS, GitHub, ...) | new | prerequisite for sharing |
| `fluxmirror upgrade` self-update | new | atomic, sha-verified |
| Real `.fluxmirror.toml` parser | new | replaces the long-standing stub |

The capture binary (`fluxmirror`) keeps its current shape and size.
Studio is a separate workspace bin, separately installed, separately
invoked. SQLite is the only thing they share.

## Architecture

```
                            ┌────────────────────────────┐
                            │ fluxmirror-studio (NEW bin)│
                            │ axum @ 127.0.0.1:7090      │
                            │ embedded React/Vite bundle │
                            └────────────┬───────────────┘
                                         │ read-only
                                         ▼
                       ┌──────────────────────────────────┐
                       │  events.db (single writer)       │
                       └──────────────────────────────────┘
                                         ▲
                                         │
  Claude / Qwen / Gemini ─── hook ──▶ fluxmirror hook
  Claude Desktop ◀─ stdio ─▶ fluxmirror proxy
```

Frontend is built at developer time with pnpm + Vite. The resulting
`dist/` folder is embedded into the Rust binary via `include_dir!()`.
End users (the friend installing the release tarball) need zero JS
toolchain.

## Tech stack

- Frontend: Vite + React 18 + TypeScript + Tailwind v4 + shadcn/ui
- Backend: axum 0.7 in `crates/fluxmirror-studio/`
- Charts: Tremor or recharts (decision at M2 entry); heatmap as inline SVG
- Cross-cutting: `toml` for config, `regex` for redaction, `ureq` for
  self-update

The capture / proxy / CLI binary picks up only `toml`, `regex`,
`ureq`. Web stack is fully isolated.

## Milestones

| ID | Milestone | Estimate | Branch |
|---|---|---|---|
| M1 | Studio crate scaffold + Vite frontend | 5 d | `feature/phase3/studio-scaffold` |
| M2 | `/today`, `/week`, `/` home | 1.5 wk | `feature/phase3/studio-today` |
| M3 | `/file/<path>` provenance | 1 wk | `feature/phase3/studio-provenance` |
| M4 | `/replay/<date>` time-machine | 1.5 wk | `feature/phase3/studio-replay` |
| M5 | Auto-named sessions (heuristic + opt LLM) | 1 wk + 1 wk opt | `feature/phase3/sessions` |
| M6 | Cost overlay | 1 wk | `feature/phase3/cost` |
| M7 | Redaction layer | 1 wk | `feature/phase3/redact` |
| M8 | `fluxmirror upgrade` self-update | 3 d | `feature/phase3/upgrade` |
| M9 | Real `.fluxmirror.toml` parser | 2 d | `feature/phase3/toml` |
| M10 | External beta | 1 wk + buffer | `feature/phase3/beta` |

Total: 9–10 weeks of focused work (LLM optional adds 1 week).

## Parallelism

After M1 lands on `feature/phase3/base`, three tracks can run in
parallel across cmux panes / worktrees:

- **Track A (web stack):** M2 → M3 → M4
- **Track B (data layer):** M2 → M5 → M6
- **Track C (underlay):** M7 ‖ M8 ‖ M9 (independent)

Each per-milestone branch lands as a PR into `feature/phase3/base`.
The final integration PR `feature/phase3/base → main` plus a `v0.6.0`
tag closes the phase.

## UI tone

Light theme by default. References: Anthropic docs, Stripe docs,
Linear (light), Notion. Warm off-white surfaces, hairline borders,
restrained slate-blue accent. Information density preserved over
decoration. No dark-mode toggle in v0.6.0.

## Acceptance gate (Phase 3 → Phase 4)

- F-1 through F-10 (see `.omc/autopilot/spec.md`) demonstrably true
- Studio binary ≤ 18 MB; capture binary ≤ 4 MB
- CI install-sim job 30 consecutive green runs
- Studio integration tests cover every route on at least 2 fixture DBs
- 1+ external tester confirmed continued use after 7 days
- Phase 1 + 2 acceptance criteria still met (zero regressions)
- Redaction blocks ≥ 4 built-in secret pattern classes across all output
- Zero open P0 issues

## Out of scope (deferred)

- FluxGate integration — Phase 5
- Anomaly rules engine — Phase 4
- Cross-agent A/B benchmark — revisit after beta
- Studio remote / multi-user / authentication — Phase 4
- Vector embeddings / semantic search — Phase 5
- VS Code / Cursor extension — Phase 5
- HTTP / SSE MCP transport — Phase 5
- Windows native cmd.exe shim hardening — re-evaluate after beta
- Dark mode toggle (light only for v0.6.0)
