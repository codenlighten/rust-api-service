//! Bearer-token authentication as an extractor: a handler that takes [`AuthUser`] cannot run
//! without a valid, unexpired token.

use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;

use crate::app::{now_secs, AppState};
use crate::error::ApiError;
use crate::token;

/// The authenticated user's id, taken from the verified token.
#[derive(Debug, Clone, Copy)]
pub struct AuthUser(pub i64);

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(AUTHORIZATION)
            .ok_or(ApiError::Unauthorized("missing Authorization header; send 'Authorization: Bearer <token>' from POST /auth/login"))?
            .to_str()
            .map_err(|_| ApiError::Unauthorized("Authorization header is not valid text"))?;
        let raw = header
            .strip_prefix("Bearer ")
            .ok_or(ApiError::Unauthorized(
                "Authorization header must use the Bearer scheme",
            ))?;

        match token::verify(&state.token_secret, raw.trim(), now_secs()) {
            Ok(claims) => Ok(Self(claims.sub)),
            Err(token::TokenError::Expired) => {
                Err(ApiError::Unauthorized("token has expired; log in again"))
            }
            Err(_) => Err(ApiError::Unauthorized("token is invalid; log in again")),
        }
    }
}
