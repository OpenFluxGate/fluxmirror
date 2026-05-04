// Phase 4 M-A6 — integration coverage of the heuristic anomaly detector.
//
// Each rule gets two cases: a positive fixture that fires the rule,
// and a control fixture that stays quiet. Five rules × two cases =
// ten tests minimum, per the milestone spec.
//
// Fixtures seed `agent_events` (and `events` for the MCP rule)
// through a hand-rolled schema so this test stays in
// `fluxmirror-core`'s dev-deps shape (tempfile only).

use chrono::{DateTime, Duration, TimeZone, Utc};
use chrono_tz::Tz;
use fluxmirror_core::report::anomaly::{detect_anomalies_at, AnomalyWindow};
use fluxmirror_core::report::dto::AnomalyKind;
use rusqlite::{params, Connection};
use tempfile::TempDir;

fn schema(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_events ( \
            id INTEGER PRIMARY KEY AUTOINCREMENT, \
            ts TEXT NOT NULL, \
            agent TEXT NOT NULL, \
            session TEXT, \
            tool TEXT, \
            tool_canonical TEXT, \
            tool_class TEXT, \
            detail TEXT, \
            cwd TEXT, \
            host TEXT, \
            user TEXT, \
            schema_version INTEGER NOT NULL DEFAULT 1, \
            raw_json TEXT \
         );
         CREATE TABLE IF NOT EXISTS events ( \
            id INTEGER PRIMARY KEY AUTOINCREMENT, \
            ts_ms INTEGER NOT NULL, \
            direction TEXT NOT NULL CHECK (direction IN ('c2s','s2c')), \
            method TEXT, \
            message_json TEXT NOT NULL, \
            server_name TEXT NOT NULL \
         );",
    )
    .unwrap();
}

fn open() -> (TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    let conn = Connection::open(&path).unwrap();
    schema(&conn);
    (dir, conn)
}

fn insert_event(
    conn: &Connection,
    ts: DateTime<Utc>,
    agent: &str,
    tool: &str,
    detail: &str,
) {
    let ts_str = ts.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    conn.execute(
        "INSERT INTO agent_events \
         (ts, agent, session, tool, tool_canonical, tool_class, detail, \
          cwd, host, user, schema_version, raw_json) \
         VALUES (?1, ?2, 's', ?3, ?3, 'Other', ?4, '/p', 'h', 'u', 1, '{}')",
        params![ts_str, agent, tool, detail],
    )
    .unwrap();
}

fn insert_mcp_method(conn: &Connection, ts: DateTime<Utc>, method: &str) {
    let ts_ms = ts.timestamp_millis();
    conn.execute(
        "INSERT INTO events (ts_ms, direction, method, message_json, server_name) \
         VALUES (?1, 'c2s', ?2, '{}', 'fs')",
        params![ts_ms, method],
    )
    .unwrap();
}

fn anchor_now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 4, 26, 12, 0, 0).unwrap()
}

fn tz() -> Tz {
    chrono_tz::UTC
}

// ---------- FileEditSpike ----------

#[test]
fn file_edit_spike_fires_on_giant_burst() {
    let (_d, conn) = open();
    let now = anchor_now();
    // Today: 14 edits to the same file, blowing past the rolling avg.
    for i in 0..14 {
        insert_event(
            &conn,
            now - Duration::minutes(i),
            "claude-code",
            "Edit",
            "Cargo.toml",
        );
    }
    // Baseline (28d): 4 edits over 4 different days. Per-day avg ≈ 0.14.
    for i in 1..=4 {
        insert_event(
            &conn,
            now - Duration::days(i),
            "claude-code",
            "Edit",
            "Cargo.toml",
        );
    }
    let detections = detect_anomalies_at(&conn, &tz(), AnomalyWindow::Today, now).unwrap();
    assert!(
        detections.iter().any(|d| d.kind == AnomalyKind::FileEditSpike),
        "expected FileEditSpike, got {detections:?}"
    );
}

