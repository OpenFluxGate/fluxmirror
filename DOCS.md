# FluxMirror Technical Documentation

A local stdio proxy that sits between AI agents (Claude Desktop, Cursor, etc.)
and their MCP servers, logging every JSON-RPC message to SQLite for observability.

## Architecture

```
Claude Desktop  <-stdio->  [fluxmirror.jar]  <-stdio->  real MCP server
                                  |
                               SQLite
```

The proxy spawns the real MCP server as a child process and bridges stdio
in both directions. Each JSON-RPC message is parsed and persisted to SQLite
before being forwarded.

## Project structure

```
src/main/java/io/github/openfluxgate/fluxmirror/
├── Main.java              # Entry point, resource bootstrap, shutdown hook
├── cli/
│   └── CliArgs.java       # CLI argument parser (record)
├── bridge/
│   ├── StdioBridge.java   # Bidirectional stdio relay (virtual threads)
│   ├── MessageFramer.java # Newline-delimited JSON-RPC framer
│   └── ChildProcess.java  # Child process lifecycle (AutoCloseable)
├── model/
│   └── Event.java         # Immutable event record
└── storage/
    ├── EventStore.java    # SQLite connection, schema, batch insert
    └── EventWriter.java   # Async background writer thread
```

## Components

### CliArgs

Immutable record that parses command-line arguments via manual switch loop.

| Argument | Required | Description |
|----------|----------|-------------|
| `--server-name <name>` | Yes | Identifier for the MCP server |
| `--db <path>` | Yes | SQLite database file path |
| `--capture-c2s <path>` | No | Debug: dump raw client-to-server bytes |
| `--capture-s2c <path>` | No | Debug: dump raw server-to-client bytes |
| `-- <command...>` | Yes | Real MCP server command to spawn |

### ChildProcess

Manages the real MCP server as a child process via `ProcessBuilder`.

- Inherits stderr so Claude Desktop sees server error output
- Graceful shutdown: SIGTERM, 2-second wait, then SIGKILL if needed
- Exposes child stdin/stdout streams for the relay

### MessageFramer

Stateful parser that extracts complete JSON-RPC messages from a raw byte stream.
Accumulates bytes in a buffer and splits on newline boundaries (`0x0A`).

- Strips trailing `\r` before returning messages
- Discards messages exceeding 10 MB (overflow protection)
- Best-effort: parsing failures never block the relay

### StdioBridge

Launches two virtual threads (Java 21 Project Loom) for bidirectional relay:

- **c2s thread**: reads parent stdin, writes to child stdin
- **s2c thread**: reads child stdout, writes to parent stdout

Each relay loop follows a priority chain:

1. **Relay** (absolute priority) — forward bytes unchanged
2. **Capture** (best-effort) — write to optional debug file
3. **Frame** (best-effort) — parse messages, log, enqueue for SQLite

Design principle: relay is never blocked by capture, framing, or storage failures.

### Event

Immutable record: `(tsMs, direction, serverName, rawBytes)`.

Direction values are `"c2s"` (client-to-server) or `"s2c"` (server-to-client).

### EventStore

Opens SQLite at the given path, creates schema if needed, and provides batch insert.

**SQLite pragmas:**
- `journal_mode = WAL` — concurrent reads during writes
- `synchronous = NORMAL` — balance durability and speed

**Schema:**

```sql
CREATE TABLE IF NOT EXISTS events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts_ms INTEGER NOT NULL,
  direction TEXT NOT NULL CHECK (direction IN ('c2s', 's2c')),
  method TEXT,
  message_json TEXT NOT NULL,
  server_name TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_events_ts ON events(ts_ms);
```

Batch insert runs inside an explicit transaction with rollback on failure.
The `method` field is extracted from JSON using Jackson; null if parsing fails.

### EventWriter

Background thread that drains events from a `BlockingQueue<Event>` and writes
them to SQLite in batches (up to 100 events per batch, 100ms poll timeout).

