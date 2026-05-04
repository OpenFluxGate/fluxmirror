// fluxmirror-core: shared types and primitives.
//
// This crate is the data definition layer of fluxmirror. It contains:
//
//   - errors    Result/Error types crossed at every boundary
//   - event     AgentEvent / ProxyEvent / Direction / AgentId
//   - normalize tool-name canonicalization + per-tool detail extraction
//   - paths     OS-aware home/db/config/cache directory resolution
//   - tz        chrono-tz parse + IANA inference
//   - config    layered config (defaults < inferred < user file < project < env < CLI)

pub mod config;
pub mod cost;
pub mod errors;
pub mod event;
pub mod normalize;
pub mod paths;
pub mod redact;
pub mod report;
pub mod tz;

pub use config::{
    AgentToggle, AgentsConfig, AiConfig, Config, ConfigSource, Language, RedactionConfig,
    SelfNoiseConfig, StorageConfig, StudioConfig, WrapperConfig, WrapperKind,
};
pub use errors::{Error, Result};
pub use event::{AgentEvent, AgentId, Direction, ProxyEvent};
pub use normalize::{extract_detail, normalize, ToolClass, ToolKind};

// Re-export the third-party crates we expose through public types so
// downstream crates can pin to the same version we built against
// without re-listing the dependency.
pub use chrono;
pub use chrono_tz;

// Crate-internal helpers used only from #[cfg(test)] code.
//
// `env_lock` serializes any test that touches process-global env vars
// (HOME, USERPROFILE, XDG_*, FLUXMIRROR_*). Without it, parallel tests
// in different modules race each other and intermittently fail under
// `cargo test --workspace` on Linux + Windows runners.
#[cfg(test)]
pub(crate) mod test_lock {
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Block until no other env-mutating test in this crate is running.
    /// Poisoned-mutex recovery returns the inner guard so a panic in
    /// one test never cascades into all other env tests.
    pub fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }
}
