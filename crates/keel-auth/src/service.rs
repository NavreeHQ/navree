//! [`AuthService`]: accounts, credentials and token pairs.

use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use keel_cache::Cache;
use keel_contracts::{AuditEntry, AuditKind, AuditSink, Clock, SystemClock};
use keel_crypto::{CryptoError, Encrypter, Hasher, constant_time_eq, random_token};
use keel_db::{Db, Repo, Uow};
use keel_kernel::{
    AppContext, BoxFuture, Injectable, KernelError, ProviderKey, Resolver, WeakAppContext,
};
use keel_validation::Messages;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, Set, Unchanged,
};
use serde::Serialize;
use serde_json::{Map, Value, json};
use tokio::sync::OnceCell;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::throttle::{Attempt, LoginThrottle};
use crate::token::RefreshSecret;
use crate::two_factor::{LoginOutcome, two_factor_enabled};
use crate::{
    AuthError, AuthOptions, JwtService, User, UserResource, UserResourceExtension, refresh_token,
    user,
};

/// The longest user agent stored with a refresh token.
const MAX_USER_AGENT: usize = 512;

/// A new account for [`AuthService::register`].
#[derive(Clone)]
pub struct NewUser {
    /// The display name.
    pub name: String,
    /// The email address (stored trimmed and lower-cased).
    pub email: String,
    /// The plain password (hashed with Argon2id; never stored or logged).
    pub password: String,
}

impl fmt::Debug for NewUser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NewUser")
            .field("name", &self.name)
            .field("email", &self.email)
            .field("password", &"[redacted]")
            .finish()
    }
}

/// Who a token pair is issued to: the client's IP address, user agent and
/// request id (recorded with refresh tokens and audit entries). An
/// extractor in HTTP handlers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClientMeta {
    /// The client IP address.
    pub ip: Option<String>,
    /// The client's `User-Agent`.
    pub user_agent: Option<String>,
    /// The request id.
    pub request_id: Option<String>,
}

/// An access token and a refresh token, as returned by the login, register
/// and refresh routes:
/// `{"access_token", "refresh_token", "token_type": "Bearer", "expires_in"}`.
#[derive(Clone, Serialize, ToSchema)]
pub struct TokenPair {
    /// The JWT access token (`Authorization: Bearer <access_token>`).
    pub access_token: String,
    /// The opaque refresh token (`POST /auth/refresh`); single use.
    pub refresh_token: String,
    /// Always `"Bearer"`.
    #[schema(example = "Bearer")]
    pub token_type: &'static str,
    /// The access token's lifetime in seconds.
    pub expires_in: u64,
}

impl TokenPair {
    fn new(access_token: String, refresh_token: String, access_ttl: Duration) -> Self {
        Self {
            access_token,
            refresh_token,
            token_type: "Bearer",
            expires_in: access_ttl.as_secs(),
        }
    }
}

/// The tokens are bearer credentials: never in debug output.
impl fmt::Debug for TokenPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenPair")
            .field("access_token", &"[redacted]")
            .field("refresh_token", &"[redacted]")
            .field("token_type", &self.token_type)
            .field("expires_in", &self.expires_in)
            .finish()
    }
}

/// Accounts, credentials and token pairs (NestJS `AuthService` +
/// `UsersService`). Provided and exported by the auth module.
///
/// - Passwords are hashed with Argon2id (keel-crypto's `Hasher`) and
///   rehashed at login when the hashing parameters changed.
/// - Access tokens are stateless JWTs ([`JwtService`]); refresh tokens are
///   opaque `<id>.<secret>` strings stored as SHA-256 digests, rotated on
///   every [`refresh`](AuthService::refresh). Presenting a used refresh
///   token revokes its whole family (the login session it belongs to).
/// - Writes without a [`Uow`] parameter join the task's unit of work
///   ([`Uow::current`], e.g. the request's) when there is an unfinished
///   one, and otherwise run in their own transaction. [`refresh`](AuthService::refresh)
///   always uses its own transaction, so a detected reuse stays revoked
///   although the request fails; on SQLite call it before writing through
///   the request's unit of work.
/// - Reads use the connection pool, or the task's unit of work when its
///   transaction is open (`Db::reader`).
/// - Logins, failed logins, refreshes, detected reuse and logouts are
///   written to the `dyn AuditSink` when one is registered.
pub struct AuthService {
    pub(crate) db: Arc<Db>,
    pub(crate) hasher: Arc<Hasher>,
    pub(crate) encrypter: Arc<Encrypter>,
    pub(crate) jwt: Arc<JwtService>,
    pub(crate) options: Arc<AuthOptions>,
    pub(crate) cache: Cache,
    pub(crate) throttle: LoginThrottle,
    pub(crate) two_factor_throttle: LoginThrottle,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) app: WeakAppContext,
    dummy_hash: OnceCell<String>,
    extensions: OnceCell<Vec<Arc<dyn UserResourceExtension>>>,
}

