//! [`Social`]: the redirect, the callback, linking and token issuing.

use std::fmt;
use std::future::Future;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use navree_auth::{ClientMeta, LoginOutcome, TokenType, User, two_factor_enabled, user};
use navree_contracts::{AuditEntry, AuditKind, Secret};
use navree_db::{Repo, Uow};
use navree_kernel::{AppContext, BoxError, BoxFuture, Lifecycle};
use sea_orm::{
    ActiveValue, ColumnTrait, ConnectionTrait, DbErr, EntityTrait, QueryFilter, QueryOrder, Set,
    Unchanged,
};
use serde_json::json;
use tokio::sync::OnceCell;
use uuid::Uuid;

use crate::entity::social_account;
use crate::events::{SocialAccountLinked, SocialAccountUnlinked};
use crate::fake::{FakeState, SocialFake};
use crate::oauth;
use crate::options::{LinkByEmail, TwoFactorPolicy};
use crate::provider::{Kind, ProviderEntry};
use crate::runtime::Runtime;
use crate::state::{StatePayload, check_return_to};
use crate::{
    AuthorizeUrl, Intent, LinkedAccount, ProviderToken, RedirectOptions, SocialCallback,
    SocialError, SocialRedirect, SocialUser,
};

/// The callback throttle's window, in seconds.
const THROTTLE_WINDOW_SECS: i64 = 60;

/// Where provider calls go.
enum Backend {
    /// The providers' real endpoints.
    Real,
    /// [`SocialFake`]'s answers.
    Fake(Arc<FakeState>),
}

/// Social sign-in: the provider's authorization URL with a sealed state,
/// the callback (state check, code exchange, profile), and signing in,
/// linking and creating Navree users. Provided and exported by
/// [`SocialModule`](crate::SocialModule).
///
/// - Writes without a [`Uow`] parameter ([`sign_in`](Social::sign_in),
///   [`token`](Social::token)'s refresh) join the task's unit of work
///   (the request's in a handler), or run in their own transaction.
/// - Provider calls happen before any database write, so no transaction is
///   held open while a provider answers.
/// - A sealed state works once: its id is remembered in the cache (the
///   app's `CacheModule`, or an in-process cache) until it expires. With
///   several app instances, use a shared cache store.
pub struct Social {
    backend: Backend,
    runtime: OnceLock<Arc<Runtime>>,
    no_password: OnceCell<String>,
}

impl Social {
    /// A `Social` that never calls a provider, and the [`SocialFake`] that
    /// decides what each code returns. Pass it to the app with
    /// `TestApp::builder().override_value(Arc::new(social))`; it binds to
    /// the app when it boots. States are still sealed and checked, users
    /// still linked or created and Navree tokens still issued.
    pub fn fake() -> (Social, SocialFake) {
        let state = Arc::new(FakeState::default());
        let social = Social {
            backend: Backend::Fake(state.clone()),
            runtime: OnceLock::new(),
            no_password: OnceCell::new(),
        };
        (social, SocialFake::new(state))
    }

    /// The real service, bound to `runtime`.
    pub(crate) fn real(runtime: Runtime) -> Social {
        let social = Social {
            backend: Backend::Real,
            runtime: OnceLock::new(),
            no_password: OnceCell::new(),
        };
        let _ = social.runtime.set(Arc::new(runtime));
        social
    }

    /// Whether this is a [`fake`](Social::fake).
    pub fn is_fake(&self) -> bool {
        matches!(self.backend, Backend::Fake(_))
    }

    fn rt(&self) -> Result<&Arc<Runtime>, SocialError> {
        self.runtime.get().ok_or(SocialError::NotBound)
    }

    /// The configured provider keys, in order (`["github", "google"]`), for
    /// a sign-in page.
    pub fn providers(&self) -> Result<Vec<String>, SocialError> {
        Ok(self.rt()?.providers.keys().cloned().collect())
    }

    /// The display name of provider `key` (`GitHub`, or the configured
    /// `name`).
    pub fn provider_name(&self, key: &str) -> Result<String, SocialError> {
        Ok(self.rt()?.provider(key)?.name.clone())
    }