Flushes remaining events on shutdown before exiting.

## Thread model

| Thread | Type | Purpose |
|--------|------|---------|
| main | OS | Bootstrap, blocks on relay completion, cleanup |
| c2s relay | Virtual | Forward client stdin to child stdin |
| s2c relay | Virtual | Forward child stdout to parent stdout |
| event-writer | OS | Drain queue, batch insert to SQLite |
| shutdown hook | OS | Terminate child process on JVM exit |

## Data flow

```
Client stdout
  → c2s virtual thread reads System.in (8192 byte buffer)
    → writes to child stdin (relay)
    → writes to capture file (optional, best-effort)
    → feeds MessageFramer → extracts messages
    → creates Event → offers to BlockingQueue (non-blocking)

Child stdout
  → s2c virtual thread reads child stdout (8192 byte buffer)
    → writes to System.out (relay)
    → writes to capture file (optional, best-effort)
    → feeds MessageFramer → extracts messages
    → creates Event → offers to BlockingQueue (non-blocking)

BlockingQueue (capacity 10,000)
  → EventWriter thread polls every 100ms
    → batches up to 100 events
    → atomic batch insert to SQLite
```

## Resource lifecycle (Main.java)

```
1. Parse CliArgs
2. Open optional capture streams (c2s, s2c)
3. Create EventStore (open SQLite, create schema)
4. Create ArrayBlockingQueue<Event> (capacity 10,000)
5. Start EventWriter thread
6. Start ChildProcess
7. Register JVM shutdown hook (close child on exit)
8. Run StdioBridge (blocks until relay completes)
9. Interrupt EventWriter, join with 5s timeout
10. Close EventStore (via try-with-resources)
```

## Usage

```bash
java -jar fluxmirror.jar \
  --server-name fs \
  --db ~/.fluxmirror/events.db \
  -- npx -y @modelcontextprotocol/server-filesystem /tmp
```

With debug capture:

```bash
java -jar fluxmirror.jar \
  --server-name fs \
  --db ~/.fluxmirror/events.db \
  --capture-c2s /tmp/c2s.raw \
  --capture-s2c /tmp/s2c.raw \
  -- npx -y @modelcontextprotocol/server-filesystem /tmp
```

## Build

```bash
./gradlew shadowJar
# Output: build/libs/fluxmirror-all.jar
```

## Dependencies

| Library | Version | Purpose |
|---------|---------|---------|
| jackson-databind | 2.18.3 | JSON parsing (method extraction) |
| sqlite-jdbc | 3.47.2.0 | SQLite driver |
| slf4j-api + slf4j-simple | 2.0.17 | Logging (stderr only) |

## Logging

All logs go to stderr via SLF4J-Simple. stdout is reserved exclusively for the
MCP protocol relay — any accidental stdout write corrupts the protocol.

Log format: `HH:mm:ss.SSS [level] message`

Key log events:
- `spawned pid=<n>, server-name=<name>` — child process started
- `relay started` / `relay stopped direction=<dir>` — relay lifecycle
- `[c2s] <json>` / `[s2c] <json>` — intercepted messages (truncated at 2000 chars)
- `event queue full, dropping events` — backpressure warning

## Design decisions

- **Virtual threads for relay**: lightweight, ideal for IO-bound forwarding
- **Async batch writer**: decouples relay from SQLite latency via queue
- **Best-effort framing**: relay never blocked by parse or storage failures
- **No Spring Boot**: small JAR, fast startup, explicit resource management
- **Manual CLI parsing**: no external dependencies for argument handling
- **PreparedStatement batching**: SQL-injection safe, efficient bulk insert

## Known limitations (Week 1 PoC)

- Newline-delimited framing only (not Content-Length-prefixed per MCP spec)
- Single MCP server at a time
- Raw logging with no redaction of sensitive data
- No UI — query events via raw SQL only
- No integration test suite yet
