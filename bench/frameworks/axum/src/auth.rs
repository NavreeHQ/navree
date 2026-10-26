use axum::{
    extract::FromRequestParts,
    http::{header::AUTHORIZATION, request::Parts},
};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::Deserialize;
use sqlx::FromRow;
use uuid::Uuid;

use crate::{AppState, error::AppError};

pub struct JwtConfig {
    key: DecodingKey,
    validation: Validation,
}

impl JwtConfig {
    /// HS256 with a required `exp` (checked by default) and a fixed `iss`.
    pub fn new(secret: &str, issuer: &str) -> Self {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[issuer]);
        validation.set_required_spec_claims(&["exp", "iss", "sub"]);
        Self {
            key: DecodingKey::from_secret(secret.as_bytes()),
            validation,
        }
    }
}

#[derive(Deserialize)]
struct Claims {
    sub: Uuid,
}

/// The authenticated user: a valid bearer token whose `sub` is a row in `users`.
#[derive(FromRow)]
pub struct AuthUser {
    pub id: Uuid,
    pub name: String,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .ok_or(AppError::Unauthorized)?;
        let claims = decode::<Claims>(token, &state.jwt.key, &state.jwt.validation)
            .map_err(|_| AppError::Unauthorized)?
            .claims;

        sqlx::query_as("SELECT id, name FROM users WHERE id = $1")
            .bind(claims.sub)
            .fetch_optional(&state.db)
            .await?
            .ok_or(AppError::Unauthorized)
    }
}
