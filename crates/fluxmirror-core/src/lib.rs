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
pub mod errors;
pub mod event;
pub mod normalize;
pub mod paths;
pub mod tz;

pub use config::{
    AgentToggle, AgentsConfig, Config, ConfigSource, Language, SelfNoiseConfig, StorageConfig,
    WrapperConfig, WrapperKind,
};
pub use errors::{Error, Result};
pub use event::{AgentEvent, AgentId, Direction, ProxyEvent};
pub use normalize::{extract_detail, normalize, ToolClass, ToolKind};
