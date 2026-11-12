//! Sign-in from native apps, revoking provider tokens, and Sign in with
//! Apple's server-to-server notifications.
//!
//! On a phone the system draws the sign-in sheet (Apple's authorization
//! controller, Android's Credential Manager) and hands the app an identity
//! token, which the app posts to the server. The flow:
//!
//! 1. The app asks for a nonce ([`Social::native_nonce`]): a fresh random
//!    value and the sealed attempt that carries it (the same sealed state
//!    as the web flow, for this provider, expiring within five minutes).
//! 2. The app hands the nonce to the sheet: Google's as is, Apple's as the
//!    lower-case hex SHA-256 of it (Apple returns the value it was given,
//!    so the token carries the hash).
//! 3. The app posts the token and the attempt ([`Social::native_user`]):
//!    the attempt is used up, the token's signature is checked against the
//!    provider's key set, then its issuer, its audience (one of the site's
//!    app ids: the bundle ids of `auth.apps.apple`, or the Google client
//!    id an Android app passes as its server client id), its expiry, its
//!    issue time (not before the attempt started) and its nonce. A token
//!    taken from another attempt or made for another app is refused.
//! 4. The person is signed in by [`Social::sign_in`], exactly as after the
//!    web callback: the same linking rules (an unverified email never links
//!    to an existing account), sign-in policies, two-factor challenge,
//!    session, events and audit entries.
//!
//! Sign in with Apple's authorization code, when the app sends it, is
//! exchanged for tokens with a client secret signed for the app's bundle
//! id. The returned ID token must name the same person. The tokens are kept
//! encrypted with the account, with the client they belong to, so that
//! disconnecting Apple ([`Social::unlink`]), offboarding and deleting the
//! account ([`Social::revoke_tokens`], asked by
//! `AuthService::revoke_all_credentials`) revoke them at Apple.

use std::sync::Arc;
use std::time::Duration;

