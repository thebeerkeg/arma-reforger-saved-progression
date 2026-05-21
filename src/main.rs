mod config;
mod db;
mod error;
mod handlers;
mod models;

use anyhow::{Context, Result};
use axum::{
    routing::{get, post},
    Router,
};
use config::Config;
use db::Store;
use handlers::{AppState, SharedState};
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let mut args = std::env::args().skip(1);
    let mut config_path = PathBuf::from("config.toml");
    while let Some(a) = args.next() {
        match a.as_str() {
            "--config" => {
                config_path = PathBuf::from(args.next().context("--config needs a path")?);
            }
            other => anyhow::bail!("unknown argument: {other}"),
        }
    }

    let cfg = Config::load(&config_path)?;
    tracing::info!("loaded config from {}", config_path.display());
    tracing::info!("database backend: {:?}", cfg.database.backend);

    let store = db::build_store(&cfg).await?;
    store.migrate().await?;
    tracing::info!("database ready");

    let dashboard_html =
        handlers::render_dashboard_html(&cfg.dashboard.title, &cfg.dashboard.subtitle);

    let state: SharedState = Arc::new(AppState {
        store,
        api_key: cfg.server.api_key.clone(),
        dashboard_html,
    });

    let app = Router::new()
        .route("/", get(handlers::dashboard_page))
        .route("/api/stats", get(handlers::api_stats))
        .route("/api/leaderboard", get(handlers::api_leaderboard))
        .route("/api/player/:uid", get(handlers::api_player))
        .route("/api/matches", get(handlers::api_matches))
        .route("/api/match/:id", get(handlers::api_match))
        .route("/health", get(handlers::health))
        .route("/player/:uid", get(handlers::get_player))
        .route("/player/:uid/increment", post(handlers::upsert_player))
        .route("/player/batch-increment", post(handlers::batch_increment))
        .route("/leaderboard", get(handlers::leaderboard))
        .route("/match/finalize", post(handlers::finalize_match))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&cfg.server.bind_address)
        .await
        .with_context(|| format!("binding {}", cfg.server.bind_address))?;
    tracing::info!("TBK Custom Ranks Bridge listening on {}", cfg.server.bind_address);
    axum::serve(listener, app).await?;
    Ok(())
}
