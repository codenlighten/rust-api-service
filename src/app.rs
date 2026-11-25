//! Application state and router assembly.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderName, Request};
use axum::routing::{get, post};
use axum::Router;
use sqlx::SqlitePool;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

use crate::error::ApiError;
use crate::password::Hasher;
use crate::routes;

const MAX_BODY_BYTES: usize = 64 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub hasher: Arc<Hasher>,
    pub token_secret: Arc<[u8]>,
    pub token_ttl_secs: u64,
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn router(state: AppState) -> Router {
    let request_id = HeaderName::from_static("x-request-id");

    Router::new()
        .route("/healthz", get(routes::health::healthz))
        .route("/openapi.json", get(crate::openapi::spec))
        .route("/auth/register", post(routes::accounts::register))
        .route("/auth/login", post(routes::accounts::login))
        .route("/notes", post(routes::notes::create).get(routes::notes::list))
        .route(
            "/notes/{id}",
            get(routes::notes::fetch).put(routes::notes::replace).delete(routes::notes::remove),
        )
        .fallback(|| async { ApiError::NotFound })
        .method_not_allowed_fallback(|| async { ApiError::MethodNotAllowed })
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(PropagateRequestIdLayer::new(request_id.clone()))
        .layer(
            TraceLayer::new_for_http().make_span_with(|req: &Request<_>| {
                let id = req
                    .headers()
                    .get("x-request-id")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("-");
                tracing::info_span!("http", method = %req.method(), path = %req.uri().path(), request_id = %id)
            }),
        )
        .layer(SetRequestIdLayer::new(request_id, MakeRequestUuid))
        .with_state(state)
}
