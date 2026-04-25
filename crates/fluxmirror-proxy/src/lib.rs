// fluxmirror-proxy: long-running stdio MCP relay logic, lifted from
// the previous rust-proxy/ binary. The runtime entry now lives in the
// fluxmirror-cli crate (`fluxmirror proxy ...`); this lib exposes the
// pieces it needs (CLI parsing + bridge + store + child supervision).

pub mod bridge;
pub mod child;
pub mod cli;
pub mod framer;
pub mod store;
pub mod writer;
