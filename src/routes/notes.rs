//! Notes CRUD. Every query is scoped by the authenticated user's id; a note that belongs to
//! someone else is reported as 404, exactly like one that does not exist.

use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::app::{now_secs, AppState};
use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::extract::{ApiJson, ApiPath, ApiQuery};
use crate::validate;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Note {
    id: i64,
    title: String,
    body: String,
    created_at: i64,
    updated_at: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteInput {
    title: String,
    #[serde(default)]
    body: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    limit: Option<i64>,
    after: Option<i64>,
}

#[derive(Serialize)]
pub struct Page {
    items: Vec<Note>,
    /// Pass as `after` to get the next page; null on the last page.
    next_after: Option<i64>,
}

const COLUMNS: &str = "id, title, body, created_at, updated_at";

pub async fn create(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    ApiJson(input): ApiJson<NoteInput>,
) -> Result<(StatusCode, HeaderMap, Json<Note>), ApiError> {
    let (title, body) = validate::note(&input.title, &input.body).map_err(ApiError::Validation)?;
    let now = now_secs() as i64;
    let note: Note = sqlx::query_as(&format!(
        "INSERT INTO notes (user_id, title, body, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?) RETURNING {COLUMNS}"
    ))
    .bind(user_id)
    .bind(title)
    .bind(body)
    .bind(now)
    .bind(now)
    .fetch_one(&state.pool)
    .await?;

    let mut headers = HeaderMap::new();
    headers.insert(
        header::LOCATION,
        HeaderValue::from_str(&format!("/notes/{}", note.id))
            .map_err(|e| ApiError::Internal(e.to_string()))?,
    );
    Ok((StatusCode::CREATED, headers, Json(note)))
}

/// Keyset pagination: stable under concurrent inserts and constant cost per page.
pub async fn list(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    ApiQuery(params): ApiQuery<ListParams>,
) -> Result<Json<Page>, ApiError> {
    let limit = validate::limit(params.limit);
    let after = validate::after(params.after);
    let (limit, after) = match (limit, after) {
        (Ok(l), Ok(a)) => (l, a),
        (l, a) => {
            return Err(ApiError::Validation(
                [l.err(), a.err()].into_iter().flatten().collect(),
            ))
        }
    };

    let mut items: Vec<Note> = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM notes WHERE user_id = ? AND id > ? ORDER BY id LIMIT ?"
    ))
    .bind(user_id)
    .bind(after)
    .bind(limit + 1)
    .fetch_all(&state.pool)
    .await?;

    let next_after = if items.len() as i64 > limit {
        items.truncate(limit as usize);
        items.last().map(|n| n.id)
    } else {
        None
    };
    Ok(Json(Page { items, next_after }))
}

pub async fn fetch(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    ApiPath(id): ApiPath<i64>,
) -> Result<Json<Note>, ApiError> {
    sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM notes WHERE id = ? AND user_id = ?"
    ))
    .bind(id)
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?
    .map(Json)
    .ok_or(ApiError::NotFound)
}

pub async fn replace(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    ApiPath(id): ApiPath<i64>,
    ApiJson(input): ApiJson<NoteInput>,
) -> Result<Json<Note>, ApiError> {
    let (title, body) = validate::note(&input.title, &input.body).map_err(ApiError::Validation)?;
    sqlx::query_as(&format!(
        "UPDATE notes SET title = ?, body = ?, updated_at = ? \
         WHERE id = ? AND user_id = ? RETURNING {COLUMNS}"
    ))
    .bind(title)
    .bind(body)
    .bind(now_secs() as i64)
    .bind(id)
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?
    .map(Json)
    .ok_or(ApiError::NotFound)
}

pub async fn remove(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    ApiPath(id): ApiPath<i64>,
) -> Result<StatusCode, ApiError> {
    let result = sqlx::query("DELETE FROM notes WHERE id = ? AND user_id = ?")
        .bind(id)
        .bind(user_id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        Err(ApiError::NotFound)
    } else {
        Ok(StatusCode::NO_CONTENT)
    }
}
