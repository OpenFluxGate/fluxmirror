//! fluxmirror-studio — local web dashboard over the fluxmirror SQLite store.
//!
//! Runs as a separate process from the `fluxmirror` capture binary.
//! Read-only SQLite. Localhost-bound by default. Single-user.
//!
//! Phase 3 M1 deliverable: boots, opens the DB read-only, exposes a
//! tiny health endpoint, serves the embedded Vite SPA bundle. Real
//! API routes land in M2 onward.

mod embed;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::{
    body::Body,
    extract::State,
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use clap::Parser;
use rusqlite::{Connection, OpenFlags};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

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

#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<Connection>>,
    db_path: PathBuf,
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

    let state = AppState {
        db: Arc::new(Mutex::new(db)),
        db_path: db_path.clone(),
    };

    let app = Router::new()
        .route("/health", get(health))
        .fallback(static_handler)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("listening on http://{}", addr);
    tracing::info!("db: {}", db_path.display());

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let agent_events: i64 = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.query_row("SELECT COUNT(*) FROM agent_events", [], |row| row.get(0))
            .unwrap_or(0)
    };
    let proxy_events: i64 = {
        let db = state.db.lock().expect("db mutex poisoned");
        db.query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
            .unwrap_or(0)
    };
    axum::Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "db": state.db_path.display().to_string(),
        "agent_events": agent_events,
        "proxy_events": proxy_events,
    }))
}

/// Serve embedded Vite assets, falling back to index.html for SPA routes.
async fn static_handler(uri: Uri) -> Response {
    let path = uri.path();

    if let Some((bytes, mime)) = embed::lookup(path) {
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime)
            .header(header::CACHE_CONTROL, "public, max-age=3600")
            .body(Body::from(bytes))
            .expect("static response build");
    }

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(embed::index_html()))
        .expect("index response build")
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