#[test]
fn file_edit_spike_silent_on_steady_workload() {
    let (_d, conn) = open();
    let now = anchor_now();
    // Today: 3 edits — under the FILE_EDIT_MIN floor, never trips.
    for i in 0..3 {
        insert_event(
            &conn,
            now - Duration::minutes(i),
            "claude-code",
            "Edit",
            "Cargo.toml",
        );
    }
    // Baseline matches today's pace.
    for i in 1..=20 {
        insert_event(
            &conn,
            now - Duration::days(i),
            "claude-code",
            "Edit",
            "Cargo.toml",
        );
    }
    let detections = detect_anomalies_at(&conn, &tz(), AnomalyWindow::Today, now).unwrap();
    assert!(
        !detections.iter().any(|d| d.kind == AnomalyKind::FileEditSpike),
        "did not expect FileEditSpike, got {detections:?}"
    );
}

// ---------- ToolMixDeparture ----------

#[test]
fn tool_mix_departure_fires_on_format_shift() {
    let (_d, conn) = open();
    let now = anchor_now();
    // Today: all Bash, no Edit. Disjoint from baseline.
    for i in 0..30 {
        insert_event(
            &conn,
            now - Duration::minutes(i),
            "claude-code",
            "Bash",
            "ls",
        );
    }
    // Baseline: all Edit. Cosine distance to today is 1.0 — well over 0.4.
    for i in 1..=20 {
        for j in 0..3 {
            insert_event(
                &conn,
                now - Duration::days(i) + Duration::minutes(j),
                "claude-code",
                "Edit",
                "src/lib.rs",
            );
        }
    }
    let detections = detect_anomalies_at(&conn, &tz(), AnomalyWindow::Today, now).unwrap();
    assert!(
        detections.iter().any(|d| d.kind == AnomalyKind::ToolMixDeparture),
        "expected ToolMixDeparture, got {detections:?}"
    );
}

#[test]
fn tool_mix_departure_silent_on_matching_mix() {
    let (_d, conn) = open();
    let now = anchor_now();
    // Today: 2 Edit + 1 Bash.
    insert_event(&conn, now, "claude-code", "Edit", "src/lib.rs");
    insert_event(&conn, now - Duration::minutes(1), "claude-code", "Edit", "src/main.rs");
    insert_event(&conn, now - Duration::minutes(2), "claude-code", "Bash", "ls");
    // Baseline: same proportion across many days.
    for i in 1..=20 {
        insert_event(&conn, now - Duration::days(i), "claude-code", "Edit", "src/lib.rs");
        insert_event(&conn, now - Duration::days(i), "claude-code", "Edit", "src/main.rs");
        insert_event(&conn, now - Duration::days(i), "claude-code", "Bash", "ls");
    }
    let detections = detect_anomalies_at(&conn, &tz(), AnomalyWindow::Today, now).unwrap();
    assert!(
        !detections.iter().any(|d| d.kind == AnomalyKind::ToolMixDeparture),
        "did not expect ToolMixDeparture, got {detections:?}"
    );
}

// ---------- NewAgent ----------

#[test]
fn new_agent_fires_on_unseen_agent_today() {
    let (_d, conn) = open();
    let now = anchor_now();
    // Baseline: only claude-code.
    for i in 1..=20 {
        insert_event(&conn, now - Duration::days(i), "claude-code", "Edit", "src/x.rs");
    }
    // Today: gemini-cli appears for the first time.
    insert_event(&conn, now, "gemini-cli", "edit_file", "src/x.rs");
    let detections = detect_anomalies_at(&conn, &tz(), AnomalyWindow::Today, now).unwrap();
    assert!(
        detections.iter().any(|d| d.kind == AnomalyKind::NewAgent),
        "expected NewAgent, got {detections:?}"
    );
}

#[test]
fn new_agent_silent_when_agent_in_baseline() {
    let (_d, conn) = open();
    let now = anchor_now();
    for i in 1..=20 {
        insert_event(&conn, now - Duration::days(i), "claude-code", "Edit", "src/x.rs");
        insert_event(&conn, now - Duration::days(i), "gemini-cli", "edit_file", "src/x.rs");
    }
    insert_event(&conn, now, "gemini-cli", "edit_file", "src/x.rs");
    let detections = detect_anomalies_at(&conn, &tz(), AnomalyWindow::Today, now).unwrap();
    assert!(
        !detections.iter().any(|d| d.kind == AnomalyKind::NewAgent),
        "did not expect NewAgent, got {detections:?}"
    );
}