    /// The provider's authorization URL and the sealed state of a sign-in
    /// flow.
    pub async fn redirect(&self, provider: &str) -> Result<SocialRedirect, SocialError> {
        self.redirect_with(provider, RedirectOptions::default())
            .await
    }

    /// The same as [`redirect`](Social::redirect), with extra scopes, extra
    /// parameters, a return path (only relative paths;
    /// [`SocialError::InvalidReturnTo`] otherwise) and the intent. A link
    /// flow needs `user_id`: only that user can complete it.
    pub async fn redirect_with(
        &self,
        provider: &str,
        options: RedirectOptions,
    ) -> Result<SocialRedirect, SocialError> {
        let rt = self.rt()?;
        let entry = rt.provider(provider)?;
        if let Some(return_to) = &options.return_to {
            check_return_to(return_to)?;
        }
        let user = match options.intent {
            Intent::SignIn => None,
            Intent::Link => Some(options.user_id.ok_or_else(|| {
                SocialError::Config("RedirectOptions::user_id is required to link".to_owned())
            })?),
        };
        for param in options.params.keys() {
            if crate::options::RESERVED_PARAMS.contains(&param.as_str()) {
                return Err(SocialError::Config(format!(
                    "the authorization parameter `{param}` is set by Navree"
                )));
            }
        }
        let ttl = i64::try_from(rt.options.state_ttl_secs).unwrap_or(i64::MAX);
        let exp = rt.now().timestamp().saturating_add(ttl);
        let state = StatePayload::new(provider, options.intent, user, options.return_to, exp);
        let sealed = state.seal(&rt.encrypter)?;

        let mut url = AuthorizeUrl::default()
            .core_param("client_id", entry.config.client_id.clone())
            .core_param("redirect_uri", rt.redirect_uri(provider))
            .core_param("state", sealed.clone());
        if entry.config.pkce {
            url = url
                .core_param("code_challenge", state.challenge())
                .core_param("code_challenge_method", "S256");
        }
        if entry.uses_nonce() {
            url = url.core_param("nonce", state.nonce.clone());
        }
        url = url.extra_scopes(entry.config.scopes.iter().chain(&options.scopes).cloned());
        let endpoints = match self.backend {
            // A fake never reads an `oidc` provider's discovery document.
            Backend::Fake(_) if entry.kind == Kind::Oidc => entry.static_endpoints(),
            _ => oauth::endpoints(rt, entry).await?,
        };
        url = oauth::decorate(entry, &endpoints, url);
        for (name, value) in entry.config.params.iter().chain(&options.params) {
            url = url.param(name.clone(), value.clone());
        }
        Ok(SocialRedirect {
            url: url.build(&entry.name)?,
            state: sealed,
        })
    }

    /// Checks the callback's state (sealed by this app, for this provider,
    /// not expired, not used), exchanges the code and reads the user.
    pub async fn user(
        &self,
        provider: &str,
        callback: &SocialCallback,
    ) -> Result<SocialUser, SocialError> {
        self.complete(provider, callback)
            .await
            .map(|(user, _)| user)
    }

    /// [`user`](Social::user), with the opened state.
    pub(crate) async fn complete(
        &self,
        provider: &str,
        callback: &SocialCallback,
    ) -> Result<(SocialUser, StatePayload), SocialError> {
        let rt = self.rt()?;
        let entry = rt.provider(provider)?;
        let state = StatePayload::open(&rt.encrypter, &callback.state)?;
        let now = rt.now().timestamp();
        if state.provider != provider || state.exp <= now {
            return Err(SocialError::InvalidState);
        }
        // A state works once: remember its id until it expires.
        let remaining = Duration::from_secs(u64::try_from(state.exp - now).unwrap_or(1).max(1));
        let first_use = rt
            .cache
            .add(
                &format!("navree-social:state:{}", state.id),
                &1_u8,
                remaining,
            )
            .await?;
        if !first_use {
            return Err(SocialError::InvalidState);
        }
        if callback.code.trim().is_empty() {
            return Err(SocialError::provider(entry.name.clone(), "no code"));
        }
        let user = match &self.backend {
            Backend::Real => {
                let token = oauth::exchange(rt, entry, &callback.code, &state).await?;
                oauth::profile(rt, entry, token, &state, callback).await?
            }
            Backend::Fake(fake) => fake.exchange(provider, &callback.code, &entry.name)?,
        };
        Ok((user, state))
    }

