//! Bearer-token authentication for Navis's access tokens.
//!
//! Loco's `auth::JWT` and `auth::JWTWithUser` extractors validate Loco's own
//! token: HS512 by default, a base64-encoded secret and a `pid` claim, with no
//! issuer check. Navis signs HS256 with the raw secret and names the user in
//! `sub`, so this extractor keeps Loco's token lookup (`auth.jwt.location`,
//! the bearer header by default) and Loco's secret (`auth.jwt.secret`), and
//! decodes Navis's claims with `jsonwebtoken`, the crate `loco_rs::auth::jwt`
//! wraps. Like `JWTWithUser`, it then loads the user row and answers 401 when
//! there is none.
use axum::{extract::FromRequestParts, http::request::Parts};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use loco_rs::{
    config::Config,
    controller::extractor::auth::{extract_token, get_jwt_from_config},
    prelude::*,
};
use serde::Deserialize;

use crate::{data::settings::Settings, models::users};

/// The claims this app reads. `exp` and `iss` are checked while decoding;
/// `iat`, `jti` and `typ` are carried but not needed.
#[derive(Debug, Deserialize)]
pub struct Claims {
    pub sub: Uuid,
}

/// The decoding key and rules, built once at boot (`Hooks::after_context`)
/// and kept in the shared store.
pub struct TokenVerifier {
    key: DecodingKey,
    validation: Validation,
}

impl TokenVerifier {
    /// HS256 with `auth.jwt.secret`; `exp`, `iss` and `sub` are required and
    /// `iss` must equal `settings.jwt_issuer`.
    ///
    /// # Errors
    /// When `auth.jwt` or `settings` is missing from the config.
    pub fn from_config(config: &Config) -> Result<Self> {
        let jwt = config.get_jwt_config()?;
        let settings = Settings::from_config(config)?;

        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[settings.jwt_issuer]);
        validation.set_required_spec_claims(&["exp", "iss", "sub"]);
        // As `loco_rs::auth::jwt::JWT::validate`: an expired token is expired.
        validation.leeway = 0;

        Ok(Self {
            key: DecodingKey::from_secret(jwt.secret.as_bytes()),
            validation,
        })
    }

    fn verify(&self, token: &str) -> Result<Claims> {
        decode::<Claims>(token, &self.key, &self.validation)
            .map(|data| data.claims)
            .map_err(|err| Error::Unauthorized(err.to_string()))
    }
}

/// The signed-in user: a valid access token whose `sub` is a row in `users`.
pub struct CurrentUser {
    pub claims: Claims,
    pub user: users::Model,
}

impl FromRequestParts<AppContext> for CurrentUser {
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, ctx: &AppContext) -> Result<Self, Self::Rejection> {
        let token = extract_token(get_jwt_from_config(ctx)?, parts)?;
        let claims = ctx
            .shared_store
            .get_ref::<TokenVerifier>()
            .ok_or_else(|| Error::string("no TokenVerifier in the shared store"))?
            .verify(&token)?;

        let user = users::Model::find_by_id(&ctx.db, claims.sub)
            .await
            .map_err(|err| match err {
                ModelError::EntityNotFound => Error::Unauthorized("user not found".to_string()),
                err => err.into(),
            })?;

        Ok(Self { claims, user })
    }
}