// ---------- NewMcpMethod ----------

#[test]
fn new_mcp_method_fires_on_unseen_method() {
    let (_d, conn) = open();
    let now = anchor_now();
    // Baseline methods (need at least one event so the agent rule
    // doesn't also fire and obscure the assertion).
    for i in 1..=10 {
        insert_event(&conn, now - Duration::days(i), "claude-code", "Edit", "src/x.rs");
        insert_mcp_method(&conn, now - Duration::days(i), "tools/list");
    }
    // Today: a new method appears.
    insert_event(&conn, now, "claude-code", "Edit", "src/x.rs");
    insert_mcp_method(&conn, now, "resources/read");
    let detections = detect_anomalies_at(&conn, &tz(), AnomalyWindow::Today, now).unwrap();
    assert!(
        detections.iter().any(|d| d.kind == AnomalyKind::NewMcpMethod),
        "expected NewMcpMethod, got {detections:?}"
    );
}

#[test]
fn new_mcp_method_silent_when_method_in_baseline() {
    let (_d, conn) = open();
    let now = anchor_now();
    for i in 1..=10 {
        insert_event(&conn, now - Duration::days(i), "claude-code", "Edit", "src/x.rs");
        insert_mcp_method(&conn, now - Duration::days(i), "tools/list");
    }
    insert_event(&conn, now, "claude-code", "Edit", "src/x.rs");
    insert_mcp_method(&conn, now, "tools/list");
    let detections = detect_anomalies_at(&conn, &tz(), AnomalyWindow::Today, now).unwrap();
    assert!(
        !detections.iter().any(|d| d.kind == AnomalyKind::NewMcpMethod),
        "did not expect NewMcpMethod, got {detections:?}"
    );
}

// ---------- CostPerCallRise ----------
//
// `claude-code` resolves to a known model in `default_model_for_agent`
// so the heuristic cost path engages. Today's calls carry far longer
// `detail` strings than baseline's, blowing the per-call cost ratio
// past 2× without changing call counts.

#[test]
fn cost_per_call_rise_fires_on_long_details() {
    let (_d, conn) = open();
    let now = anchor_now();
    // Baseline: many short-detail calls per day for 14 days.
    for i in 1..=14 {
        for j in 0..6 {
            insert_event(
                &conn,
                now - Duration::days(i) + Duration::minutes(j),
                "claude-code",
                "Bash",
                "ls",
            );
        }
    }
    // Today: 6 calls with megabyte-scale detail strings.
    let huge = "x".repeat(8000);
    for j in 0..6 {
        insert_event(
            &conn,
            now - Duration::minutes(j),
            "claude-code",
            "Bash",
            &huge,
        );
    }
    let detections = detect_anomalies_at(&conn, &tz(), AnomalyWindow::Today, now).unwrap();
    assert!(
        detections.iter().any(|d| d.kind == AnomalyKind::CostPerCallRise),
        "expected CostPerCallRise, got {detections:?}"
    );
}

#[test]
fn cost_per_call_rise_silent_on_steady_costs() {
    let (_d, conn) = open();
    let now = anchor_now();
    // Same short detail today and baseline → ratio ≈ 1.0, no fire.
    for i in 1..=14 {
        for j in 0..6 {
            insert_event(
                &conn,
                now - Duration::days(i) + Duration::minutes(j),
                "claude-code",
                "Bash",
                "ls",
            );
        }
    }
    for j in 0..6 {
        insert_event(&conn, now - Duration::minutes(j), "claude-code", "Bash", "ls");
    }
    let detections = detect_anomalies_at(&conn, &tz(), AnomalyWindow::Today, now).unwrap();
    assert!(
        !detections.iter().any(|d| d.kind == AnomalyKind::CostPerCallRise),
        "did not expect CostPerCallRise, got {detections:?}"
    );
}