    /// Signs `social_user` in, like a password login: finds the user by a
    /// linked account, links a user with the same verified email
    /// (`link_by_email = "verified"`), or creates one (when registration is
    /// on), then issues Navree tokens, or a two-factor challenge for users
    /// with two-factor authentication (`two_factor = "require"`).
    ///
    /// [`SocialError::EmailTaken`] (409) when the email belongs to a user
    /// and linking does not apply, [`SocialError::RegistrationClosed`] (403)
    /// when nobody matches and registration is off,
    /// [`SocialError::NotAllowed`] (403) outside `allowed_email_domains`.
    /// Audited as `auth.login` (or `auth.two_factor_challenge`), with
    /// `metadata.provider`; a new user also as `auth.register`.
    pub async fn sign_in(
        &self,
        social_user: &SocialUser,
        meta: &ClientMeta,
    ) -> Result<LoginOutcome, SocialError> {
        let rt = self.rt()?;
        let entry = rt.provider(&social_user.provider)?;
        check_email_domains(rt, entry, social_user)?;
        let request_id = meta.request_id.clone();
        let user = self
            .write(rt, request_id, |uow| async move {
                self.resolve_user(rt, &uow, entry, social_user).await
            })
            .await?;
        let metadata = json!({ "provider": entry.key });
        if two_factor_enabled(&user) && rt.options.two_factor == TwoFactorPolicy::Require {
            let ttl = Duration::from_secs(rt.auth_options.two_factor.challenge_ttl_secs);
            let challenge_token =
                rt.auth
                    .jwt()
                    .sign(user.id, TokenType::TwoFactorChallenge, ttl)?;
            rt.audit(
                entry_for(AuditKind::Auth, "auth.two_factor_challenge", meta)
                    .actor(user.id)
                    .metadata(metadata),
            )
            .await;
            return Ok(LoginOutcome::TwoFactorRequired {
                user,
                challenge_token,
            });
        }
        let tokens = rt.auth.issue_tokens(&user, meta.clone()).await?;
        rt.audit(
            entry_for(AuditKind::Auth, "auth.login", meta)
                .actor(user.id)
                .metadata(metadata),
        )
        .await;
        Ok(LoginOutcome::Authenticated { user, tokens })
    }

    /// Links `social_user`'s provider account to `user_id` in `uow` (from
    /// the user's settings). [`SocialError::AlreadyLinked`] (409) when the
    /// account belongs to another user,
    /// [`SocialError::ProviderAlreadyConnected`] (409) when the user has
    /// another account of this provider. Linking the same account again
    /// refreshes it. Emits [`SocialAccountLinked`] and audits `social.linked`
    /// after the commit.
    pub async fn link(
        &self,
        uow: &Uow,
        user_id: Uuid,
        social_user: &SocialUser,
    ) -> Result<LinkedAccount, SocialError> {
        let rt = self.rt()?;
        let entry = rt.provider(&social_user.provider)?;
        let (by_provider_id, by_user) = {
            let tx = uow.tx().await?;
            (
                find_account(&*tx, &entry.key, &social_user.id).await?,
                find_user_account(&*tx, user_id, &entry.key).await?,
            )
        };
        let account = match by_provider_id {
            Some(account) if account.user_id == user_id => {
                self.refresh_account(rt, uow, account, social_user).await?
            }
            Some(_) => {
                return Err(SocialError::AlreadyLinked {
                    provider: entry.name.clone(),
                });
            }
            None if by_user.is_some() => {
                return Err(SocialError::ProviderAlreadyConnected {
                    provider: entry.name.clone(),
                });
            }
            None => {
                self.insert_account(rt, uow, user_id, entry, social_user)
                    .await?
            }
        };
        Ok(linked_account(rt, account))
    }

