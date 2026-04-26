# Phase 2 Plan — fluxmirror

> Branch: `feature/phase2`
> Goal: take fluxmirror from "personal scratch tool" to "I can confidently
> send this to a friend."
> Live state (untracked, machine-local) lives at
> `.omc/autopilot/{spec,plan,progress}.md`. This file is the
> branch-visible summary.

## Why Phase 2

Today's debugging surfaced four install / init regressions on a single
day's run, while the README still promises capabilities (anomaly alerts,
control / policy, FluxGate integration) that aren't implemented. Phase 2
closes that trust gap: every promise in the README has a working test,
every install path has a CI guard, and one differentiating feature exists
to justify installation over rolling your own SQLite-and-hook script.

## Milestones

| ID | Milestone | Estimate | Critical path |
|---|---|---|---|
| M1 | Slash commands → binary subcommands | 1.5 wk | yes |
| M2 | CI install-sim regression guard | 4 d | yes |
| M3 | First-run friction removal | 4 d | yes |
| M4 | Honest README / value-prop alignment | 2 d | parallel |
| M5 | One differentiator (HTML card / cross-agent / anomaly) | ~2 wk | parallel |
| M6 | External beta with 1+ tester | 1 wk | yes |

Total: ~6-8 weeks of focused work.

## M1 — Why first

The slash commands today are markdown files containing shell blocks that
the model is expected to execute in order. Today proved that's fragile
(Gemini's model raced past an interactive question step and ran the
non-interactive path immediately). Moving the report logic into Rust
subcommands removes prompt engineering from the runtime, makes the
reports testable with fixture DBs, and reduces every slash command file
to a 2-line shell that calls the binary. Once M1 lands, M2 (CI tests
exist deterministic stdout to assert against), M3 (demo row produces a
real report), and M5 (HTML card is a new subcommand, not a new prompt)
all become much cheaper.

## Phase 2 → Phase 3 gate

All of:

- [ ] F-1 through F-6 in `.omc/autopilot/spec.md` demonstrably true
- [ ] Beta tester reports continued use after 7 days
- [ ] CI install-sim job ≥ 30 consecutive green runs
- [ ] Zero open P0 issues

## What Phase 2 explicitly does NOT include

These slip to Phase 3:

- Full anomaly / policy engine (M5 ships at most one heuristic seed)
- FluxGate integration
- Redaction / sensitive-data masking
- Windows native `cmd.exe` shim hardening
- TOML config parser implementation (currently a stub)
- A docs site / OSS launch readiness

## Resumption protocol

If a session is compacted mid-flight:

1. Confirm branch with `git status`.
2. Read `.omc/autopilot/spec.md` for mission context.
3. Read `.omc/autopilot/plan.md` for the milestone task breakdown.
4. Read `.omc/autopilot/progress.md` (bottom up) for what's just been
   done and what's next.
5. Resume from the "Resumption pointer" at the bottom of progress.md.
