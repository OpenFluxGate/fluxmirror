# ADR-0003: EventStore trait, SqliteStore the only Phase 1 impl

## Status

Accepted (Phase 1, 2026-04).

## Context

Pre-Phase 1, the hook subcommand opened a `rusqlite::Connection`
directly in `cmd/hook.rs`, called `CREATE TABLE IF NOT EXISTS` inline,
and bound an `INSERT` per call. The proxy did the same in its own
crate against a different schema. There was no abstraction over the
storage layer, no migration tracking, and no way to redirect writes to
a different backend without forking the call sites.

## Decision

Introduce a small trait in `fluxmirror-store`:

```rust
pub trait EventStore: Send + Sync {
    fn write_agent_event(&self, e: &AgentEvent) -> Result<()>;
    fn write_proxy_event(&self, e: &ProxyEvent) -> Result<()>;
    fn schema_version(&self) -> u32;
    fn migrate(&self) -> Result<()>;
}
```

Ship exactly one implementation in Phase 1: `SqliteStore`. The hook
subcommand opens it via `SqliteStore::open(default_db_path()?)` which
runs `migrate()` automatically. `cmd/hook.rs` no longer carries any
SQL; the proxy lib's separate store is left untouched in Phase 1
(separate refactor — its on-disk schema is unchanged so a hook-only DB
is still readable / writable by the existing proxy code).

## Consequences

- The `migrate()` contract is **idempotent**, **additive**, and **fully
  transactional** — see `crates/fluxmirror-store/src/sqlite.rs`. A
  brand-new DB gets the full v1 schema; a legacy DB gets one
  `ALTER TABLE … ADD COLUMN` per missing column plus index ensures.
  Old rows keep their data; the new `schema_version` column has a
  `DEFAULT 1` so they read back as 1. The whole migration runs in one
  transaction.
- Adding `PostgresStore` or `ClickhouseStore` in Phase 4 is a matter of
  implementing the trait; no call site has to change.
- Phase 1 deliberately does not extend the trait with `query()` /
  `stream()` methods. Reports query through `SqliteStore` directly via
  the bundled rusqlite, so the trait stays minimal until a second impl
  actually exists. (YAGNI: the project's CLAUDE.md forbids speculative
  abstractions.)