impl Injectable for AuthService {
    fn dependencies() -> Vec<ProviderKey> {
        vec![
            ProviderKey::of::<Db>(),
            ProviderKey::of::<Hasher>(),
            ProviderKey::of::<Encrypter>(),
            ProviderKey::of::<JwtService>(),
            ProviderKey::of::<AuthOptions>(),
        ]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let db = r.get::<Db>().await?;
            let hasher = r.get::<Hasher>().await?;
            let encrypter = r.get::<Encrypter>().await?;
            let jwt = r.get::<JwtService>().await?;
            let options = r.get::<AuthOptions>().await?;
            let app = r.app();
            // The app's cache when CacheModule is in the app (shared by
            // every instance with a shared store), else an in-process one.
            let cache = match app.try_get::<Cache>().await {
                Some(cache) => (*cache).clone(),
                None => Cache::memory(),
            };
            let clock = clock_of(&app).await;
            let throttle = LoginThrottle::new(
                cache.clone(),
                options.login_throttle_per_minute,
                clock.clone(),
            );
            let two_factor_throttle = LoginThrottle::new(
                cache.clone(),
                options.two_factor.max_attempts_per_minute,
                clock.clone(),
            );
            Ok(AuthService {
                db,
                hasher,
                encrypter,
                jwt,
                options,
                cache,
                throttle,
                two_factor_throttle,
                clock,
                app: app.downgrade(),
                dummy_hash: OnceCell::new(),
                extensions: OnceCell::new(),
            })
        })
    }
}

/// The app's `dyn Clock`, or the system clock.
pub(crate) async fn clock_of(app: &AppContext) -> Arc<dyn Clock> {
    match app.try_get::<dyn Clock>().await {
        Some(clock) => clock,
        None => Arc::new(SystemClock),
    }
}

impl AuthService {
    /// The module's options.
    pub fn options(&self) -> &AuthOptions {
        &self.options
    }

    /// Creates an account in `uow` (public sign-up and admin screens):
    /// checks the name, the email (unique, case-insensitively) and the
    /// password length (`auth.password_min_length`), then stores the
    /// Argon2id hash. [`AuthError::EmailTaken`] or [`AuthError::Invalid`]
    /// otherwise. Writes an `auth.register` audit entry after the commit.
    pub async fn register(&self, uow: &Uow, input: NewUser) -> Result<User, AuthError> {
        self.check_password(&input.password)?;
        let user = self
            .insert_user(uow, &input.name, &input.email, &input.password)
            .await?;
        if let Some(sink) = self.audit_sink().await {
            let mut entry = AuditEntry::new(AuditKind::Auth, "auth.register")
                .actor(user.id)
                .subject("User", user.id.to_string());
            if let Some(request_id) = uow.request_id() {
                entry = entry.request_id(request_id);
            }
            uow.after_commit(move || Box::pin(record(sink, entry)));
        }
        Ok(user)
    }

    /// Creates an account without the password policy (seeders, commands,
    /// tests); see [`register`](AuthService::register) for public sign-up.
    pub async fn create_user(
        &self,
        name: &str,
        email: &str,
        password: &str,
    ) -> Result<User, AuthError> {
        let (name, email, password) = (name.to_owned(), email.to_owned(), password.to_owned());
        self.write(None, |uow| async move {
            self.insert_user(&uow, &name, &email, &password).await
        })
        .await
    }

