//! Registration and login.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::app::{now_secs, AppState};
use crate::error::{ApiError, FieldError};
use crate::extract::ApiJson;
use crate::{token, validate};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    email: String,
    password: String,
}

#[derive(Serialize)]
pub struct Registered {
    id: i64,
    email: String,
}

#[derive(Serialize)]
pub struct Session {
    access_token: String,
    token_type: &'static str,
    expires_in: u64,
}

pub async fn register(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<Credentials>,
) -> Result<(StatusCode, Json<Registered>), ApiError> {
    let mut errors: Vec<FieldError> = Vec::new();
    let email = validate::email(&body.email)
        .map_err(|e| errors.push(e))
        .ok();
    if let Err(e) = validate::password(&body.password) {
        errors.push(e);
    }
    let Some(email) = email.filter(|_| errors.is_empty()) else {
        return Err(ApiError::Validation(errors));
    };

    let hasher = state.hasher.clone();
    let password = body.password;
    let hash = tokio::task::spawn_blocking(move || hasher.hash(&password))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let inserted = sqlx::query_scalar::<_, i64>(
        "INSERT INTO users (email, password_hash, created_at) VALUES (?, ?, ?) RETURNING id",
    )
    .bind(&email)
    .bind(&hash)
    .bind(now_secs() as i64)
    .fetch_one(&state.pool)
    .await;

    match inserted {
        Ok(id) => Ok((StatusCode::CREATED, Json(Registered { id, email }))),
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => Err(ApiError::Conflict(
            "an account with this email already exists; log in instead",
        )),
        Err(e) => Err(e.into()),
    }
}

pub async fn login(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<Credentials>,
) -> Result<Json<Session>, ApiError> {
    const REFUSED: ApiError = ApiError::Unauthorized("invalid email or password");

    let email = body.email.trim().to_lowercase();
    let row: Option<(i64, String)> =
        sqlx::query_as("SELECT id, password_hash FROM users WHERE email = ?")
            .bind(&email)
            .fetch_optional(&state.pool)
            .await?;

    let hasher = state.hasher.clone();
    let password = body.password;
    let user_id = tokio::task::spawn_blocking(move || match row {
        Some((id, hash)) => hasher.verify(&password, &hash).then_some(id),
        None => {
            hasher.verify_decoy(&password);
            None
        }
    })
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?;

    let Some(user_id) = user_id else {
        return Err(REFUSED);
    };

    let claims = token::Claims {
        sub: user_id,
        exp: now_secs() + state.token_ttl_secs,
    };
    Ok(Json(Session {
        access_token: token::sign(&state.token_secret, &claims),
        token_type: "Bearer",
        expires_in: state.token_ttl_secs,
    }))
}