    /// Disconnects `provider` from `user_id` in `uow`.
    /// [`SocialError::NotLinked`] (404) when it is not connected;
    /// [`SocialError::LastSignInMethod`] (409) when the user has no
    /// password and no other provider. Emits [`SocialAccountUnlinked`] and
    /// audits `social.unlinked` after the commit.
    pub async fn unlink(
        &self,
        uow: &Uow,
        user_id: Uuid,
        provider: &str,
    ) -> Result<(), SocialError> {
        let rt = self.rt()?;
        let provider_name = rt.provider_name(provider);
        let (account, user, others) = {
            let tx = uow.tx().await?;
            let account = find_user_account(&*tx, user_id, provider).await?;
            let user = user::Entity::find_by_id(user_id).one(&*tx).await?;
            let others = social_account::Entity::find()
                .filter(social_account::Column::UserId.eq(user_id))
                .filter(social_account::Column::Provider.ne(provider))
                .all(&*tx)
                .await?
                .len();
            (account, user, others)
        };
        let Some(account) = account else {
            return Err(SocialError::NotLinked {
                provider: provider_name,
            });
        };
        let has_password = user.is_some_and(|user| has_password(&user.password_hash));
        if !has_password && others == 0 {
            return Err(SocialError::LastSignInMethod {
                provider: provider_name,
            });
        }
        let event = SocialAccountUnlinked {
            user_id,
            provider: account.provider.clone(),
            provider_user_id: account.provider_user_id.clone(),
        };
        Repo::<social_account::Entity>::delete(uow, account).await?;
        rt.emit_after_commit(uow, event).await;
        let mut audit = AuditEntry::new(AuditKind::Auth, "social.unlinked")
            .actor(user_id)
            .subject("User", user_id.to_string())
            .metadata(json!({ "provider": provider }));
        if let Some(request_id) = uow.request_id() {
            audit = audit.request_id(request_id);
        }
        rt.audit_after_commit(uow, audit).await;
        Ok(())
    }