    /// The user with `email` and `password` (NestJS `LocalStrategy`
    /// `validate`): [`AuthError::InvalidCredentials`] for an unknown email,
    /// a wrong password or an unreadable stored hash, in about the same
    /// time. Rehashes the password when the hashing parameters changed.
    pub async fn validate_user(&self, email: &str, password: &str) -> Result<User, AuthError> {
        self.check_credentials(email, password)
            .await?
            .map_err(|_| AuthError::InvalidCredentials)
    }

    /// Signs in without a second factor: [`attempt_login`](AuthService::attempt_login)
    /// for accounts without two-factor authentication;
    /// [`AuthError::TwoFactorRequired`] for the others (the HTTP route
    /// answers those with a challenge instead).
    pub async fn login(
        &self,
        email: &str,
        password: &str,
        meta: &ClientMeta,
    ) -> Result<(User, TokenPair), AuthError> {
        match self.attempt_login(email, password, meta).await? {
            LoginOutcome::Authenticated { user, tokens } => Ok((user, tokens)),
            LoginOutcome::TwoFactorRequired { .. } => Err(AuthError::TwoFactorRequired),
        }
    }

    /// The login flow behind `POST /auth/login`: the per email and IP
    /// throttle ([`AuthError::TooManyAttempts`] once
    /// `auth.login_throttle_per_minute` failures are counted; a successful
    /// login resets it), [`validate_user`](AuthService::validate_user),
    /// then [`issue_tokens`](AuthService::issue_tokens), or, when the user
    /// has confirmed two-factor authentication, a challenge token for
    /// [`verify_two_factor`](AuthService::verify_two_factor) instead.
    /// Audited as `auth.login`, `auth.two_factor_challenge` or
    /// `auth.login_failed` (with the email, never the password).
    pub async fn attempt_login(
        &self,
        email: &str,
        password: &str,
        meta: &ClientMeta,
    ) -> Result<LoginOutcome, AuthError> {
        let key = LoginThrottle::key(email, meta.ip.as_deref());
        // Counted before the (slow) password check, so parallel guesses
        // cannot all pass while the counter is still low.
        self.throttle_attempt(
            &self.throttle,
            &key,
            meta,
            None,
            json!({ "flow": "login", "email": normalize_email(email) }),
        )
        .await?;
        match self.check_credentials(email, password).await? {
            Ok(user) => {
                if let Err(error) = self.throttle.clear(&key).await {
                    tracing::warn!(target: "keel::auth", %error, "could not reset the login throttle");
                }
                if two_factor_enabled(&user) {
                    let challenge_token = self.sign_challenge(&user)?;
                    self.audit(
                        entry(AuditKind::Auth, "auth.two_factor_challenge", meta).actor(user.id),
                    )
                    .await;
                    return Ok(LoginOutcome::TwoFactorRequired {
                        user,
                        challenge_token,
                    });
                }
                let tokens = self.issue_tokens(&user, meta.clone()).await?;
                self.audit(entry(AuditKind::Auth, "auth.login", meta).actor(user.id))
                    .await;
                Ok(LoginOutcome::Authenticated { user, tokens })
            }
            Err(known_user) => {
                tracing::warn!(
                    target: "keel::security",
                    ip = meta.ip.as_deref().unwrap_or("-"),
                    known_user = known_user.is_some(),
                    "failed login"
                );
                let mut failed = entry(AuditKind::Security, "auth.login_failed", meta)
                    .metadata(json!({ "email": normalize_email(email) }));
                if let Some(user) = known_user {
                    failed = failed.actor(user);
                }
                self.audit(failed).await;
                Err(AuthError::InvalidCredentials)
            }
        }
    }

