use std::{future::Future, pin::Pin};

use actix_web::{FromRequest, HttpRequest, dev::Payload, http::header::AUTHORIZATION, web};
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

impl FromRequest for AuthUser {
    type Error = AppError;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        let state = req
            .app_data::<web::Data<AppState>>()
            .expect("AppState is registered")
            .clone();
        let claims = req
            .headers()
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .and_then(|token| decode::<Claims>(token, &state.jwt.key, &state.jwt.validation).ok())
            .map(|data| data.claims);

        Box::pin(async move {
            let claims = claims.ok_or(AppError::Unauthorized)?;
            sqlx::query_as("SELECT id, name FROM users WHERE id = $1")
                .bind(claims.sub)
                .fetch_optional(&state.db)
                .await?
                .ok_or(AppError::Unauthorized)
        })
    }
}
