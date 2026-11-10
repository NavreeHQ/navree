//! Trusted token exchange on [`AuthService`] (`POST /auth/exchange`): a
//! sibling system signs a short-lived JWT for one of its users, and the
//! app accepts it once and signs the user in.
//!
//! - Issuers are configured under `auth.trusted_issuers.<name>`: the
//!   `iss` its tokens carry, its key (`jwks_url`, fetched with the app's
//!   `HttpClient` and cached, or a `public_key`), the `aud` they must
//!   carry, and `enabled`, a kill switch.
//! - A token is accepted when it is signed with RS256, ES256 or EdDSA by
//!   the issuer's key, names this app in `aud`, is current (`exp`, `nbf`,
//!   `iat` with 30 seconds of leeway, and lives at most
//!   `max_lifetime_secs`), carries a `jti` not seen before (kept in the
//!   cache until the token expires) and the `email` of a registered user
//!   (not `email_verified: false`).
//! - The [`SignInPolicy`](crate::SignInPolicy)s are asked
//!   ([`SignInMethod::TokenExchange`] with the issuer's name): an address
//!   whose domain enforces single sign-on is refused (403 `sso_required`).
//! - The user then gets a session like any sign-in. Accepted tokens are
//!   audited as `auth.token_exchanged`, refused ones as
//!   `auth.token_exchange_failed` with the reason (which the client never
//!   sees: every refusal is 401 "Invalid token").

mod der;
mod jws;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use navree_contracts::AuditKind;
use navree_http_client::{HttpClient, HttpClientOptions};
use serde_json::{Value, json};

use self::jws::{CompactJws, KeyEntry, key_from_text, keys_from_jwks};
use crate::service::entry;
use crate::{
    AuthError, AuthService, ClientMeta, LEEWAY_SECS, SignInAttempt, SignInMethod, TokenPair,
    TrustedIssuerOptions, User,
};

/// The shortest time between two fetches of a key set for an unknown key
/// id, in seconds.
const REFETCH_SECS: i64 = 60;

/// The key sets fetched from `jwks_url`s, per issuer name.
#[derive(Default)]
pub(crate) struct KeyCache {
    sets: Mutex<HashMap<String, CachedKeys>>,
}

#[derive(Clone)]
struct CachedKeys {
    keys: Arc<Vec<KeyEntry>>,
    fetched_at: i64,
}

impl KeyCache {
    fn get(&self, name: &str) -> Option<CachedKeys> {
        self.sets
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(name)
            .cloned()
    }

    fn put(&self, name: &str, keys: Vec<KeyEntry>, now: i64) -> Arc<Vec<KeyEntry>> {
        let keys = Arc::new(keys);
        self.sets
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                name.to_owned(),
                CachedKeys {
                    keys: keys.clone(),
                    fetched_at: now,
                },
            );
        keys
    }
}

/// Why a token was refused (audited, never sent).
struct Refusal {
    issuer: Option<String>,
    reason: String,
}

fn refuse(issuer: Option<&str>, reason: impl Into<String>) -> Refusal {
    Refusal {
        issuer: issuer.map(str::to_owned),
        reason: reason.into(),
    }
}

impl AuthService {
    /// Accepts a token signed by a trusted issuer (`POST /auth/exchange`)
    /// and signs its user in (see the module documentation for the checks).
    /// Every refusal is [`AuthError::ExchangeRefused`] (401 "Invalid
    /// token"), audited as `auth.token_exchange_failed`; success is audited
    /// as `auth.token_exchanged` with the issuer's name.
    pub async fn exchange_token(
        &self,
        token: &str,
        meta: &ClientMeta,
    ) -> Result<(User, TokenPair), AuthError> {
        if self.options.trusted_issuers.is_empty() {
            return Err(AuthError::Disabled("token exchange"));
        }
        let (name, user) = match self.check_exchange_token(token).await {
            Ok(Ok(found)) => found,
            Ok(Err(refusal)) => {
                tracing::warn!(
                    target: "navree::security",
                    issuer = refusal.issuer.as_deref().unwrap_or("-"),
                    reason = %refusal.reason,
                    ip = meta.ip.as_deref().unwrap_or("-"),
                    "token exchange refused"
                );
                self.audit(
                    entry(AuditKind::Security, "auth.token_exchange_failed", meta)
                        .metadata(json!({ "issuer": refusal.issuer, "reason": refusal.reason })),
                )
                .await;
                return Err(AuthError::ExchangeRefused(refusal.reason));
            }
            Err(error) => return Err(error),
        };
        let method = SignInMethod::TokenExchange {
            issuer: name.clone(),
        };
        self.check_sign_in(&SignInAttempt::of(&user, method), meta)
            .await?;
        let tokens = self.issue_tokens(&user, meta.clone()).await?;
        self.audit(
            entry(AuditKind::Auth, "auth.token_exchanged", meta)
                .actor(user.id)
                .metadata(json!({ "issuer": name })),
        )
        .await;
        Ok((user, tokens))
    }

