// Integration tests for the AI service layer.
//
// Covers the 14 named cases from the M-A1 task spec:
//
//   - budget_resets_at_midnight
//   - budget_blocks_when_over_cap
//   - cache_round_trip
//   - cache_key_changes_with_prompt_version
//   - redact_outbound_covers_aws_key
//   - redact_outbound_covers_github_pat
//   - redact_outbound_covers_env_path
//   - redact_outbound_covers_kv_secret
//   - redact_outbound_replaces_home_with_tilde
//   - redact_outbound_truncates_long_messages
//   - synthesise_returns_cached_on_second_call (mocked provider via cache)
//   - synthesise_falls_back_when_provider_off
//   - provider_anthropic_parses_response_shape (mock server)
//   - provider_ollama_unreachable_returns_clean_error

use std::path::PathBuf;
use std::sync::Mutex;

use chrono::Local;
use serde_json::json;
use tempfile::TempDir;

use fluxmirror_ai::budget::Budget;
use fluxmirror_ai::cache::{insert as cache_insert, lookup as cache_lookup, make_cache_key};
use fluxmirror_ai::provider::{anthropic::AnthropicProvider, OllamaProvider, Provider};
use fluxmirror_ai::redact_outbound::redact_outbound;
use fluxmirror_ai::types::{AiError, LlmRequest};
use fluxmirror_ai::{synthesise, SynthOptions};

use fluxmirror_core::config::Config;
use fluxmirror_core::redact::default_rules;
use fluxmirror_store::SqliteStore;

