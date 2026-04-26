# ADR-0004: Onboarding (init + doctor + welcome flow)

## Status

Accepted (Phase 1, 2026-04).

## Context

Pre-Phase 1, fluxmirror had no first-run UX: the user installed the
plugin, the hook started writing rows immediately, and there was no
way to inspect or change configuration. Multi-OS support landed in the
same phase, which made path-and-shell defaults more important to
surface.

## Decision

Three onboarding subcommands plus a layered config:

- `fluxmirror init` — Tier A asks for language and timezone (smart
  defaults from `tz::infer_default_tz()` and `LANG`). With
  `--non-interactive` the defaults win silently. With `--advanced` the
  Tier B prompts (retention, self-noise filter, per-agent toggles)
  also fire. Init then calls `wrapper::set(<chosen>)` to atomically
  rewrite every plugin's `hooks.json`, writes
  `~/.fluxmirror/config.json` with `schema_version: 1`, and prints a
  `Try: fluxmirror today` hint.
- `fluxmirror config get|set|show|explain` — `explain` prints each key
  and which layer provided it.
- `fluxmirror doctor` — prints a 5-component health table (config,
  database, schema_version, wrapper, agents discovered) plus last hook
  fire time. Exit codes: `0` clean, `1` warnings, `2` errors.

Configuration layering, **highest priority first**:

```
CLI flags
  > env vars
    > project ./.fluxmirror.toml
      > user ~/.fluxmirror/config.json (or platform-default config dir)
        > inferred defaults
```

Per-OS config locations:

| OS | Path |
|---|---|
| macOS | `~/.fluxmirror/config.json` (legacy compat — kept for users with existing v0.5.x state) |
| Linux | `${XDG_CONFIG_HOME:-~/.config}/fluxmirror/config.json` |
| Windows | `%APPDATA%\fluxmirror\config.json` |

Welcome marker: on the first successful hook insert, the hook writes
`~/.fluxmirror/.first-fire-at` plus a one-page `welcome.md` next to
the config. The next time the user runs a slash command, a one-line
hint pointing at `/fluxmirror:setup` is prepended exactly once. Both
artifacts are opt-in friendliness — they never block, and if the user
deletes them, nothing else cares.

## Consequences

- New users get a guided first-run without forfeiting the
  zero-prompt CI / scripted install path (`--non-interactive`).
- Doctor gives a single command for "is anything broken?". The
  non-zero exit codes let CI scripts gate on it.
- The four-level config layering matches the rule used by `kubectl`,
  `git`, and most other CLI tools, so users do not have to learn a new
  precedence model.
- macOS keeps the historical `~/.fluxmirror/` location for backwards
  compatibility with existing v0.5.x installs; new platforms (Linux,
  Windows) use the OS-native config dir from day one.