    /// The issuer name and user of `token`, or why it is refused.
    async fn check_exchange_token(
        &self,
        token: &str,
    ) -> Result<Result<(String, User), Refusal>, AuthError> {
        let jws = match CompactJws::parse(token) {
            Ok(jws) => jws,
            Err(reason) => return Ok(Err(refuse(None, reason))),
        };
        let Some(iss) = jws.payload["iss"].as_str() else {
            return Ok(Err(refuse(None, "no issuer")));
        };
        let Some((name, issuer)) = self
            .options
            .trusted_issuers
            .iter()
            .find(|(_, issuer)| issuer.issuer == iss)
        else {
            return Ok(Err(refuse(None, "unknown issuer")));
        };
        let named = Some(name.as_str());
        if !issuer.enabled {
            return Ok(Err(refuse(named, "issuer disabled")));
        }
        let keys = match self.issuer_keys(name, issuer, &jws).await {
            Ok(keys) => keys,
            Err(reason) => return Ok(Err(refuse(named, reason))),
        };
        if !jws.verified_by(&keys) {
            return Ok(Err(refuse(named, "bad signature")));
        }
        let claims = &jws.payload;
        let now = self.clock.now().timestamp();
        let audience_ok = match &claims["aud"] {
            Value::String(aud) => *aud == issuer.audience,
            Value::Array(auds) => auds
                .iter()
                .any(|aud| aud.as_str() == Some(&issuer.audience)),
            _ => false,
        };
        if !audience_ok {
            return Ok(Err(refuse(named, "wrong audience")));
        }
        let (Some(exp), Some(iat)) = (claims["exp"].as_i64(), claims["iat"].as_i64()) else {
            return Ok(Err(refuse(named, "exp and iat are required")));
        };
        if exp.saturating_add(LEEWAY_SECS) < now {
            return Ok(Err(refuse(named, "expired")));
        }
        if iat > now.saturating_add(LEEWAY_SECS)
            || claims["nbf"]
                .as_i64()
                .is_some_and(|nbf| nbf > now.saturating_add(LEEWAY_SECS))
        {
            return Ok(Err(refuse(named, "not yet valid")));
        }
        let max_lifetime = i64::try_from(issuer.max_lifetime_secs).unwrap_or(i64::MAX);
        if exp.saturating_sub(iat) > max_lifetime {
            return Ok(Err(refuse(named, "lifetime too long")));
        }
        let Some(jti) = claims["jti"].as_str().filter(|jti| !jti.is_empty()) else {
            return Ok(Err(refuse(named, "jti is required")));
        };
        let Some(email) = claims["email"].as_str() else {
            return Ok(Err(refuse(named, "email is required")));
        };
        if claims["email_verified"] == Value::Bool(false) {
            return Ok(Err(refuse(named, "email not verified")));
        }
        let Some(user) = self.find_by_email(email).await? else {
            return Ok(Err(refuse(named, "unknown user")));
        };
        let remaining = exp.saturating_add(LEEWAY_SECS).saturating_sub(now).max(1);
        let ttl = Duration::from_secs(u64::try_from(remaining).unwrap_or(1));
        let seen = format!("navree-auth:exchange:{name}:{jti}");
        if !self.cache.add(&seen, &true, ttl).await? {
            return Ok(Err(refuse(named, "replayed")));
        }
        Ok(Ok((name.clone(), user)))
    }

    /// The keys of `issuer`: its `public_key`, or its key set (cached for
    /// `jwks_cache_secs`; fetched again, at most once a minute, for a key
    /// id the cached set does not have).
    async fn issuer_keys(
        &self,
        name: &str,
        issuer: &TrustedIssuerOptions,
        jws: &CompactJws,
    ) -> Result<Arc<Vec<KeyEntry>>, String> {
        if let Some(text) = &issuer.public_key {
            return key_from_text(text)
                .map(|key| Arc::new(vec![key]))
                .ok_or_else(|| "the configured public key cannot be read".to_owned());
        }
        let Some(url) = &issuer.jwks_url else {
            return Err("no key configured".to_owned());
        };
        let now = self.clock.now().timestamp();
        let max_age = i64::try_from(issuer.jwks_cache_secs).unwrap_or(i64::MAX);
        if let Some(cached) = self.exchange.get(name) {
            let fresh = now.saturating_sub(cached.fetched_at) < max_age;
            let recent = now.saturating_sub(cached.fetched_at) < REFETCH_SECS;
            if fresh && (jws.kid_known(&cached.keys) || recent) {
                return Ok(cached.keys);
            }
        }
        let keys = self.fetch_jwks(url).await?;
        Ok(self.exchange.put(name, keys, now))
    }

    async fn fetch_jwks(&self, url: &str) -> Result<Vec<KeyEntry>, String> {
        let client = match self.app.upgrade() {
            Some(app) => match app.try_get::<HttpClient>().await {
                Some(client) => (*client).clone(),
                None => HttpClient::new(HttpClientOptions::default())
                    .map_err(|error| format!("no HTTP client: {error}"))?,
            },
            None => return Err("the application has shut down".to_owned()),
        };
        let set: Value = client
            .get(url)
            .send()
            .await
            .and_then(|response| response.error_for_status())
            .and_then(|response| response.json())
            .map_err(|error| format!("fetching the key set failed: {error}"))?;
        Ok(keys_from_jwks(&set))
    }
}