    /// The user's linked accounts, oldest first.
    pub async fn accounts(&self, user_id: Uuid) -> Result<Vec<LinkedAccount>, SocialError> {
        let rt = self.rt()?;
        let rows = social_account::Entity::find()
            .filter(social_account::Column::UserId.eq(user_id))
            .order_by_asc(social_account::Column::CreatedAt)
            .all(&rt.db.reader().await)
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| linked_account(rt, row))
            .collect())
    }

    /// The user linked to `provider_user_id` at `provider`, if any.
    pub async fn find_user(
        &self,
        provider: &str,
        provider_user_id: &str,
    ) -> Result<Option<User>, SocialError> {
        let rt = self.rt()?;
        let Some(account) = find_account(&rt.db.reader().await, provider, provider_user_id).await?
        else {
            return Ok(None);
        };
        Ok(user::Entity::find_by_id(account.user_id)
            .one(&rt.db.reader().await)
            .await?)
    }

    /// A current access token of `user_id` at `provider`, refreshed first
    /// when it has expired (or expires within a minute). Needs
    /// `social.store_tokens`. [`SocialError::Reauthorize`] when the provider
    /// refuses the refresh or no refresh token was stored (the account
    /// stays linked); [`SocialError::NoToken`] when no tokens are stored;
    /// [`SocialError::NotLinked`] when the provider is not connected.
    pub async fn token(
        &self,
        user_id: Uuid,
        provider: &str,
    ) -> Result<Secret<String>, SocialError> {
        let rt = self.rt()?;
        let entry = rt.provider(provider)?;
        let account = find_user_account(&rt.db.reader().await, user_id, provider)
            .await?
            .ok_or_else(|| SocialError::NotLinked {
                provider: entry.name.clone(),
            })?;
        let no_token = || SocialError::NoToken {
            provider: entry.name.clone(),
        };
        if !rt.options.store_tokens {
            return Err(no_token());
        }
        let access = account.access_token.as_deref().ok_or_else(no_token)?;
        let fresh = account
            .token_expires_at
            .is_none_or(|at| at > rt.now() + chrono::TimeDelta::seconds(60));
        if fresh {
            return Ok(Secret::new(rt.encrypter.decrypt_string(access)?));
        }
        let Some(refresh_token) = account.refresh_token.as_deref() else {
            return Err(SocialError::Reauthorize {
                provider: entry.name.clone(),
            });
        };
        let refresh_token = rt.encrypter.decrypt_string(refresh_token)?;
        let token = match &self.backend {
            Backend::Real => oauth::refresh(rt, entry, &refresh_token).await?,
            Backend::Fake(fake) => fake.refresh(provider, &entry.name)?,
        };
        let access = token.access_token.expose().clone();
        let columns = TokenColumns::of(rt, Some(&token))?;
        let now = rt.now();
        self.write(rt, None, |uow| async move {
            let row = social_account::ActiveModel {
                id: Unchanged(account.id),
                access_token: Set(columns.access),
                refresh_token: Set(columns.refresh.or(account.refresh_token)),
                token_expires_at: Set(columns.expires_at),
                scopes: if columns.scopes.is_some() {
                    Set(columns.scopes)
                } else {
                    ActiveValue::NotSet
                },
                updated_at: Set(now),
                ..Default::default()
            };
            Repo::<social_account::Entity>::update(&uow, row).await?;
            Ok(())
        })
        .await?;
        Ok(Secret::new(access))
    }

    // -- internals ----------------------------------------------------------

    /// The app's `AuthService`.
    pub(crate) fn auth_service(&self) -> Result<&navree_auth::AuthService, SocialError> {
        Ok(&self.rt()?.auth)
    }

    /// The lifetime of a two-factor challenge token, in seconds.
    pub(crate) fn challenge_ttl_secs(&self) -> Result<u64, SocialError> {
        Ok(self.rt()?.auth_options.two_factor.challenge_ttl_secs)
    }

    /// Counts a callback from `ip` against `auth.login_throttle_per_minute`
    /// (fixed one-minute windows): [`SocialError::TooManyAttempts`] over
    /// the limit. Returns the counter to clear after a successful sign-in.
    pub(crate) async fn count_callback(
        &self,
        ip: Option<&str>,
    ) -> Result<Option<String>, SocialError> {
        let rt = self.rt()?;
        let limit = rt.auth_options.login_throttle_per_minute;
        if limit == 0 {
            return Ok(None);
        }
        let now = rt.now().timestamp();
        let window = now.div_euclid(THROTTLE_WINDOW_SECS);
        let digest = navree_crypto::sha256_hex(ip.unwrap_or("-").as_bytes());
        let key = format!("navree-social:callback:{digest}:{window}");
        // Each window has its own counter, so one that outlives its window
        // is never read again.
        rt.cache
            .add(
                &key,
                &0_i64,
                Duration::from_secs(2 * THROTTLE_WINDOW_SECS as u64),
            )
            .await?;
        let attempts = rt.cache.increment(&key, 1).await?;
        if attempts > i64::from(limit) {
            let wait = (window + 1) * THROTTLE_WINDOW_SECS - now;
            return Err(SocialError::TooManyAttempts {
                retry_after: Duration::from_secs(u64::try_from(wait).unwrap_or(1).max(1)),
            });
        }
        Ok(Some(key))
    }

    /// Forgets the callback counter after a successful sign-in.
    pub(crate) async fn clear_callback_throttle(&self, key: Option<String>) {
        if let (Some(key), Ok(rt)) = (key, self.rt())
            && let Err(error) = rt.cache.forget(&key).await
        {
            tracing::warn!(target: "navree::social", %error, "could not reset the callback throttle");
        }
    }

    /// The user `social_user` signs in as, linking or creating one.
    async fn resolve_user(
        &self,
        rt: &Runtime,
        uow: &Uow,
        entry: &ProviderEntry,
        social_user: &SocialUser,
    ) -> Result<User, SocialError> {
        let (linked, by_email) = {
            let tx = uow.tx().await?;
            let linked = match find_account(&*tx, &entry.key, &social_user.id).await? {
                Some(account) => {
                    let user = user::Entity::find_by_id(account.user_id).one(&*tx).await?;
                    user.map(|user| (account, user))
                }
                None => None,
            };
            let by_email = match (&linked, &social_user.email) {
                (None, Some(email)) => {
                    user::Entity::find()
                        .filter(user::Column::Email.eq(email.as_str()))
                        .one(&*tx)
                        .await?
                }
                _ => None,
            };
            (linked, by_email)
        };
        if let Some((account, user)) = linked {
            self.refresh_account(rt, uow, account, social_user).await?;
            return Ok(user);
        }
        if let Some(user) = by_email {
            if rt.options.link_by_email == LinkByEmail::Verified && social_user.email_verified {
                let taken = {
                    let tx = uow.tx().await?;
                    find_user_account(&*tx, user.id, &entry.key)
                        .await?
                        .is_some()
                };
                if taken {
                    return Err(SocialError::ProviderAlreadyConnected {
                        provider: entry.name.clone(),
                    });
                }
                self.insert_account(rt, uow, user.id, entry, social_user)
                    .await?;
                return Ok(user);
            }
            return Err(SocialError::EmailTaken {
                provider: entry.name.clone(),
            });
        }
        if !rt.registration() {
            return Err(SocialError::RegistrationClosed);
        }
        let email = social_user
            .email
            .clone()
            .ok_or_else(|| SocialError::MissingEmail {
                provider: entry.name.clone(),
            })?;
        let user = self.create_user(rt, uow, social_user, &email).await?;
        self.insert_account(rt, uow, user.id, entry, social_user)
            .await?;
        let mut audit = AuditEntry::new(AuditKind::Auth, "auth.register")
            .actor(user.id)
            .subject("User", user.id.to_string())
            .metadata(json!({ "provider": entry.key }));
        if let Some(request_id) = uow.request_id() {
            audit = audit.request_id(request_id);
        }
        rt.audit_after_commit(uow, audit).await;
        Ok(user)
    }

    /// Creates a user with the provider's name and email, no password, and
    /// the email verified when the provider verified it.
    async fn create_user(
        &self,
        rt: &Runtime,
        uow: &Uow,
        social_user: &SocialUser,
        email: &str,
    ) -> Result<User, SocialError> {
        let password_hash = self.no_password_hash(rt).await?;
        let now = rt.now();
        let name = social_user
            .name
            .clone()
            .or_else(|| social_user.nickname.clone())
            .unwrap_or_else(|| email.split('@').next().unwrap_or(email).to_owned());
        let row = user::ActiveModel {
            id: Set(Uuid::now_v7()),
            name: Set(name),
            email: Set(email.trim().to_lowercase()),
            password_hash: Set(password_hash),
            email_verified_at: Set(social_user.email_verified.then_some(now)),
            two_factor_secret: Set(None),
            two_factor_recovery_codes: Set(None),
            two_factor_confirmed_at: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        };
        Ok(Repo::<user::Entity>::insert(uow, row).await?)
    }

    /// The password hash of a user without a password: an Argon2id PHC
    /// string with the hasher's current parameters whose digest is all
    /// zero bytes, so no password verifies against it, a password login
    /// takes as long as for anyone else, and [`has_password`] can tell.
    async fn no_password_hash(&self, rt: &Runtime) -> Result<String, SocialError> {
        let template = self
            .no_password
            .get_or_try_init(|| async {
                let hash = rt.hasher.hash(&navree_crypto::random_token(32)).await?;
                let (prefix, digest) = hash.rsplit_once('$').ok_or_else(|| {
                    SocialError::Config("the password hasher returned no PHC string".to_owned())
                })?;
                Ok::<_, SocialError>(format!("{prefix}${}", "A".repeat(digest.len())))
            })
            .await?;
        Ok(template.clone())
    }

    /// Inserts the account row, then emits and audits the link after the
    /// commit.
    async fn insert_account(
        &self,
        rt: &Runtime,
        uow: &Uow,
        user_id: Uuid,
        entry: &ProviderEntry,
        social_user: &SocialUser,
    ) -> Result<social_account::Model, SocialError> {
        let now = rt.now();
        let tokens = TokenColumns::of(rt, social_user.token.as_ref())?;
        let row = social_account::ActiveModel {
            id: Set(Uuid::now_v7()),
            user_id: Set(user_id),
            provider: Set(entry.key.clone()),
            provider_user_id: Set(social_user.id.clone()),
            email: Set(social_user.email.clone()),
            name: Set(social_user.name.clone()),
            avatar_url: Set(social_user.avatar_url.clone()),
            nickname: Set(social_user.nickname.clone()),
            access_token: Set(tokens.access),
            refresh_token: Set(tokens.refresh),
            token_expires_at: Set(tokens.expires_at),
            scopes: Set(tokens.scopes),
            created_at: Set(now),
            updated_at: Set(now),
        };
        let account = Repo::<social_account::Entity>::insert(uow, row)
            .await
            .map_err(|error| match error.sql_err() {
                // A concurrent sign-in or link got there first.
                Some(sea_orm::SqlErr::UniqueConstraintViolation(_)) => SocialError::AlreadyLinked {
                    provider: entry.name.clone(),
                },
                _ => error.into(),
            })?;
        rt.emit_after_commit(
            uow,
            SocialAccountLinked {
                user_id,
                provider: entry.key.clone(),
                provider_user_id: social_user.id.clone(),
            },
        )
        .await;
        let mut audit = AuditEntry::new(AuditKind::Auth, "social.linked")
            .actor(user_id)
            .subject("User", user_id.to_string())
            .metadata(json!({ "provider": entry.key }));
        if let Some(request_id) = uow.request_id() {
            audit = audit.request_id(request_id);
        }
        rt.audit_after_commit(uow, audit).await;
        Ok(account)
    }

    /// Updates a linked account with what the provider reported this time
    /// (and its tokens, with `store_tokens`).
    async fn refresh_account(
        &self,
        rt: &Runtime,
        uow: &Uow,
        account: social_account::Model,
        social_user: &SocialUser,
    ) -> Result<social_account::Model, SocialError> {
        let tokens = TokenColumns::of(rt, social_user.token.as_ref())?;
        let keep = |new: &Option<String>, old: &Option<String>| new.clone().or_else(|| old.clone());
        let mut row = social_account::ActiveModel {
            id: Unchanged(account.id),
            email: Set(keep(&social_user.email, &account.email)),
            // Apple sends the name on the first sign-in only.
            name: Set(keep(&social_user.name, &account.name)),
            avatar_url: Set(keep(&social_user.avatar_url, &account.avatar_url)),
            nickname: Set(keep(&social_user.nickname, &account.nickname)),
            updated_at: Set(rt.now()),
            ..Default::default()
        };
        if tokens.access.is_some() {
            row.access_token = Set(tokens.access);
            row.refresh_token = Set(tokens.refresh.or(account.refresh_token));
            row.token_expires_at = Set(tokens.expires_at);
            row.scopes = Set(tokens.scopes);
        }
        Ok(Repo::<social_account::Entity>::update(uow, row).await?)
    }

    /// Runs `f` in the task's unfinished unit of work, or else in a new one
    /// committed when `f` succeeds.
    async fn write<T, F, Fut>(
        &self,
        rt: &Runtime,
        request_id: Option<String>,
        f: F,
    ) -> Result<T, SocialError>
    where
        F: FnOnce(Uow) -> Fut,
        Fut: Future<Output = Result<T, SocialError>>,
    {
        if let Some(uow) = Uow::current()
            && !uow.is_finished().await
        {
            return f(uow).await;
        }
        let uow = rt.db.uow(None, request_id).await;
        match uow.scope(f(uow.clone())).await {
            Ok(value) => {
                uow.commit().await?;
                Ok(value)
            }
            Err(error) => {
                if let Err(rollback) = uow.rollback().await {
                    tracing::warn!(target: "navree::social", error = %rollback, "rollback failed");
                }
                Err(error)
            }
        }
    }
}

