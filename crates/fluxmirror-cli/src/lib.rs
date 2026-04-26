// fluxmirror-cli library surface.
//
// The crate ships a single `[[bin]]` (the `fluxmirror` executable) but
// also exposes a thin library so integration tests can call into
// `cmd::*` directly without spawning a subprocess. Keeping the module
// tree behind both `main.rs` and `lib.rs` would mean two trees of the
// same files; instead, `main.rs` re-uses this `lib.rs` via
// `use fluxmirror_cli::cmd;`.

pub mod cmd;
