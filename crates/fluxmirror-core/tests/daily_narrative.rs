// Phase 4 M-A2 — heuristic fallback contract.
//
// Property under test: `heuristic_paragraph(today)` returns a non-empty
// 2–3 sentence paragraph for any `TodayData`, including the degenerate
// empty-window case. The deterministic snapshot test below also pins
// the exact output for a fixture so future tweaks don't silently drift
// the wording.

use chrono::NaiveDate;

use fluxmirror_core::report::ai_narrative::heuristic_paragraph;
use fluxmirror_core::report::data::build_daily_summary_input;
use fluxmirror_core::report::dto::{
    AgentCount, FileTouch, ShellEvent, TodayData, ToolMixEntry,
};

fn busy_fixture() -> TodayData {
    let mut t = TodayData {
        date: NaiveDate::from_ymd_opt(2026, 4, 26).unwrap(),
        tz: "UTC".to_string(),
        total_events: 240,
        writes_total: 80,
        reads_total: 110,
        ..Default::default()
    };
    t.agents = vec![
        AgentCount {
            agent: "claude-code".into(),
            calls: 200,
            sessions: vec!["s1".into(), "s2".into()],
            active_days: vec![],
            top_tool: "Edit".into(),
        },
        AgentCount {
            agent: "gemini-cli".into(),
            calls: 40,
            sessions: vec!["g1".into()],
            active_days: vec![],
            top_tool: "edit_file".into(),
        },
    ];
    t.files_edited = vec![FileTouch {
        path: "crates/fluxmirror-core/src/report/data.rs".into(),
        tool: "Edit".into(),
        count: 22,
    }];
    t.tool_mix = vec![
        ToolMixEntry {
            tool: "Edit".into(),
            count: 80,
        },
        ToolMixEntry {
            tool: "Read".into(),
            count: 60,
        },
    ];
    t.distinct_files = vec![
        "crates/fluxmirror-core/src/report/data.rs".into(),
        "crates/fluxmirror-ai/src/daily_narrative.rs".into(),
    ];
    t.shells = vec![ShellEvent {
        time_local: "10:00".into(),
        detail: "cargo test".into(),
        ts_utc: "2026-04-26T10:00:00Z".parse().unwrap(),
    }];
    t
}

#[test]
fn heuristic_paragraph_is_non_empty_for_busy_fixture() {
    let p = heuristic_paragraph(&busy_fixture());
    assert!(!p.is_empty(), "expected non-empty paragraph");
    assert!(
        p.contains("claude-code"),
        "should mention dominant agent: {p}"
    );
    assert!(p.contains("Edit"), "should mention dominant tool: {p}");
}

#[test]
fn heuristic_paragraph_is_non_empty_for_empty_window() {
    let t = TodayData {
        date: NaiveDate::from_ymd_opt(2026, 4, 26).unwrap(),
        tz: "UTC".to_string(),
        ..Default::default()
    };
    let p = heuristic_paragraph(&t);
    assert!(!p.is_empty());
    assert!(p.contains("2026-04-26"));
}

#[test]
fn heuristic_paragraph_is_deterministic_snapshot() {
    // Snapshot pin: the same fixture produces the same paragraph
    // byte-for-byte. If you intentionally tweak the wording, refresh
    // this expected string.
    let expected =
        "A read-and-polish pass on 2026-04-26 (UTC) — 240 calls through claude-code and gemini-cli. \
         80 writes on 1 file against 110 reads on 0 files (edit/read 0.73). \
         Dominant tool was Edit (80 calls); the spine file was crates/fluxmirror-core/src/report/data.rs.";
    let got = heuristic_paragraph(&busy_fixture());
    assert_eq!(got, expected, "snapshot drift:\n  got: {got}\n  expected: {expected}");
}

#[test]
fn build_daily_summary_input_carries_required_keys() {
    let ctx = build_daily_summary_input(&busy_fixture());
    let obj = ctx.as_object().expect("ctx should be an object");
    for key in [
        "agent_total",
        "session_count",
        "top_tool",
        "edit_to_read_ratio",
        "primary_languages",
        "summary_window",
        "date",
        "tz",
        "writes_total",
        "reads_total",
        "distinct_file_count",
        "lifecycle_hint",
        "top_files_json",
        "agents_json",
        "shell_signatures_json",
    ] {
        assert!(obj.contains_key(key), "missing key: {key}");
    }
    assert_eq!(obj.get("agent_total").and_then(|v| v.as_u64()), Some(240));
    assert_eq!(obj.get("top_tool").and_then(|v| v.as_str()), Some("Edit"));
    assert_eq!(obj.get("tz").and_then(|v| v.as_str()), Some("UTC"));
}

#[test]
fn build_daily_summary_input_handles_zero_reads() {
    let mut t = busy_fixture();
    t.reads_total = 0;
    let ctx = build_daily_summary_input(&t);
    // edit_to_read_ratio falls back to "n/a" rather than dividing by zero.
    assert_eq!(
        ctx.get("edit_to_read_ratio").and_then(|v| v.as_str()),
        Some("n/a")
    );
}