impl fmt::Debug for Social {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Social")
            .field("fake", &self.is_fake())
            .field("bound", &self.runtime.get().is_some())
            .finish_non_exhaustive()
    }
}

/// Binds a [`Social::fake`] (or any `Social` given to `override_value`) to
/// the app at boot, and reads the `oidc` providers' discovery documents.
impl Lifecycle for Social {
    fn on_init(&self, ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            if self.runtime.get().is_none() {
                let runtime = Runtime::build(&ctx).await?;
                let _ = self.runtime.set(Arc::new(runtime));
            }
            Ok(())
        })
    }

    fn on_bootstrap(&self, _ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            if self.is_fake() {
                return Ok(());
            }
            let Ok(rt) = self.rt() else {
                return Ok(());
            };
            for entry in rt.providers.values().filter(|e| e.kind == Kind::Oidc) {
                if let Err(error) = oauth::endpoints(rt, entry).await {
                    tracing::warn!(
                        target: "navree::social",
                        provider = %entry.key,
                        %error,
                        "could not read the OpenID Connect discovery document; retrying on first use"
                    );
                }
            }
            Ok(())
        })
    }
}

/// Whether a stored password hash is a real one (not
/// [`Social`]'s no-password marker: a digest of zero bytes).
pub(crate) fn has_password(hash: &str) -> bool {
    !hash
        .rsplit('$')
        .next()
        .is_some_and(|digest| !digest.is_empty() && digest.bytes().all(|b| b == b'A'))
}

