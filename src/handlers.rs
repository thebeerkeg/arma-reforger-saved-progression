use crate::db::{AnyStore, Store};
use crate::error::{BridgeError, BridgeResult};
use crate::models::{BatchIncrementRequest, IncrementRequest, PlayerRecord};
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::Html,
    Json,
};
use serde::Deserialize;
use std::sync::Arc;

pub struct AppState {
    pub store: AnyStore,
    pub api_key: String,
    pub dashboard_html: String,
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub fn render_dashboard_html(title: &str, subtitle: &str) -> String {
    include_str!("dashboard.html")
        .replace("{{TITLE}}", &html_escape(title))
        .replace("{{SUBTITLE}}", &html_escape(subtitle))
}

pub type SharedState = Arc<AppState>;

#[derive(Deserialize, Default)]
pub struct AuthQuery {
    #[serde(default)]
    pub api_key: Option<String>,
}

fn check_auth(headers: &HeaderMap, query_key: Option<&str>, expected: &str) -> BridgeResult<()> {
    // Prefer X-Api-Key header (works for direct curl testing), fall back to ?api_key=
    // query param. The Reforger addon must use the query param — Reforger's RestApi
    // strips all custom headers, so SetHeaders("X-Api-Key: ...") never reaches us.
    let provided = headers
        .get("X-Api-Key")
        .and_then(|v| v.to_str().ok())
        .or(query_key)
        .unwrap_or("");
    if provided == expected {
        Ok(())
    } else {
        Err(BridgeError::Unauthorized)
    }
}

pub async fn health() -> &'static str {
    "ok"
}

pub async fn get_player(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(auth): Query<AuthQuery>,
    Path(uid): Path<String>,
) -> BridgeResult<Json<PlayerRecord>> {
    let header_keys: Vec<&str> = headers.keys().map(|k| k.as_str()).collect();
    tracing::debug!(
        target: "tbk::req",
        method = "GET",
        path = format!("/player/{uid}"),
        query_api_key_present = auth.api_key.is_some(),
        headers = ?header_keys,
        "incoming request"
    );
    check_auth(&headers, auth.api_key.as_deref(), &state.api_key)?;
    let rec = state
        .store
        .get_player(&uid)
        .await?
        .ok_or_else(|| BridgeError::NotFound(uid.clone()))?;
    Ok(Json(rec))
}

pub async fn upsert_player(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(auth): Query<AuthQuery>,
    Path(uid): Path<String>,
    body: Bytes,
) -> BridgeResult<Json<PlayerRecord>> {
    let header_keys: Vec<&str> = headers.keys().map(|k| k.as_str()).collect();
    tracing::debug!(
        target: "tbk::req",
        method = "POST",
        path = format!("/player/{uid}/increment"),
        query_api_key_present = auth.api_key.is_some(),
        headers = ?header_keys,
        "incoming request"
    );
    check_auth(&headers, auth.api_key.as_deref(), &state.api_key)?;
    // Reforger's RestApi strips Content-Type, so axum's Json<T> extractor would 415 us
    // before we ever see the body. Parse from raw bytes instead.
    let req: IncrementRequest = serde_json::from_slice(&body)
        .map_err(|e| BridgeError::BadRequest(format!("invalid JSON body: {e}")))?;
    if req.last_known_name.trim().is_empty() {
        return Err(BridgeError::BadRequest("last_known_name is empty".into()));
    }
    let rec = state
        .store
        .upsert_increment(&uid, &req.last_known_name, &req.delta)
        .await?;
    Ok(Json(rec))
}

pub async fn batch_increment(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(auth): Query<AuthQuery>,
    body: Bytes,
) -> BridgeResult<Json<serde_json::Value>> {
    let header_keys: Vec<&str> = headers.keys().map(|k| k.as_str()).collect();
    tracing::debug!(
        target: "tbk::req",
        method = "POST",
        path = "/player/batch-increment",
        query_api_key_present = auth.api_key.is_some(),
        headers = ?header_keys,
        "incoming request"
    );
    check_auth(&headers, auth.api_key.as_deref(), &state.api_key)?;
    let req: BatchIncrementRequest = serde_json::from_slice(&body)
        .map_err(|e| BridgeError::BadRequest(format!("invalid JSON body: {e}")))?;
    let n = state.store.batch_upsert_increment(&req.entries).await?;
    Ok(Json(serde_json::json!({ "applied": n })))
}

#[derive(Deserialize)]
pub struct LeaderboardQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub api_key: Option<String>,
}
fn default_limit() -> i64 {
    100
}

pub async fn leaderboard(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(q): Query<LeaderboardQuery>,
) -> BridgeResult<Json<serde_json::Value>> {
    check_auth(&headers, q.api_key.as_deref(), &state.api_key)?;
    let limit = q.limit.clamp(1, 1000);
    let rows = state.store.leaderboard(limit).await?;
    Ok(Json(serde_json::json!({ "entries": rows })))
}

// Public dashboard: serves the embedded single-page UI. No auth — read-only,
// derived from the same data the leaderboard endpoint exposes. The HTML is
// pre-rendered once at startup with the configured title/subtitle.
pub async fn dashboard_page(State(state): State<SharedState>) -> Html<String> {
    Html(state.dashboard_html.clone())
}

// Public aggregate payload consumed by the dashboard tiles.
pub async fn api_stats(
    State(state): State<SharedState>,
) -> BridgeResult<Json<serde_json::Value>> {
    let aggregate = state.store.aggregate_stats().await?;
    Ok(Json(serde_json::json!({ "aggregate": aggregate })))
}

#[derive(Deserialize)]
pub struct ApiLeaderboardQuery {
    #[serde(default = "default_api_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    #[serde(default)]
    pub q: Option<String>,
}
fn default_api_limit() -> i64 {
    25
}

// Public paginated leaderboard with optional name search. Echoes back the
// effective limit/offset so the dashboard doesn't have to track its own clamps.
pub async fn api_leaderboard(
    State(state): State<SharedState>,
    Query(q): Query<ApiLeaderboardQuery>,
) -> BridgeResult<Json<serde_json::Value>> {
    let limit = q.limit.clamp(1, 100);
    let offset = q.offset.max(0);
    let search = q.q.as_deref().filter(|s| !s.trim().is_empty());
    let (entries, total) = state.store.leaderboard_paged(limit, offset, search).await?;
    Ok(Json(serde_json::json!({
        "entries": entries,
        "total": total,
        "limit": limit,
        "offset": offset,
    })))
}

// Public player lookup for the dashboard's detail modal. Mirrors GET /player/:uid
// but without the api_key gate — the dashboard itself is public.
pub async fn api_player(
    State(state): State<SharedState>,
    Path(uid): Path<String>,
) -> BridgeResult<Json<PlayerRecord>> {
    let rec = state
        .store
        .get_player(&uid)
        .await?
        .ok_or_else(|| BridgeError::NotFound(uid.clone()))?;
    Ok(Json(rec))
}
