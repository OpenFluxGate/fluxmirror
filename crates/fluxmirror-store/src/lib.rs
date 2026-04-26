// fluxmirror-store: pluggable event storage.
//
// Defines the `EventStore` trait that the rest of fluxmirror writes
// against, plus the only impl that ships in Phase 1: `SqliteStore`.
// Adding Postgres / ClickHouse / etc. later means a new module behind
// the same trait — call sites stay put.
//
// Schema is owned here too. `SqliteStore::open` runs `migrate()` once
// per process so callers never have to think about schema state.

pub mod sqlite;

pub use sqlite::SqliteStore;

use fluxmirror_core::{AgentEvent, ProxyEvent, Result};

/// Storage backend for fluxmirror events.
///
/// Both write paths are infallible from the agent's perspective — the
/// hook subcommand maps any error to a stderr log line and exits 0 so
/// telemetry never breaks the calling agent.
pub trait EventStore: Send + Sync {
    fn write_agent_event(&self, e: &AgentEvent) -> Result<()>;
    fn write_proxy_event(&self, e: &ProxyEvent) -> Result<()>;
    fn schema_version(&self) -> u32;
    fn migrate(&self) -> Result<()>;
}