    /// A new login session for `user`: an access token and the first
    /// refresh token of a new family.
    pub async fn issue_tokens(
        &self,
        user: &User,
        meta: ClientMeta,
    ) -> Result<TokenPair, AuthError> {
        let secret = RefreshSecret::generate();
        let row = self.refresh_row(&secret, user.id, Uuid::now_v7(), &meta);
        self.write(Some(user.id), |uow| async move {
            let tx = uow.tx().await?;
            row.insert(&*tx).await?;
            Ok(())
        })
        .await?;
        self.pair(user.id, &secret)
    }

    /// Exchanges a refresh token for a new pair (rotation): the presented
    /// token is revoked and replaced by the next one of its family.
    /// [`AuthError::InvalidRefreshToken`] when it is unknown, malformed,
    /// expired or revoked (logout); [`AuthError::RefreshTokenReused`] when
    /// it was already exchanged, in which case the whole family is revoked
    /// (audited as `auth.refresh_reuse_detected`). Runs in its own
    /// transaction, so the revocation stands although the request fails.
    pub async fn refresh(
        &self,
        refresh_token: &str,
        meta: ClientMeta,
    ) -> Result<TokenPair, AuthError> {
        let Some(presented) = RefreshSecret::parse(refresh_token) else {
            return Err(AuthError::InvalidRefreshToken);
        };
        let next = RefreshSecret::generate();
        let now = self.clock.now();
        let rotation = {
            let (presented, next, meta) = (&presented, &next, &meta);
            self.own(None, meta.request_id.clone(), |uow| async move {
                self.rotate(&uow, presented, next, meta, now).await
            })
            .await?
        };
        match rotation {
            Rotation::Rotated { user_id, family_id } => {
                let pair = self.pair(user_id, &next)?;
                self.audit(
                    entry(AuditKind::Auth, "auth.refresh", &meta)
                        .actor(user_id)
                        .metadata(json!({ "family_id": family_id })),
                )
                .await;
                Ok(pair)
            }
            Rotation::Reused { user_id, family_id } => {
                tracing::warn!(
                    target: "keel::security",
                    %user_id,
                    %family_id,
                    "refresh token reuse detected; the token family is revoked"
                );
                self.audit(
                    entry(AuditKind::Security, "auth.refresh_reuse_detected", &meta)
                        .actor(user_id)
                        .metadata(json!({ "family_id": family_id })),
                )
                .await;
                Err(AuthError::RefreshTokenReused)
            }
            Rotation::Invalid => Err(AuthError::InvalidRefreshToken),
        }
    }

    /// Ends sessions of `user_id`: with `all`, every refresh token of the
    /// user; otherwise the family of `refresh_token` when it is the user's
    /// (anything else is ignored). Access tokens are stateless and stay
    /// valid until they expire. The user's real-time subscriptions end
    /// (`dyn Broadcaster::end_user_subscriptions`, after the commit).
    /// Audited as `auth.logout`.
    pub async fn logout(
        &self,
        refresh_token: Option<&str>,
        user_id: Uuid,
        all: bool,
    ) -> Result<(), AuthError> {
        let presented = refresh_token.and_then(RefreshSecret::parse);
        let now = self.clock.now();
        let revoked = self
            .write(Some(user_id), |uow| async move {
                let tx = uow.tx().await?;
                if all {
                    return revoke_user(&*tx, user_id, now).await;
                }
                let Some(presented) = presented else {
                    return Ok(0);
                };
                match refresh_token::Entity::find_by_id(presented.id())
                    .one(&*tx)
                    .await?
                {
                    Some(row) if row.user_id == user_id && same_hash(&row, &presented) => {
                        revoke_family(&*tx, row.family_id, now).await
                    }
                    _ => Ok(0),
                }
            })
            .await?;
        let mut logout = AuditEntry::new(AuditKind::Auth, "auth.logout")
            .actor(user_id)
            .metadata(json!({ "all": all, "revoked": revoked }));
        if let Some(request_id) = Uow::current().and_then(|uow| uow.request_id().map(str::to_owned))
        {
            logout = logout.request_id(request_id);
        }
        self.audit(logout).await;
        crate::disconnect::end_user_subscriptions(&self.app, user_id).await;
        Ok(())
    }

