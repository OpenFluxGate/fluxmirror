# fluxmirror-proxy (Rust)

Long-running stdio MCP proxy. Sits between an MCP client (Claude Desktop)
and a real MCP server, transparently relays JSON-RPC traffic in both
directions, and writes every line to the FluxMirror SQLite database.

Replaces the original Java jar — same CLI, same DB schema, but a single
~1.2 MB statically-linked binary with **zero runtime dependencies**.

## Build

```bash
cd rust-proxy
cargo build --release
# → target/release/fluxmirror-proxy
```

## CLI

```
fluxmirror-proxy --server-name <name> --db <path> \
  [--capture-c2s <path>] [--capture-s2c <path>] \
  -- <real MCP server command...>
```

| Flag | Required | Description |
|---|---|---|
| `--server-name <name>` | yes | Identifier for this MCP server (stored in `events.server_name`) |
| `--db <path>` | yes | SQLite database file path |
| `--capture-c2s <path>` | no | Append raw client→server bytes here (debug) |
| `--capture-s2c <path>` | no | Append raw server→client bytes here (debug) |
| `-- <command...>` | yes | The real MCP server to spawn |

## Architecture

```
parent stdin (Claude Desktop) ──c2s thread──▶ child stdin
child stdout                  ──s2c thread──▶ parent stdout
                              ──framer──▶ events table (SQLite, batched)
child stderr                  ──inherit──▶ parent stderr
```

- **Two relay threads** (c2s, s2c) using stdlib `std::thread`. Reads
  block on the source; writes are best-effort to two sinks (downstream
  + capture file + framer).
- **Newline-delimited framer** with 10 MiB cap and resync-on-overflow
  (matches the original Java behavior).
- **Background writer thread** drains an MPSC channel into batched
  SQLite INSERTs (up to 100 events per transaction). Final drain on
  channel disconnect ensures no data loss on shutdown.
- **Child process** spawned via `std::process::Command`; stderr
  inherited (clients see server diagnostics). On shutdown: SIGTERM,
  wait 2s, then SIGKILL if still alive.

## SQLite schema

Same table as the original Java proxy:

```sql
CREATE TABLE events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts_ms INTEGER NOT NULL,
  direction TEXT NOT NULL CHECK (direction IN ('c2s', 's2c')),
  method TEXT,
  message_json TEXT NOT NULL,
  server_name TEXT NOT NULL
);
CREATE INDEX idx_events_ts ON events(ts_ms);
```

`method` is extracted via `serde_json` (best-effort) so requests are
queryable by their JSON-RPC method name.

## Tests

```bash
cargo test --release        # 15 unit tests across cli/framer/store
```

Plus an integration smoke test in `.github/workflows/test.yml` that
spawns the binary with `cat` as the child and verifies both c2s and s2c
events land in SQLite with correct method extraction.

## Logging

Diagnostics go to stderr only — stdout is reserved for the MCP protocol
relay. Any accidental stdout write would corrupt the protocol and break
the client.

## Status

- Logic complete and parity-tested with the Java implementation
- Integration smoke test passes locally and in CI
- Cross-arch CI matrix (5 targets) builds release binaries on every
  `v*` tag push (see `.github/workflows/rust-release.yml`)
