// fluxmirror-store: pluggable event storage.
//
// STEP 1 placeholder. Real EventStore trait + SqliteStore impl + v1
// migration land in STEP 3. For now we re-export rusqlite so downstream
// crates that link against this lib still compile.

pub use rusqlite;