    /// Revokes every refresh token of `user_id` (after a password reset,
    /// for example); the number of tokens revoked.
    pub async fn revoke_all_refresh_tokens(&self, user_id: Uuid) -> Result<u64, AuthError> {
        let now = self.clock.now();
        self.write(Some(user_id), |uow| async move {
            let tx = uow.tx().await?;
            revoke_user(&*tx, user_id, now).await
        })
        .await
    }

    /// The user with `id`. Inside a unit of work whose transaction is open
    /// (a request after a record check, say) it reads through that
    /// transaction (`Db::reader`), so the request never waits for a second
    /// pool connection while it holds SQLite's write lock.
    pub async fn find_user(&self, id: Uuid) -> Result<Option<User>, AuthError> {
        Ok(user::Entity::find_by_id(id)
            .one(&self.db.reader().await)
            .await?)
    }

    /// The user with `email` (compared trimmed and case-insensitively);
    /// read like [`find_user`](AuthService::find_user).
    pub async fn find_by_email(&self, email: &str) -> Result<Option<User>, AuthError> {
        Ok(user::Entity::find()
            .filter(user::Column::Email.eq(normalize_email(email)))
            .one(&self.db.reader().await)
            .await?)
    }

    /// Sets `user_id`'s password (checked against
    /// `auth.password_min_length`). Refresh tokens are left alone; call
    /// [`revoke_all_refresh_tokens`](AuthService::revoke_all_refresh_tokens)
    /// to end other sessions. [`AuthError::UserNotFound`] if there is no
    /// such user.
    pub async fn update_password(
        &self,
        user_id: Uuid,
        new_password: &str,
    ) -> Result<(), AuthError> {
        self.check_password(new_password)?;
        let hash = self.hasher.hash(new_password).await?;
        self.set_password_hash(user_id, hash).await.map(drop)
    }

    /// The user's resource as the auth routes return it: [`UserResource`]
    /// plus the fields of every [`UserResourceExtension`].
    pub async fn present(&self, user: &User) -> Result<Value, AuthError> {
        let mut fields = match serde_json::to_value(UserResource::from(user)) {
            Ok(Value::Object(fields)) => fields,
            _ => Map::new(),
        };
        for extension in self.extensions().await? {
            extension
                .extend(user, &mut fields)
                .await
                .map_err(AuthError::Extension)?;
        }
        Ok(Value::Object(fields))
    }

    /// The [`JwtService`] signing the access tokens.
    pub fn jwt(&self) -> &JwtService {
        &self.jwt
    }

    // -- internals ----------------------------------------------------------

    /// `Ok(Ok(user))` for valid credentials, `Ok(Err(known user id))` for
    /// invalid ones.
    async fn check_credentials(
        &self,
        email: &str,
        password: &str,
    ) -> Result<Result<User, Option<Uuid>>, AuthError> {
        let Some(user) = self.find_by_email(email).await? else {
            // Spend the time of a verification anyway, so response times
            // do not tell which emails have accounts.
            let dummy = self.dummy_hash().await?;
            let _ = self.hasher.verify(password, dummy).await;
            return Ok(Err(None));
        };
        match self.hasher.verify(password, &user.password_hash).await {
            Ok(true) => {}
            Ok(false) => return Ok(Err(Some(user.id))),
            Err(CryptoError::InvalidHash) => {
                tracing::warn!(
                    target: "keel::security",
                    user_id = %user.id,
                    "stored password hash is not a valid Argon2 PHC string"
                );
                return Ok(Err(Some(user.id)));
            }
            Err(error) => return Err(error.into()),
        }
        if !self.hasher.needs_rehash(&user.password_hash) {
            return Ok(Ok(user));
        }
        let rehashed = match self.hasher.hash(password).await {
            Ok(hash) => self.set_password_hash(user.id, hash).await,
            Err(error) => Err(error.into()),
        };
        match rehashed {
            Ok(user) => Ok(Ok(user)),
            Err(error) => {
                // The login itself is valid; try again next time.
                tracing::warn!(target: "keel::auth", user_id = %user.id, %error, "password rehash failed");
                Ok(Ok(user))
            }
        }
    }

