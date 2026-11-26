use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};

use crate::app::AppState;
use crate::error::ApiError;

/// Reports healthy only if the tables the service needs can be read. A bare `SELECT 1` would
/// succeed without touching the database file.
pub async fn healthz(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    sqlx::query("SELECT (SELECT 1 FROM users LIMIT 1), (SELECT 1 FROM notes LIMIT 1)")
        .execute(&state.pool)
        .await
        // Whatever is wrong with the database, the answer to this question is "not healthy".
        .map_err(|err| {
            tracing::error!(error = %err, "health check failed");
            ApiError::Unavailable
        })?;
    Ok(Json(json!({ "status": "ok" })))
}
