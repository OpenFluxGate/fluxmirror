# Phase 4 Plan — fluxmirror

> Branch: `feature/phase4/base` (integration) + `feature/phase4/<slug>` per slice
> Goal: move from data ledger to **outcome ledger**. The studio
> opens to LLM-written narrative, auto-named session intent,
> cross-day project arcs, and surfaced anomalies — all push,
> never pull.
> Live state (untracked, machine-local) lives at
> `.omc/autopilot/{spec,plan,progress}.md`. This file is the
> branch-visible summary.

## Why Phase 4

Phase 1 captured. Phase 2 reported. Phase 3 visualised and gave
every file an AI provenance trail. The data is rich, but every
surface still talks at the level of **what was run**:

> 384 calls · 33 Bash · edit-to-read 0.25 · 3 sessions

Phase 4 turns the corner: the studio answers **what got done**.

> Phase 3 closure work continued — shipped M6 (cost overlay,
> +1107 LOC, $0.0043) and resolved 1 conflict (M5 vs M6 dto.rs).
> Pace 50% faster than yesterday.

Same data. New synthesis layer. Push only — Phase 4 ships zero
input boxes.

## What ships

| # | Output | Phase 4 status |
|---|---|---|
| Daily narrative on `/today` + CLI text + HTML card | new | LLM-written, cached, heuristic fallback |
| Session intent subtitle | new | every Phase 3 session gets a one-sentence intent |
| Project arc + `/projects` route | new | cross-day clusters, LLM-named, with arc paragraph |
| Anomaly stories on `/today` | new | heuristic detect → LLM explain |
| AI service layer | new | provider abstraction (Anthropic + Ollama-stub), budget cap, SQLite cache, prompt registry, outbound redaction |
| Per-file insight card | v0.7.1 | deferred |
| macOS notification at end of day | v0.7.1 | deferred |
| Ollama as default provider | v0.7.1 | deferred — Anthropic flow first |
| Q&A / "Ask my history" | dropped | input UI conflicts with push-only principle |

The capture binary (`fluxmirror`) keeps its current shape and
size. AI deps land in a new `fluxmirror-ai` crate consumed only by
the studio and the CLI report renderers.

## Architecture

```
┌────────────────────────────────────────┐
│  fluxmirror-studio                     │
│   /api/today, /api/week, /api/sessions │
│   /api/anomalies (NEW), /api/projects  │
│   (NEW)                                │
└────────────────┬───────────────────────┘
                 │
                 ▼
   ┌──────────────────────────────────┐
   │ fluxmirror-ai (NEW crate)        │
   │  provider {anthropic | ollama}   │
   │  cache (SQLite ai_cache, 7d TTL) │
   │  budget (daily USD ceiling)      │
   │  prompts/{daily,session,project, │
   │           anomaly}.txt           │
   │  redact_outbound (M7 + path scrub)│
   └──────────────────────────────────┘
                 ▲
                 │
   fluxmirror-cli (text + HTML reports use the
   same daily/anomaly synthesis pipeline)
```

## Tech stack

- New crate **`fluxmirror-ai`** in `crates/fluxmirror-ai/`
- Frontend gains `studio-web/src/routes/Projects.tsx` and
  narrative components on `Home`, `Today`, `Sessions`, `Session`
- New deps confined to `fluxmirror-ai`: `ureq` (already pulled in
  by Phase 3 M8), optional `sha2` for stable cache keys (decided
  at M-A1 entry)
- New SQLite table `ai_cache(key, response, created_at, cost_usd)`
  via additive migration in `fluxmirror-store`

## Milestones

| ID | Milestone | Estimate | Branch |
|---|---|---|---|
| M-A1 | AI service layer (provider, budget, cache, prompts, redact_outbound) | 1.5 wk | `feature/phase4/ai` |
| M-A2 | Daily narrative on `/today` + CLI text + HTML card | 1 wk | `feature/phase4/daily-narrative` |
| M-A3 | Session intent (LLM-classified subtitle, cached) | 1 wk | `feature/phase4/session-intent` |
| M-A4 | Project arc clustering + LLM name/summary + `/projects` | 1.5 wk | `feature/phase4/project-arc` |
| M-A6 | Anomaly stories (heuristic detect + LLM explain) | 1 wk | `feature/phase4/anomaly-stories` |

Total: ~5 weeks of focused work. Operating cost: ~$2-10/month
(Haiku default; Sonnet only for project arc).

## Parallelism

M-A1 lands solo first — every other milestone consumes the AI
service layer. After it merges into `feature/phase4/base`, M-A2 /
M-A3 / M-A4 / M-A6 dispatch in parallel across worktrees + tmux
yolo agents:

```
M-A1 (solo) ──► M-A2 ‖ M-A3 ‖ M-A4 ‖ M-A6
```

Each per-milestone branch lands as a PR into
`feature/phase4/base`. Final `feature/phase4/base → main` ff merge
plus a `v0.7.0` tag closes Phase 4.

## Privacy boundary

Outbound LLM payloads contain only:
- agent label (`claude-code` / `gemini-cli` / …)
- tool_canonical (`Edit` / `Bash` / …)
- detail truncated to 250 bytes
- timestamps
- file path basenames (home dir replaced with `~`)
- session start / end + heuristic name
- git commit subjects

Never sent: `raw_json`, full hostname, full file paths, IP, MCP
message bodies. Phase 3 M7's redaction layer runs on every prompt
before send.

## Acceptance gate (Phase 4 v0.7.0 → v0.7.1)

- F-1 through F-6 (see `.omc/autopilot/spec.md`) demonstrably true
- AI service layer enforces the daily USD ceiling and falls back
  cleanly when exceeded
- Cache hit rate > 80 % under normal navigation
- Privacy boundary verified — fixture run produces zero `raw_json`
  bytes in captured outbound HTTP body
- Phase 1 + 2 + 3 acceptance tests stay green
- No AI / Claude / LLM attribution leaks in commit messages or
  code comments
- Studio binary ≤ 22 MB; capture binary still ≤ 4 MB

Phase 3 M10 (external beta) remains a separate gate.

## Out of scope (deferred)

- Q&A / "Ask my history" — input UI conflicts with push-only
  principle. Phase 5 candidate at the earliest.
- Per-file insight card on `/file/<path>` — v0.7.1
- macOS notifications / OS push — v0.7.1
- Ollama provider as default — v0.7.1
- FluxGate integration — Phase 5
- VS Code / Cursor extension — Phase 5
- HTTP / SSE MCP transport — Phase 5
- Phase 3 M10 external beta — separate gate after v0.7.0