    pub(crate) async fn set_password_hash(
        &self,
        user_id: Uuid,
        hash: String,
    ) -> Result<User, AuthError> {
        let now = self.clock.now();
        self.write(Some(user_id), |uow| async move {
            let row = user::ActiveModel {
                id: Unchanged(user_id),
                password_hash: Set(hash),
                updated_at: Set(now),
                ..Default::default()
            };
            Repo::<user::Entity>::update(&uow, row)
                .await
                .map_err(|error| match error {
                    DbErr::RecordNotFound(_) | DbErr::RecordNotUpdated => AuthError::UserNotFound,
                    error => error.into(),
                })
        })
        .await
    }

    async fn insert_user(
        &self,
        uow: &Uow,
        name: &str,
        email: &str,
        password: &str,
    ) -> Result<User, AuthError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(invalid("name", "required", &[]));
        }
        let email = normalize_email(email);
        if email.is_empty() || !email.contains('@') {
            return Err(invalid("email", "email", &[]));
        }
        // Hash before taking the write lock: Argon2id is slow on purpose.
        let password_hash = self.hasher.hash(password).await?;
        let now = self.clock.now();
        let taken = {
            let tx = uow.tx().await?;
            user::Entity::find()
                .filter(user::Column::Email.eq(email.as_str()))
                .one(&*tx)
                .await?
                .is_some()
        };
        if taken {
            return Err(AuthError::EmailTaken);
        }
        let row = user::ActiveModel {
            id: Set(Uuid::now_v7()),
            name: Set(name.to_owned()),
            email: Set(email),
            password_hash: Set(password_hash),
            email_verified_at: Set(None),
            two_factor_secret: Set(None),
            two_factor_recovery_codes: Set(None),
            two_factor_confirmed_at: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        };
        Ok(Repo::<user::Entity>::insert(uow, row).await?)
    }

    /// Claims the presented token and inserts its successor, or revokes
    /// the family on reuse.
    async fn rotate(
        &self,
        uow: &Uow,
        presented: &RefreshSecret,
        next: &RefreshSecret,
        meta: &ClientMeta,
        now: DateTime<Utc>,
    ) -> Result<Rotation, AuthError> {
        let tx = uow.tx().await?;
        let Some(row) = refresh_token::Entity::find_by_id(presented.id())
            .one(&*tx)
            .await?
        else {
            return Ok(Rotation::Invalid);
        };
        if !same_hash(&row, presented) {
            return Ok(Rotation::Invalid);
        }
        let reused = Rotation::Reused {
            user_id: row.user_id,
            family_id: row.family_id,
        };
        if row.revoked_at.is_some() {
            // Already exchanged for a successor: someone replays it. A token
            // revoked by a logout (or with its family) is simply invalid.
            if row.replaced_by.is_none() {
                return Ok(Rotation::Invalid);
            }
            revoke_family(&*tx, row.family_id, now).await?;
            return Ok(reused);
        }
        if row.expires_at <= now {
            return Ok(Rotation::Invalid);
        }
        // Claim the token; a concurrent refresh that got there first
        // makes this one a reuse.
        let claimed = refresh_token::Entity::update_many()
            .col_expr(refresh_token::Column::RevokedAt, Expr::value(now))
            .col_expr(refresh_token::Column::ReplacedBy, Expr::value(next.id()))
            .filter(refresh_token::Column::Id.eq(row.id))
            .filter(refresh_token::Column::RevokedAt.is_null())
            .exec(&*tx)
            .await?;
        if claimed.rows_affected != 1 {
            revoke_family(&*tx, row.family_id, now).await?;
            return Ok(reused);
        }
        self.refresh_row(next, row.user_id, row.family_id, meta)
            .insert(&*tx)
            .await?;
        Ok(Rotation::Rotated {
            user_id: row.user_id,
            family_id: row.family_id,
        })
    }

    fn refresh_row(
        &self,
        secret: &RefreshSecret,
        user_id: Uuid,
        family_id: Uuid,
        meta: &ClientMeta,
    ) -> refresh_token::ActiveModel {
        let now = self.clock.now();
        let ttl = TimeDelta::seconds(
            i64::try_from(self.options.jwt.refresh_ttl_secs).unwrap_or(i64::MAX),
        );
        refresh_token::ActiveModel {
            id: Set(secret.id()),
            user_id: Set(user_id),
            family_id: Set(family_id),
            token_hash: Set(secret.hash().to_owned()),
            expires_at: Set(now
                .checked_add_signed(ttl)
                .unwrap_or(DateTime::<Utc>::MAX_UTC)),
            revoked_at: Set(None),
            replaced_by: Set(None),
            ip: Set(meta.ip.clone()),
            user_agent: Set(meta.user_agent.as_deref().map(truncate_user_agent)),
            created_at: Set(now),
        }
    }

    fn pair(&self, user_id: Uuid, refresh: &RefreshSecret) -> Result<TokenPair, AuthError> {
        Ok(TokenPair::new(
            self.jwt.sign_access(user_id)?,
            refresh.plain().to_owned(),
            self.jwt.access_ttl(),
        ))
    }

    pub(crate) fn check_password(&self, password: &str) -> Result<(), AuthError> {
        let min = self.options.password_min_length;
        if password.chars().count() < min {
            return Err(invalid(
                "password",
                "min.string",
                &[("min", min.to_string())],
            ));
        }
        Ok(())
    }

    /// Runs `f` in the task's unfinished unit of work, or else in a new one
    /// committed when `f` succeeds.
    pub(crate) async fn write<T, F, Fut>(&self, actor: Option<Uuid>, f: F) -> Result<T, AuthError>
    where
        F: FnOnce(Uow) -> Fut,
        Fut: Future<Output = Result<T, AuthError>>,
    {
        if let Some(uow) = Uow::current()
            && !uow.is_finished().await
        {
            return f(uow).await;
        }
        self.own(actor, None, f).await
    }

    /// Runs `f` in a new unit of work: commits when it succeeds, rolls back
    /// otherwise.
    async fn own<T, F, Fut>(
        &self,
        actor: Option<Uuid>,
        request_id: Option<String>,
        f: F,
    ) -> Result<T, AuthError>
    where
        F: FnOnce(Uow) -> Fut,
        Fut: Future<Output = Result<T, AuthError>>,
    {
        let uow = self.db.uow(actor, request_id).await;
        match uow.scope(f(uow.clone())).await {
            Ok(value) => {
                uow.commit().await?;
                Ok(value)
            }
            Err(error) => {
                if let Err(rollback) = uow.rollback().await {
                    tracing::warn!(target: "keel::auth", error = %rollback, "rollback failed");
                }
                Err(error)
            }
        }
    }

    /// A hash to verify against for unknown emails.
    async fn dummy_hash(&self) -> Result<&str, AuthError> {
        let hash = self
            .dummy_hash
            .get_or_try_init(|| async { self.hasher.hash(&random_token(32)).await })
            .await?;
        Ok(hash)
    }

    async fn extensions(&self) -> Result<&[Arc<dyn UserResourceExtension>], AuthError> {
        let extensions = self
            .extensions
            .get_or_try_init(|| async {
                match self.app.upgrade() {
                    Some(app) => app
                        .get_all::<dyn UserResourceExtension>()
                        .await
                        .map_err(|error| AuthError::Extension(error.into())),
                    None => Ok(Vec::new()),
                }
            })
            .await?;
        Ok(extensions)
    }

    pub(crate) async fn audit_sink(&self) -> Option<Arc<dyn AuditSink>> {
        self.app.upgrade()?.try_get::<dyn AuditSink>().await
    }

    pub(crate) async fn audit(&self, entry: AuditEntry) {
        if let Some(sink) = self.audit_sink().await {
            record(sink, entry).await;
        }
    }

    /// Counts an attempt on `throttle` for `key` before a credential is
    /// checked. A refusal is logged as a security event and, the first
    /// time in a window (so a flood is one entry), audited as
    /// `auth.throttled` with `metadata` (the `flow`, plus the email for
    /// logins); it is [`AuthError::TooManyAttempts`].
    pub(crate) async fn throttle_attempt(
        &self,
        throttle: &LoginThrottle,
        key: &str,
        meta: &ClientMeta,
        actor: Option<Uuid>,
        metadata: Value,
    ) -> Result<(), AuthError> {
        let (retry_after, first) = match throttle.count(key).await? {
            Attempt::Allowed => return Ok(()),
            Attempt::Refused { retry_after, first } => (retry_after, first),
        };
        tracing::warn!(
            target: "keel::security",
            ip = meta.ip.as_deref().unwrap_or("-"),
            flow = metadata["flow"].as_str().unwrap_or("-"),
            "throttled after too many failed attempts"
        );
        if first {
            let mut throttled =
                entry(AuditKind::Security, "auth.throttled", meta).metadata(metadata);
            if let Some(actor) = actor {
                throttled = throttled.actor(actor);
            }
            self.audit(throttled).await;
        }
        Err(AuthError::TooManyAttempts { retry_after })
    }
}