// Tests in this file mutate process-global env vars (HOME, etc). Run them
// under a single mutex so parallel `cargo test` doesn't see torn state.
static ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}
impl EnvGuard {
    fn set(key: &'static str, val: &str) -> Self {
        let prior = std::env::var_os(key);
        std::env::set_var(key, val);
        Self { key, prior }
    }
    fn unset(key: &'static str) -> Self {
        let prior = std::env::var_os(key);
        std::env::remove_var(key);
        Self { key, prior }
    }
}
impl Drop for EnvGuard {
    fn drop(&mut self) {
        match self.prior.take() {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}

fn fresh_store(dir: &TempDir) -> (PathBuf, SqliteStore) {
    let path = dir.path().join("events.db");
    let store = SqliteStore::open(&path).expect("store opens");
    (path, store)
}

// ===== budget =================================================================

#[test]
fn budget_resets_at_midnight() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let tmp = TempDir::new().unwrap();
    let b = Budget::new(tmp.path().to_path_buf(), 1.00);

    // Hand-write yesterday's ledger; today should still report 0.0.
    let yesterday = Local::now()
        .date_naive()
        .pred_opt()
        .expect("yesterday exists");
    let stale = tmp
        .path()
        .join(format!("ai-budget-{}.txt", yesterday.format("%Y-%m-%d")));
    std::fs::write(&stale, "0.99\n").unwrap();

    assert_eq!(
        b.current_spend(),
        0.0,
        "yesterday's file must not contribute to today"
    );
    // And reservation against the cap works fresh.
    b.check_and_reserve(0.50).expect("under cap");
}

#[test]
fn budget_blocks_when_over_cap() {
    let tmp = TempDir::new().unwrap();
    let b = Budget::new(tmp.path().to_path_buf(), 0.10);
    b.record(0.08).unwrap();
    assert!(b.check_and_reserve(0.01).is_ok());
    match b.check_and_reserve(0.05) {
        Err(AiError::BudgetExceeded) => (),
        other => panic!("expected BudgetExceeded, got {other:?}"),
    }
}

// ===== cache =================================================================

#[test]
fn cache_round_trip() {
    let tmp = TempDir::new().unwrap();
    let (path, _store) = fresh_store(&tmp);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let key = make_cache_key("claude-haiku-4-5", "sys", "u", 1);
    cache_insert(&conn, &key, "ok", 0.0001, "claude-haiku-4-5", "anthropic").unwrap();

    let hit = cache_lookup(&conn, &key, 7).unwrap().expect("hit");
    assert!(hit.cache_hit);
    assert_eq!(hit.text, "ok");
    assert_eq!(hit.cost_usd, 0.0);

    // Aged row eviction: rewrite created_at into the past beyond the TTL.
    conn.execute(
        "UPDATE ai_cache SET created_at = 0 WHERE key = ?1",
        rusqlite::params![key],
    )
    .unwrap();
    assert!(cache_lookup(&conn, &key, 7).unwrap().is_none());
}

#[test]
fn cache_key_changes_with_prompt_version() {
    let v1 = make_cache_key("m", "s", "u", 1);
    let v2 = make_cache_key("m", "s", "u", 2);
    assert_ne!(v1, v2);
    assert_eq!(v1.len(), 64);
}

// ===== redact_outbound =======================================================

#[test]
fn redact_outbound_covers_aws_key() {
    let r = default_rules();
    let out = redact_outbound("AKIAIOSFODNN7EXAMPLE", &r, 8192);
    assert!(out.contains("[REDACTED:aws_key]"));
    assert!(!out.contains("AKIAIOSFODNN7EXAMPLE"));
}

#[test]
fn redact_outbound_covers_github_pat() {
    let r = default_rules();
    let out = redact_outbound("ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", &r, 8192);
    assert!(out.contains("[REDACTED:"));
    assert!(!out.contains("ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
}

#[test]
fn redact_outbound_covers_env_path() {
    let r = default_rules();
    let out = redact_outbound("loaded .env from cwd", &r, 8192);
    assert!(out.contains("[REDACTED:env_path]"));
}

#[test]
fn redact_outbound_covers_kv_secret() {
    let r = default_rules();
    let out = redact_outbound("password=hunter2 in code", &r, 8192);
    assert!(out.contains("[REDACTED:kv_secret]"));
    assert!(!out.contains("hunter2"));
}

#[test]
fn redact_outbound_replaces_home_with_tilde() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().to_str().unwrap();
    let _h = EnvGuard::set("HOME", home);
    let _u = EnvGuard::unset("USERPROFILE");

    let input = format!("touched {home}/Documents/notes.md");
    let r = default_rules();
    let out = redact_outbound(&input, &r, 8192);
    assert!(out.contains("~/Documents/notes.md"), "got {out}");
    assert!(!out.contains(home));
}

#[test]
fn redact_outbound_truncates_long_messages() {
    let r = default_rules();
    let long = "x".repeat(20_000);
    let out = redact_outbound(&long, &r, 1024);
    assert!(out.chars().count() <= 1024);
    assert!(out.ends_with("[truncated to fit prompt budget]"));
}

// ===== synthesise ===========================================================

#[test]
fn synthesise_returns_cached_on_second_call() {
    let tmp = TempDir::new().unwrap();
    let (db_path, store) = fresh_store(&tmp);

    // Pre-seed the cache with a row that exactly matches what
    // synthesise() will derive from the daily prompt + ctx.
    let mut cfg = Config::default();
    cfg.ai.provider = "anthropic".into();
    cfg.ai.cache_ttl_days = 7;
    cfg.ai.max_user_chars = 8192;
    cfg.ai.default_model = "claude-haiku-4-5-20251001".into();

    let ctx = json!({
        "agent_total": 384,
        "top_tool": "Bash",
        "summary_window": "yesterday",
        "session_count": 3,
        "edit_to_read_ratio": "0.25",
        "primary_languages": "Rust",
    });

    // Render the prompt the same way synthesise() does, then reuse the
    // redact_outbound + version digest to compute the expected key.
    let (system, user_raw) =
        fluxmirror_ai::prompts::render_prompt("daily", &ctx).unwrap();
    let rules = fluxmirror_core::redact::from_config(&cfg);
    let user = redact_outbound(&user_raw, &rules, cfg.ai.max_user_chars);
    let version = fluxmirror_ai::prompts::version_of("daily").unwrap();
    let key = make_cache_key(&cfg.ai.default_model, &system, &user, version);

    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        cache_insert(
            &conn,
            &key,
            "Yesterday: shipped a small thing.",
            0.0042,
            &cfg.ai.default_model,
            "anthropic",
        )
        .unwrap();
    }

    let opts = SynthOptions::for_default_model(&cfg);
    let resp = synthesise(&store, &cfg, "daily", &ctx, opts).expect("cache hit");
    assert!(resp.cache_hit);
    assert_eq!(resp.text, "Yesterday: shipped a small thing.");
    assert_eq!(resp.provider, "anthropic");
    assert_eq!(resp.cost_usd, 0.0);
}

#[test]
fn synthesise_falls_back_when_provider_off() {
    let tmp = TempDir::new().unwrap();
    let (_path, store) = fresh_store(&tmp);
    let mut cfg = Config::default();
    cfg.ai.provider = "off".into();

    let ctx = json!({
        "agent_total": 1,
        "top_tool": "Bash",
        "summary_window": "yesterday",
        "session_count": 1,
        "edit_to_read_ratio": "0.0",
        "primary_languages": "—",
    });

    let opts = SynthOptions::for_default_model(&cfg);
    match synthesise(&store, &cfg, "daily", &ctx, opts) {
        Err(AiError::ProviderNotImplemented) => (),
        other => panic!("expected ProviderNotImplemented, got {other:?}"),
    }
}

// ===== provider — Anthropic via mockito =====================================

#[test]
fn provider_anthropic_parses_response_shape() {
    let mut server = mockito::Server::new();
    let body = serde_json::json!({
        "id": "msg_abc",
        "type": "message",
        "role": "assistant",
        "model": "claude-haiku-4-5-20251001",
        "content": [{"type": "text", "text": "tested ok"}],
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 21,
            "output_tokens": 9,
            "cache_creation_input_tokens": 0,
            "cache_read_input_tokens": 0
        }
    });
    let _m = server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body.to_string())
        .create();

    let p = AnthropicProvider::with_base("test-key", server.url());
    let req = LlmRequest {
        model: "claude-haiku-4-5-20251001".into(),
        system: "be terse".into(),
        user: "ping".into(),
        max_tokens: 64,
        cache_key: "k".into(),
    };
    let r = p.complete(&req).expect("provider returns ok");
    assert_eq!(r.text, "tested ok");
    assert_eq!(r.provider, "anthropic");
    assert_eq!(r.tokens_in, 21);
    assert_eq!(r.tokens_out, 9);
    assert!(r.cost_usd > 0.0);
    assert!(!r.cache_hit);
}

// ===== provider — Ollama =====================================================

#[test]
fn provider_ollama_unreachable_returns_clean_error() {
    let p = OllamaProvider::with_base("http://127.0.0.1:1");
    let req = LlmRequest {
        model: "llama3".into(),
        system: "be terse".into(),
        user: "hi".into(),
        max_tokens: 32,
        cache_key: "k".into(),
    };
    match p.complete(&req) {
        Err(AiError::ProviderUnreachable(_)) => (),
        other => panic!("expected unreachable, got {other:?}"),
    }
}
