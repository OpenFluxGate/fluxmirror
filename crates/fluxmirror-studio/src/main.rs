//! fluxmirror-studio — local web dashboard over the fluxmirror SQLite store.
//!
//! Runs as a separate process from the `fluxmirror` capture binary.
//! Read-only SQLite. Localhost-bound by default. Single-user.
//!
//! Phase 3 M2 deliverable: boots, opens the DB read-only, exposes
//! `/health` plus `/api/today`, `/api/week`, `/api/now`, and serves
//! the embedded Vite SPA bundle as a fallback for everything else.
//! The router and handlers live in the sibling library so integration
//! tests can build the same wiring. M7 layers a content-type-aware
//! redaction middleware on top of that router for outbound bodies.

mod redact_layer;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use clap::Parser;
use rusqlite::{Connection, OpenFlags};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use fluxmirror_studio::{build_router, AppState};

#[derive(Parser, Debug)]
#[command(version, about = "fluxmirror local web dashboard")]
struct Args {
    /// TCP port to bind on.
    #[arg(long, default_value_t = 7090)]
    port: u16,

    /// IP address to bind on. Default 127.0.0.1. Binding 0.0.0.0 is
    /// opt-in and gives up the localhost-only safety promise.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// SQLite events.db path. Defaults to the platform path used by
    /// the capture binary.
    #[arg(long)]
    db: Option<PathBuf>,

    /// Reserved for the real .fluxmirror.toml parser landing in M9.
    #[arg(long)]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "fluxmirror_studio=info,tower_http=info".into()),
        )
        .with_target(false)
        .init();

    let args = Args::parse();

    let db_path = args
        .db
        .unwrap_or_else(fluxmirror_core::paths::default_db_path);

    if !db_path.exists() {
        eprintln!("error: events.db not found at {}", db_path.display());
        eprintln!("hint: run a Claude/Qwen/Gemini session first to populate it,");
        eprintln!("      or pass --db <path> to point at an existing fluxmirror DB.");
        std::process::exit(1);
    }

    let db = Connection::open_with_flags(
        &db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("failed to open SQLite at {}: {e}", db_path.display()))?;

    // Load config once at boot. Drives both the redaction layer below
    // and the optional AI surface above. Falls back to defaults if the
    // file is missing — the AI surface short-circuits on provider="off".
    let cfg = fluxmirror_core::Config::load().unwrap_or_default();

    // Open a writable store handle for the AI cache + budget layer when
    // a real provider is wired up. Read-only `db` above is kept distinct
    // so dashboard reads don't contend with AI cache writes.
    let ai_store = if cfg.ai.provider != "off" {
        match fluxmirror_store::SqliteStore::open(&db_path) {
            Ok(s) => Some(Arc::new(s)),
            Err(e) => {
                tracing::warn!(
                    "AI cache store unavailable ({e}); session intents will be skipped"
                );
                None
            }
        }
    } else {
        None
    };

    let state = AppState {
        db: Arc::new(Mutex::new(db)),
        db_path: db_path.clone(),
        config: Arc::new(cfg.clone()),
        ai_store,
    };

    // Load redaction rules once at boot. The capture path never goes
    // through this binary; the layer below scrubs only outbound bodies
    // whose Content-Type tags them as text-shaped (HTML / JSON / plain
    // text), leaving CSS / JS / image bytes untouched.
    let redact_rules = Arc::new(fluxmirror_core::redact::from_config(&cfg));

    let app = build_router(state)
        .layer(axum::middleware::from_fn_with_state(
            redact_rules.clone(),
            redact_layer::scrub_response,
        ))
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("listening on http://{}", addr);
    tracing::info!("db: {}", db_path.display());

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install ctrl_c handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