impl fmt::Debug for AuthService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthService")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

/// What a refresh found.
enum Rotation {
    Rotated { user_id: Uuid, family_id: Uuid },
    Reused { user_id: Uuid, family_id: Uuid },
    Invalid,
}

pub(crate) async fn record(sink: Arc<dyn AuditSink>, entry: AuditEntry) {
    let action = entry.action.clone();
    if let Err(error) = sink.record(entry).await {
        tracing::warn!(target: "keel::audit", %error, action, "could not write an auth audit entry");
    }
}

/// An audit entry with the client's address, user agent and request id.
pub(crate) fn entry(kind: AuditKind, action: &str, meta: &ClientMeta) -> AuditEntry {
    let mut entry = AuditEntry::new(kind, action);
    if let Some(ip) = &meta.ip {
        entry = entry.ip(ip.clone());
    }
    if let Some(user_agent) = &meta.user_agent {
        entry = entry.user_agent(truncate_user_agent(user_agent));
    }
    if let Some(request_id) = &meta.request_id {
        entry = entry.request_id(request_id.clone());
    }
    entry
}

fn same_hash(row: &refresh_token::Model, presented: &RefreshSecret) -> bool {
    constant_time_eq(row.token_hash.as_bytes(), presented.hash().as_bytes())
}

