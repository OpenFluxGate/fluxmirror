# Pricing sources

The pricing table in `crates/fluxmirror-core/src/cost/mod.rs` is a
manual snapshot of the public per-MTok rates each provider publishes.
This document holds the source URL for every entry so the next
quarterly refresh has one place to walk.

Cost figures are best-effort — fluxmirror estimates an upper-bound for
your local development usage from MCP usage blocks plus a heuristic on
non-MCP agent activity. It is not a billing system.

## Refresh cadence

Quarterly. Bump `PRICING` together with the entries below; the
single-binary fluxmirror release sweeps the change to every report.

## Anthropic — `https://www.anthropic.com/pricing#api`

Source URL: <https://www.anthropic.com/pricing#api> (last sampled 2026-04).

| model | input $/MTok | output $/MTok | cache read $/MTok | cache write $/MTok |
|---|---|---|---|---|
| claude-opus-4-7 | 15.00 | 75.00 | 1.50 | 18.75 |
| claude-opus-4 | 15.00 | 75.00 | 1.50 | 18.75 |
| claude-sonnet-4-6 | 3.00 | 15.00 | 0.30 | 3.75 |
| claude-sonnet-4 | 3.00 | 15.00 | 0.30 | 3.75 |
| claude-haiku-4-5 | 1.00 | 5.00 | 0.10 | 1.25 |
| claude-3-5-sonnet | 3.00 | 15.00 | 0.30 | 3.75 |
| claude-3-5-haiku | 0.80 | 4.00 | 0.08 | 1.00 |
| claude-3-opus | 15.00 | 75.00 | 1.50 | 18.75 |

## OpenAI — `https://openai.com/api/pricing/`

Source URL: <https://openai.com/api/pricing/> (last sampled 2026-04).

| model | input $/MTok | output $/MTok | cache read $/MTok | notes |
|---|---|---|---|---|
| gpt-4-turbo | 10.00 | 30.00 | — | flagship 2024 |
| gpt-4o | 2.50 | 10.00 | 1.25 | flagship multimodal |
| gpt-4o-mini | 0.15 | 0.60 | 0.075 | cost-tier |

OpenAI does not publish a separate cache-write price; cache writes are
billed at the standard input rate. Our table omits the field — the
`cost_for_usage` helper falls back to `input_per_mtok_usd` when
`cache_write_per_mtok_usd` is `None`.

## Google — `https://ai.google.dev/pricing`

Source URL: <https://ai.google.dev/pricing> (last sampled 2026-04).

| model | input $/MTok | output $/MTok | cache read $/MTok | notes |
|---|---|---|---|---|
| gemini-2.5-pro | 1.25 | 10.00 | 0.31 | <=200K tokens tier |
| gemini-2.5-flash | 0.30 | 2.50 | 0.075 | flash tier |
| gemini-1.5-pro | 1.25 | 5.00 | 0.3125 | legacy |
| gemini-1.5-flash | 0.075 | 0.30 | 0.01875 | legacy |

Google's >200K-token premium tier is intentionally not modelled — most
local developer requests fit in the under-200K bucket and the doubled
rate would only inflate the estimate.

## Adding a model

1. Find the canonical per-MTok rate on the provider's pricing page.
2. Add the entry to `PRICING` in `cost/mod.rs`. Provider tag is
   lowercase (`anthropic` / `openai` / `google` / …).
3. Add a row to the matching section above with the source URL.
4. Re-run `cargo test -p fluxmirror-core` — the lookup tests assert
   exact-match and prefix-match resolution stay stable.
