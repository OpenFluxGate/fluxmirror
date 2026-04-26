# ADR-0001: Single fluxmirror binary, kubectl-style subcommands

## Status

Accepted (Phase 1, 2026-04).

## Context

Until v0.5.6 fluxmirror shipped two binaries: `fluxmirror-hook` and
`fluxmirror-proxy`. Each had its own crate, its own release matrix,
its own cache key. Adding more subcommands (`init`, `doctor`,
`config`, `wrapper`, `window`, `histogram`, `daily-totals`,
`per-day-files`, `sqlite`) would have required either growing that
fan-out further or collapsing into one binary.

## Decision

Collapse into a single `fluxmirror` binary at `crates/fluxmirror-cli`,
dispatching subcommands via clap-derive. Modeled on `kubectl` /
`git` / `cargo`. The previous behavior is preserved 1:1 under
`fluxmirror hook` and `fluxmirror proxy`.

## Consequences

Positives:

- One binary on `PATH`; one cache entry; one release artifact per arch.
- Shared dependencies (`chrono`, `rusqlite`, `serde_json`) are linked
  once.
- Subcommands reuse `fluxmirror-core` (`Config`, `paths`, `normalize`)
  without contortion.

Negatives / costs:

- Slightly larger binary (~2.2 MB stripped vs 1.2 MB previously).
- Wrappers must learn to exec `fluxmirror hook --kind <X>` instead of
  the old flag-only form. STEP 7 of Phase 1 ships `shim.sh`,
  `shim.mjs`, `shim.cmd`, and `router.sh` to do exactly this.
- The Phase 1 release publishes legacy `fluxmirror-hook-<arch>` and
  `fluxmirror-proxy-<arch>` asset names as plain copies of the same
  binary so existing wrappers downloading those names keep working
  until the next major release.