async fn revoke_family<C: ConnectionTrait>(
    conn: &C,
    family_id: Uuid,
    now: DateTime<Utc>,
) -> Result<u64, AuthError> {
    let result = refresh_token::Entity::update_many()
        .col_expr(refresh_token::Column::RevokedAt, Expr::value(now))
        .filter(refresh_token::Column::FamilyId.eq(family_id))
        .filter(refresh_token::Column::RevokedAt.is_null())
        .exec(conn)
        .await?;
    Ok(result.rows_affected)
}

pub(crate) async fn revoke_user<C: ConnectionTrait>(
    conn: &C,
    user_id: Uuid,
    now: DateTime<Utc>,
) -> Result<u64, AuthError> {
    let result = refresh_token::Entity::update_many()
        .col_expr(refresh_token::Column::RevokedAt, Expr::value(now))
        .filter(refresh_token::Column::UserId.eq(user_id))
        .filter(refresh_token::Column::RevokedAt.is_null())
        .exec(conn)
        .await?;
    Ok(result.rows_affected)
}

/// Emails are compared and stored trimmed and lower-cased.
pub(crate) fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

pub(crate) fn truncate_user_agent(user_agent: &str) -> String {
    match user_agent.char_indices().nth(MAX_USER_AGENT) {
        Some((end, _)) => user_agent[..end].to_owned(),
        None => user_agent.to_owned(),
    }
}

/// [`AuthError::Invalid`] with keel-validation's default message for `rule`.
pub(crate) fn invalid(field: &'static str, rule: &str, params: &[(&str, String)]) -> AuthError {
    AuthError::Invalid {
        field,
        message: Messages::default().render(field, rule, params),
    }
}