/// The `allowed_email_domains` checks (the section's, then the
/// provider's): a verified address of a listed domain.
fn check_email_domains(
    rt: &Runtime,
    entry: &ProviderEntry,
    social_user: &SocialUser,
) -> Result<(), SocialError> {
    for allowed in [
        &rt.options.allowed_email_domains,
        &entry.config.allowed_email_domains,
    ] {
        if allowed.is_empty() {
            continue;
        }
        let ok = social_user.email_verified
            && social_user
                .email_domain()
                .is_some_and(|domain| allowed.iter().any(|a| a.eq_ignore_ascii_case(domain)));
        if !ok {
            return Err(SocialError::NotAllowed(
                "Your email address may not sign in here".to_owned(),
            ));
        }
    }
    Ok(())
}

fn linked_account(rt: &Runtime, account: social_account::Model) -> LinkedAccount {
    LinkedAccount {
        provider_name: rt.provider_name(&account.provider),
        provider: account.provider,
        email: account.email,
        name: account.name,
        avatar_url: account.avatar_url,
        linked_at: account.created_at,
    }
}

/// An audit entry with the client's address, user agent and request id.
fn entry_for(kind: AuditKind, action: &str, meta: &ClientMeta) -> AuditEntry {
    let mut entry = AuditEntry::new(kind, action);
    if let Some(ip) = &meta.ip {
        entry = entry.ip(ip.clone());
    }
    if let Some(user_agent) = &meta.user_agent {
        entry = entry.user_agent(user_agent.chars().take(512).collect::<String>());
    }
    if let Some(request_id) = &meta.request_id {
        entry = entry.request_id(request_id.clone());
    }
    entry
}