use navree_contracts::{AuditEntry, AuditKind, CredentialRevoker, RevokedByKind};
use navree_crypto::jose::Jws;
use navree_db::Uow;
use navree_kernel::{BoxError, BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_orm::{Executor, Model as _};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::entity::{social_account, social_account_client};
use crate::events::{SocialAccountUnlinked, SocialProviderNotice};
use crate::oauth::{self, LEEWAY_SECS, invalid_id_token, refused, str_of};
use crate::provider::{Endpoints, Kind, ProviderEntry};
use crate::runtime::Runtime;
use crate::service::{Backend, find_account};
use crate::state::StatePayload;
use crate::{
    Intent, NativeNonce, NativeSignIn, ProviderToken, Social, SocialCallback, SocialError,
    SocialUser,
};

/// The longest life of a native sign-in attempt, in seconds
/// (`social.state_ttl_secs` when that is shorter).
pub(crate) const NATIVE_ATTEMPT_SECS: u64 = 300;

/// The longest display name taken from the app's post, in characters.
const MAX_NAME_CHARS: usize = 200;

/// How old a provider notification may be, in seconds.
const NOTIFICATION_MAX_AGE_SECS: i64 = 7 * 86_400;

impl Social {
    /// The nonce of a new sign-in attempt from a native app with
    /// `provider` (`apple` or `google`): a random `nonce` for the system's
    /// request and the sealed `state` to post back with the identity token,
    /// within `expires_in` seconds, once. Nothing is stored.
    ///
    /// [`SocialError::NativeUnavailable`] (404) for other providers, and
    /// for Apple when `auth.apps.apple` lists no app.
    pub async fn native_nonce(&self, provider: &str) -> Result<NativeNonce, SocialError> {
        let rt = self.rt()?;
        let entry = rt.provider(provider)?;
        native_audiences(rt, entry)?;
        let ttl = rt.options.state_ttl_secs.min(NATIVE_ATTEMPT_SECS);
        let now = rt.now().timestamp();
        let mut state = StatePayload::new(
            provider,
            Intent::SignIn,
            None,
            None,
            now.saturating_add(i64::try_from(ttl).unwrap_or(i64::MAX)),
        );
        state.iat = Some(now);
        Ok(NativeNonce {
            nonce: state.nonce.clone(),
            state: state.seal(&rt.encrypter)?,
            expires_in: ttl,
        })
    }

    /// Checks what a native app posted after the system's sign-in sheet
    /// and reads the person: the attempt (sealed by this app, for this
    /// provider, not expired, used up now), then the identity token
    /// (signature against the provider's key set, issuer, one of the app
    /// ids as audience, expiry, issue time and the attempt's nonce), then
    /// Apple's authorization code when there is one. Pass the user to
    /// [`sign_in`](Social::sign_in).
    ///
    /// [`SocialError::InvalidState`] (400) for a bad, foreign, expired or
    /// used attempt, [`SocialError::InvalidIdToken`] (400) for a token that
    /// fails a check, [`SocialError::NativeUnavailable`] (404) as for
    /// [`native_nonce`](Social::native_nonce).
    pub async fn native_user(
        &self,
        provider: &str,
        post: &NativeSignIn,
    ) -> Result<SocialUser, SocialError> {
        let rt = self.rt()?;
        let entry = rt.provider(provider)?;
        let audiences = native_audiences(rt, entry)?;
        let state = self.use_state(rt, provider, &post.state).await?;
        // A web flow's state never starts a native sign-in.
        let Some(started) = state.iat.filter(|_| state.intent == Intent::SignIn) else {
            return Err(SocialError::InvalidState);
        };
        if post.id_token.trim().is_empty() {
            return Err(invalid_id_token(entry, "no identity token"));
        }
        let mut user = match &self.backend {
            Backend::Fake(fake) => fake.native(provider, &post.id_token, &entry.name)?,
            Backend::Real => {
                let endpoints = oauth::endpoints(rt, entry).await?;
                let claims = oauth::verify_signature(rt, entry, &endpoints, &post.id_token).await?;
                let audiences: Vec<&str> = audiences.iter().map(String::as_str).collect();
                let now = rt.now().timestamp();
                oauth::check_token(entry, &endpoints.issuer, &claims, &audiences, now)?;
                check_nonce_and_age(entry, &claims, &state, started, now)?;
                let client = str_of(&claims, "aud").map(str::to_owned);
                let callback = SocialCallback {
                    user: post.user.clone(),
                    ..SocialCallback::default()
                };
                let mut user = oauth::id_token_user(entry, claims, &callback)?;
                if let Some(name) = &user.name
                    && name.chars().count() > MAX_NAME_CHARS
                {
                    user.name = Some(name.chars().take(MAX_NAME_CHARS).collect());
                }
                let code = post
                    .authorization_code
                    .as_deref()
                    .map(str::trim)
                    .filter(|code| !code.is_empty());
                if entry.kind == Kind::Apple
                    && let (Some(code), Some(client)) = (code, client)
                {
                    let token =
                        exchange_apple_code(rt, entry, &endpoints, code, &client, &user.id).await?;
                    user.token = Some(token);
                }
                user
            }
        };
        user.provider = entry.key.clone();
        Ok(user)
    }

    /// Revokes at the provider every stored token of `user_id` that the
    /// provider can revoke (Sign in with Apple's, Google's), then forgets
    /// them; the accounts stay linked. The number of accounts whose tokens
    /// were revoked. A token the provider no longer knows counts as
    /// revoked; an unreachable provider is
    /// [`SocialError::ProviderUnavailable`] and the call can be repeated.
    ///
    /// Call it before deleting a user (Apple requires apps to revoke the
    /// tokens of a deleted account). `AuthService::revoke_all_credentials`
    /// calls it through the module's credential revoker.
    pub async fn revoke_tokens(&self, user_id: Uuid) -> Result<u64, SocialError> {
        let rt = self.rt()?;
        let accounts = social_account::Model::query()
            .where_(social_account::Model::USER_ID.eq(user_id))
            .get(&rt.db)
            .await?
            .into_models();
        let mut revoked = 0;
        for account in accounts {
            if !self.revoke_at_provider(rt, &rt.db, &account).await? {
                continue;
            }
            revoked += 1;
            let id = account.id;
            let now = rt.now();
            self.write(
                rt,
                None,
                |uow| async move { forget_tokens(&uow, id, now).await },
            )
            .await?;
        }
        Ok(revoked)
    }

    /// Revokes `account`'s stored tokens at the provider (through `uow`),
    /// before the account is disconnected.
    pub(crate) async fn revoke_account(
        &self,
        rt: &Runtime,
        uow: &Uow,
        account: &social_account::Model,
    ) -> Result<(), SocialError> {
        self.revoke_at_provider(rt, uow, account).await.map(drop)
    }

    /// Revokes `account`'s stored token at its provider: whether there was
    /// one the provider can revoke.
    async fn revoke_at_provider<E: Executor + ?Sized>(
        &self,
        rt: &Runtime,
        exec: &E,
        account: &social_account::Model,
    ) -> Result<bool, SocialError> {
        let Some(entry) = rt.providers.get(&account.provider) else {
            return Ok(false);
        };
        if !matches!(entry.kind, Kind::Apple | Kind::Google) {
            return Ok(false);
        }
        let (stored, hint) = match (&account.refresh_token, &account.access_token) {
            (Some(token), _) => (token, "refresh_token"),
            (None, Some(token)) => (token, "access_token"),
            (None, None) => return Ok(false),
        };
        let endpoints = match &self.backend {
            Backend::Fake(fake) => {
                fake.revoked(&account.provider, &account.provider_user_id);
                return Ok(true);
            }
            Backend::Real => oauth::endpoints(rt, entry).await?,
        };
        let Some(url) = endpoints.revoke.clone() else {
            return Ok(false);
        };
        let token = rt.encrypter.decrypt_string(stored)?;
        let form = vec![("token", token), ("token_type_hint", hint.to_owned())];
        let response = if entry.kind == Kind::Apple {
            let client = social_account_client::of(exec, account.id)
                .await?
                .unwrap_or_else(|| entry.config.client_id.clone());
            oauth::client_request(rt, entry, &endpoints, &url, &client, form).await?
        } else {
            rt.http
                .post(&url)
                .header("accept", "application/json")
                .form(&form)
                .send()
                .await
                .map_err(oauth::unavailable(entry))?
        };
        let status = response.status();
        if status.is_success() {
            return Ok(true);
        }
        if status.is_server_error() {
            return Err(SocialError::ProviderUnavailable {
                provider: entry.name.clone(),
                message: format!("the revocation endpoint answered {}", status.as_u16()),
            });
        }
        let body: Value = response.json().unwrap_or(Value::Null);
        match body.get("error").and_then(Value::as_str) {
            // The provider no longer knows the token: it is dead already.
            Some("invalid_grant" | "invalid_token") => Ok(true),
            Some(error) => Err(refused(entry, error)),
            None => Err(refused(
                entry,
                format!("the revocation endpoint answered {}", status.as_u16()),
            )),
        }
    }

    /// Acts on a server-to-server notification of `provider` (Sign in with
    /// Apple's `payload`, a JWS): checks its signature against Apple's key
    /// set, its issuer, its audience (the Services ID or an app's bundle
    /// id), its age and that it was not handled before (`jti`), then, for
    /// the linked account it names:
    ///
    /// - `consent-revoked`: forgets the stored tokens and signs the user
    ///   out everywhere;
    /// - `account-deleted`: removes the link and signs the user out
    ///   everywhere;
    /// - `email-disabled`, `email-enabled`: records it.
    ///
    /// Each emits [`SocialProviderNotice`] and is audited. A notification
    /// about nobody linked here is accepted and ignored.
    /// [`SocialError::InvalidNotification`] (400) when a check fails.
    ///
    /// The fake ([`Social::fake`]) reads the payload without checking its
    /// signature.
    pub async fn receive_notification(
        &self,
        provider: &str,
        payload: &str,
    ) -> Result<(), SocialError> {
        let rt = self.rt()?;
        let entry = rt.provider(provider)?;
        if entry.kind != Kind::Apple {
            return Err(SocialError::UnknownProvider(provider.to_owned()));
        }
        let invalid = |reason: &str| SocialError::InvalidNotification {
            provider: entry.name.clone(),
            reason: reason.to_owned(),
        };
        let as_invalid = |error: SocialError| match error {
            SocialError::InvalidIdToken { reason, .. } => invalid(&reason),
            other => other,
        };
        let (claims, endpoints) = match &self.backend {
            Backend::Fake(_) => {
                let jws = Jws::parse(payload).map_err(|_| invalid("not a JWS"))?;
                let claims: Map<String, Value> = jws
                    .claims()
                    .map_err(|_| invalid("the claims are not an object"))?;
                (Value::Object(claims), entry.static_endpoints())
            }
            Backend::Real => {
                let endpoints = oauth::endpoints(rt, entry).await?;
                let claims = oauth::verify_signature(rt, entry, &endpoints, payload)
                    .await
                    .map_err(as_invalid)?;
                (claims, endpoints)
            }
        };
        let mut audiences = vec![entry.config.client_id.as_str()];
        audiences.extend(rt.apple_apps.iter().map(String::as_str));
        oauth::check_issuer_and_audience(entry, &endpoints.issuer, &claims, &audiences)
            .map_err(as_invalid)?;
        let now = rt.now().timestamp();
        let iat = claims
            .get("iat")
            .and_then(Value::as_i64)
            .ok_or_else(|| invalid("no issue time"))?;
        if iat - LEEWAY_SECS > now || iat < now - NOTIFICATION_MAX_AGE_SECS {
            return Err(invalid("issued too long ago or in the future"));
        }
        let events = match claims.get("events") {
            Some(Value::String(raw)) => {
                serde_json::from_str::<Value>(raw).map_err(|_| invalid("unreadable events"))?
            }
            Some(events @ Value::Object(_)) => events.clone(),
            _ => return Err(invalid("no events")),
        };
        let (Some(kind), Some(sub)) = (str_of(&events, "type"), str_of(&events, "sub")) else {
            return Err(invalid("the event has no type or subject"));
        };
        let notice = match kind {
            "consent-revoked" => "consent_revoked",
            "account-deleted" | "account-delete" => "account_deleted",
            "email-disabled" => "email_disabled",
            "email-enabled" => "email_enabled",
            // A kind this version does not know: accepted, nothing to do.
            _ => return Ok(()),
        };
        // Each notification is acted on once (Apple may deliver it again).
        let once = claims.get("jti").and_then(Value::as_str).map(|jti| {
            format!(
                "navree-social:notification:{}",
                navree_crypto::sha256_hex(jti.as_bytes())
            )
        });
        if let Some(key) = &once {
            let ttl = Duration::from_secs(NOTIFICATION_MAX_AGE_SECS.unsigned_abs());
            if !rt.cache.add(key, &1_u8, ttl).await? {
                return Ok(());
            }
        }
        let acted = self.act_on_notice(rt, entry, sub, notice).await;
        if acted.is_err()
            && let Some(key) = &once
            && let Err(error) = rt.cache.forget(key).await
        {
            tracing::warn!(target: "navree::social", %error, "could not forget a notification id");
        }
        acted
    }

    async fn act_on_notice(
        &self,
        rt: &Runtime,
        entry: &ProviderEntry,
        sub: &str,
        notice: &'static str,
    ) -> Result<(), SocialError> {
        let Some(account) = find_account(&rt.db, &entry.key, sub).await? else {
            return Ok(());
        };
        let user_id = account.user_id;
        let event = SocialProviderNotice {
            user_id,
            provider: entry.key.clone(),
            provider_user_id: account.provider_user_id.clone(),
            notice: notice.to_owned(),
        };
        let audit = AuditEntry::new(AuditKind::Auth, format!("social.{notice}"))
            .actor(user_id)
            .subject("User", user_id.to_string())
            .metadata(json!({ "provider": entry.key }));
        match notice {
            "consent_revoked" | "account_deleted" => {
                let now = rt.now();
                let delete = notice == "account_deleted";
                self.write(rt, None, |uow| async move {
                    if delete {
                        social_account::delete(&uow, &account).await?;
                        rt.emit_after_commit(
                            &uow,
                            SocialAccountUnlinked {
                                user_id,
                                provider: account.provider.clone(),
                                provider_user_id: account.provider_user_id.clone(),
                            },
                        )
                        .await;
                    } else {
                        forget_tokens(&uow, account.id, now).await?;
                    }
                    rt.emit_after_commit(&uow, event).await;
                    rt.audit_after_commit(&uow, audit).await;
                    Ok(())
                })
                .await?;
                // Apple: the person's sign-in with the app has ended.
                rt.auth.logout(None, user_id, true).await?;
            }
            _ => {
                rt.emit(event).await;
                rt.audit(audit).await;
            }
        }
        Ok(())
    }
}

/// The audiences a native identity token of `entry` may carry: the bundle
/// ids of `auth.apps.apple` for Apple, the Google client id for Google.
fn native_audiences(rt: &Runtime, entry: &ProviderEntry) -> Result<Vec<String>, SocialError> {
    let audiences = match entry.kind {
        Kind::Apple => rt.apple_apps.clone(),
        Kind::Google => vec![entry.config.client_id.clone()],
        _ => Vec::new(),
    };
    if audiences.is_empty() {
        return Err(SocialError::NativeUnavailable {
            provider: entry.name.clone(),
        });
    }
    Ok(audiences)
}

/// The nonce of the attempt (Apple's in the hashed form the app gave it)
/// and an issue time within the attempt.
fn check_nonce_and_age(
    entry: &ProviderEntry,
    claims: &Value,
    state: &StatePayload,
    started: i64,
    now: i64,
) -> Result<(), SocialError> {
    let expected = match entry.kind {
        Kind::Apple => navree_crypto::sha256_hex(state.nonce.as_bytes()),
        _ => state.nonce.clone(),
    };
    match claims.get("nonce").and_then(Value::as_str) {
        None => return Err(invalid_id_token(entry, "the token has no nonce")),
        Some(nonce) if nonce != expected => {
            return Err(invalid_id_token(entry, "the nonce does not match"));
        }
        Some(_) => {}
    }
    let iat = claims
        .get("iat")
        .and_then(Value::as_i64)
        .ok_or_else(|| invalid_id_token(entry, "the token has no issue time"))?;
    if iat + LEEWAY_SECS < started {
        return Err(invalid_id_token(
            entry,
            "the token is older than the sign-in attempt",
        ));
    }
    if iat - LEEWAY_SECS > now {
        return Err(invalid_id_token(
            entry,
            "the token was issued in the future",
        ));
    }
    Ok(())
}

/// Exchanges Sign in with Apple's authorization code as `client` (the
/// app's bundle id) and checks the ID token Apple returns names `subject`,
/// the person of the verified identity token.
async fn exchange_apple_code(
    rt: &Runtime,
    entry: &ProviderEntry,
    endpoints: &Endpoints,
    code: &str,
    client: &str,
    subject: &str,
) -> Result<ProviderToken, SocialError> {
    let form = vec![
        ("grant_type", "authorization_code".to_owned()),
        ("code", code.to_owned()),
    ];
    let response =
        oauth::client_request(rt, entry, endpoints, &endpoints.token, client, form).await?;
    let body = oauth::read_token_response(entry, &response)?;
    let token = oauth::parse_token(entry, &body, rt.now(), None)?;
    let id_token = token
        .id_token
        .as_ref()
        .ok_or_else(|| refused(entry, "no ID token in the token response"))?;
    let claims = oauth::verify_signature(rt, entry, endpoints, id_token.expose()).await?;
    oauth::check_token(
        entry,
        &endpoints.issuer,
        &claims,
        &[client],
        rt.now().timestamp(),
    )?;
    if str_of(&claims, "sub") != Some(subject) {
        return Err(invalid_id_token(
            entry,
            "the authorization code belongs to another account",
        ));
    }
    Ok(token)
}

/// Forgets account `id`'s stored tokens and their client.
async fn forget_tokens(
    uow: &Uow,
    id: Uuid,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), SocialError> {
    social_account::update(uow, id, |row| {
        row.access_token = None;
        row.refresh_token = None;
        row.token_expires_at = None;
        row.updated_at = now;
    })
    .await?;
    social_account_client::Model::query()
        .where_key(id)
        .delete(uow)
        .await?;
    Ok(())
}

/// The module's [`CredentialRevoker`]: offboarding a user
/// (`AuthService::revoke_all_credentials`, `revoke_credentials_of_domain`)
/// revokes their provider tokens through [`Social::revoke_tokens`].
pub(crate) struct SocialRevoker {
    social: Arc<Social>,
}

impl Injectable for SocialRevoker {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Social>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(SocialRevoker {
                social: r.get::<Social>().await?,
            })
        })
    }
}

impl CredentialRevoker for SocialRevoker {
    fn name(&self) -> &'static str {
        "social"
    }

    fn revoke_for_users<'a>(
        &'a self,
        users: &'a [Uuid],
    ) -> BoxFuture<'a, Result<RevokedByKind, BoxError>> {
        Box::pin(async move {
            let mut revoked = 0;
            let mut failure = None;
            // Every user is tried; the first failure is reported after.
            for user in users {
                match self.social.revoke_tokens(*user).await {
                    Ok(count) => revoked += count,
                    Err(error) => {
                        failure.get_or_insert(error);
                    }
                }
            }
            if let Some(error) = failure {
                return Err(Box::new(error) as BoxError);
            }
            Ok(RevokedByKind::from([("social_provider_tokens", revoked)]))
        })
    }
}