async fn find_account<C: ConnectionTrait>(
    conn: &C,
    provider: &str,
    provider_user_id: &str,
) -> Result<Option<social_account::Model>, DbErr> {
    social_account::Entity::find()
        .filter(social_account::Column::Provider.eq(provider))
        .filter(social_account::Column::ProviderUserId.eq(provider_user_id))
        .one(conn)
        .await
}

async fn find_user_account<C: ConnectionTrait>(
    conn: &C,
    user_id: Uuid,
    provider: &str,
) -> Result<Option<social_account::Model>, DbErr> {
    social_account::Entity::find()
        .filter(social_account::Column::UserId.eq(user_id))
        .filter(social_account::Column::Provider.eq(provider))
        .one(conn)
        .await
}

/// The token columns of an account: encrypted, and only with
/// `store_tokens`.
struct TokenColumns {
    access: Option<String>,
    refresh: Option<String>,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
    scopes: Option<String>,
}

impl TokenColumns {
    fn of(rt: &Runtime, token: Option<&ProviderToken>) -> Result<TokenColumns, SocialError> {
        let token = match token {
            Some(token) if rt.options.store_tokens => token,
            _ => {
                return Ok(TokenColumns {
                    access: None,
                    refresh: None,
                    expires_at: None,
                    scopes: None,
                });
            }
        };
        Ok(TokenColumns {
            access: Some(rt.encrypter.encrypt_string(token.access_token.expose())?),
            refresh: token
                .refresh_token
                .as_ref()
                .map(|t| rt.encrypter.encrypt_string(t.expose()))
                .transpose()?,
            expires_at: token.expires_at,
            scopes: (!token.scopes.is_empty()).then(|| token.scopes.join(" ")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_no_password_marker_is_recognized() {
        let real = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQ$RdescudvJCsgt3ub+b+dWRWJTmaaJObG";
        assert!(has_password(real));
        let marker = format!(
            "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQ${}",
            "A".repeat(43)
        );
        assert!(!has_password(&marker));
        assert!(has_password(""), "an empty hash is not the marker");
    }

    #[tokio::test]
    async fn an_unbound_service_says_so() {
        let (social, _fake) = Social::fake();
        assert!(social.is_fake());
        assert!(matches!(
            social.redirect("github").await,
            Err(SocialError::NotBound)
        ));
    }
}
