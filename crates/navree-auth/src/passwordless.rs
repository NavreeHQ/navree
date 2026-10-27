//! Passwordless sign-in on [`AuthService`]: magic links
//! (`POST /auth/magic-link`) and emailed sign-in codes (`POST /auth/otp`).
//!
//! - Both are kept in `login_tokens` as digests (a magic link token is
//!   `<id>.<48 base62>`, stored as its SHA-256; a code is stored as an
//!   HMAC-SHA-256 keyed by a key derived from the application key), work
//!   once, expire (`auth.magic_link.ttl_secs`, `auth.otp.ttl_secs`), and a
//!   new one replaces the user's previous one.
//! - Requests always answer the same (202), whether the address is
//!   registered or not; a second request within `throttle_secs` sends
//!   nothing.
//! - Verifying counts against the login throttle of the address and the
//!   client IP; a code is burned after `auth.otp.max_attempts` wrong
//!   guesses.
//! - A verified link or code proves the address: an unverified one becomes
//!   verified. The sign-in then continues like a password login: a
//!   two-factor challenge when the user has two-factor authentication on,
//!   a session otherwise.

use chrono::{DateTime, Utc};
use navree_contracts::AuditKind;
use navree_crypto::{constant_time_eq, hmac_sha256, sha256_hex};
use sea_orm::sea_query::Expr;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set};
use serde_json::json;
use uuid::Uuid;

use crate::account::{after, send_after_commit};
use crate::service::{entry, normalize_email};
use crate::throttle::LoginThrottle;
use crate::token::RefreshSecret;
use crate::two_factor::two_factor_enabled;
use crate::{AuthError, AuthService, ClientMeta, LoginOutcome, User, login_token, mail, user};

/// `login_tokens.kind` of magic links.
pub(crate) const MAGIC_LINK: &str = "magic_link";
/// `login_tokens.kind` of emailed sign-in codes.
pub(crate) const OTP: &str = "otp";

/// The purpose the sign-in code key is derived for.
const OTP_KEY_PURPOSE: &str = "navree-auth.otp";

impl AuthService {
    /// Mails a magic link to `email` (`POST /auth/magic-link`) when it is
    /// registered, once the unit of work commits: `{auth.magic_link.url}?token=..`,
    /// or the token alone without a URL. Unknown addresses and requests
    /// within `auth.magic_link.throttle_secs` of the last link do nothing.
    /// Audited as `auth.magic_link_sent`.
    pub async fn send_magic_link(&self, email: &str, meta: &ClientMeta) -> Result<(), AuthError> {
        let options = &self.options.magic_link;
        if !options.enabled {
            return Err(AuthError::Disabled("magic links"));
        }
        let Some(user) = self
            .passwordless_user(email, MAGIC_LINK, options.throttle_secs)
            .await?
        else {
            return Ok(());
        };
        let secret = RefreshSecret::generate();
        let mail = self.mailer().await.map(|mailer| {
            (
                mailer,
                mail::magic_link(&user, secret.plain(), &self.options.magic_link),
            )
        });
        self.store_login_token(
            &user,
            MAGIC_LINK,
            secret.id(),
            secret.hash().to_owned(),
            options.ttl_secs,
            mail,
            entry(AuditKind::Auth, "auth.magic_link_sent", meta).actor(user.id),
        )
        .await
    }

    /// Signs in with a magic link token (`POST /auth/magic-link/verify`):
    /// unknown, used, expired or wrong tokens are
    /// [`AuthError::InvalidLoginToken`] (audited as `auth.login_failed`
    /// with `method: magic_link`). The token works once; then the sign-in
    /// continues like [`attempt_login`](AuthService::attempt_login).
    pub async fn login_with_magic_link(
        &self,
        token: &str,
        meta: &ClientMeta,
    ) -> Result<LoginOutcome, AuthError> {
        if !self.options.magic_link.enabled {
            return Err(AuthError::Disabled("magic links"));
        }
        let now = self.clock.now();
        let found = match RefreshSecret::parse(token) {
            Some(presented) => login_token::Entity::find_by_id(presented.id())
                .one(&self.db.reader().await)
                .await?
                .filter(|row| {
                    row.kind == MAGIC_LINK
                        && row.used_at.is_none()
                        && row.expires_at > now
                        && constant_time_eq(row.token_hash.as_bytes(), presented.hash().as_bytes())
                }),
            None => None,
        };
        let Some(row) = found else {
            self.audit(
                entry(AuditKind::Security, "auth.login_failed", meta)
                    .metadata(json!({ "method": MAGIC_LINK })),
            )
            .await;
            return Err(AuthError::InvalidLoginToken);
        };
        if !self.claim_login_token(row.id, now).await? {
            return Err(AuthError::InvalidLoginToken);
        }
        let user = self
            .find_user(row.user_id)
            .await?
            .ok_or(AuthError::InvalidLoginToken)?;
        self.finish_passwordless(user, MAGIC_LINK, meta, now).await
    }

    /// Mails a sign-in code to `email` (`POST /auth/otp`) when it is
    /// registered: `auth.otp.length` digits, valid `auth.otp.ttl_secs`.
    /// Unknown addresses and requests within `auth.otp.throttle_secs` of
    /// the last code do nothing. Audited as `auth.otp_sent`.
    pub async fn send_login_code(&self, email: &str, meta: &ClientMeta) -> Result<(), AuthError> {
        let options = &self.options.otp;
        if !options.enabled {
            return Err(AuthError::Disabled("sign-in codes"));
        }
        let Some(user) = self
            .passwordless_user(email, OTP, options.throttle_secs)
            .await?
        else {
            return Ok(());
        };
        let id = Uuid::now_v7();
        let code = random_digits(options.length)?;
        let digest = self.code_digest(id, &code)?;
        let mail = self
            .mailer()
            .await
            .map(|mailer| (mailer, mail::login_code(&user, &code, options)));
        self.store_login_token(
            &user,
            OTP,
            id,
            digest,
            options.ttl_secs,
            mail,
            entry(AuditKind::Auth, "auth.otp_sent", meta).actor(user.id),
        )
        .await
    }

    /// Signs in with an emailed code (`POST /auth/otp/verify`): counted by
    /// the login throttle of the address and IP before the check; a wrong
    /// code counts against the code (burned after `auth.otp.max_attempts`)
    /// and is [`AuthError::InvalidLoginToken`], as are unknown addresses
    /// and expired or used codes (audited as `auth.login_failed` with
    /// `method: otp`). Then the sign-in continues like
    /// [`attempt_login`](AuthService::attempt_login).
    pub async fn login_with_code(
        &self,
        email: &str,
        code: &str,
        meta: &ClientMeta,
    ) -> Result<LoginOutcome, AuthError> {
        let options = &self.options.otp;
        if !options.enabled {
            return Err(AuthError::Disabled("sign-in codes"));
        }
        let key = LoginThrottle::key(email, meta.ip.as_deref());
        self.throttle_attempt(
            &self.throttle,
            &key,
            meta,
            None,
            json!({ "flow": "otp", "email": normalize_email(email) }),
        )
        .await?;
        let now = self.clock.now();
        let code: String = code.chars().filter(|c| !c.is_whitespace()).collect();
        let user = self.find_by_email(email).await?;
        let pending = match &user {
            Some(user) => {
                login_token::Entity::find()
                    .filter(login_token::Column::UserId.eq(user.id))
                    .filter(login_token::Column::Kind.eq(OTP))
                    .filter(login_token::Column::UsedAt.is_null())
                    .filter(login_token::Column::ExpiresAt.gt(now))
                    .order_by_desc(login_token::Column::CreatedAt)
                    .one(&self.db.reader().await)
                    .await?
            }
            None => None,
        };
        let valid = match &pending {
            Some(row) => {
                let digest = self.code_digest(row.id, &code)?;
                constant_time_eq(row.token_hash.as_bytes(), digest.as_bytes())
            }
            None => false,
        };
        let (Some(user), Some(row), true) = (user.clone(), pending.clone(), valid) else {
            if let Some(row) = &pending {
                self.count_wrong_code(row, now, meta).await?;
            }
            let mut failed = entry(AuditKind::Security, "auth.login_failed", meta)
                .metadata(json!({ "method": OTP, "email": normalize_email(email) }));
            if let Some(user) = &user {
                failed = failed.actor(user.id);
            }
            self.audit(failed).await;
            return Err(AuthError::InvalidLoginToken);
        };
        if !self.claim_login_token(row.id, now).await? {
            return Err(AuthError::InvalidLoginToken);
        }
        if let Err(error) = self.throttle.clear(&key).await {
            tracing::warn!(target: "navree::auth", %error, "could not reset the login throttle");
        }
        self.finish_passwordless(user, OTP, meta, now).await
    }

    // -- internals ----------------------------------------------------------

    /// The registered user of `email`, unless a `kind` token was issued to
    /// them within `throttle_secs`.
    async fn passwordless_user(
        &self,
        email: &str,
        kind: &str,
        throttle_secs: u64,
    ) -> Result<Option<User>, AuthError> {
        let Some(user) = self.find_by_email(email).await? else {
            tracing::debug!(target: "navree::auth", kind, "passwordless sign-in requested for an unknown email");
            return Ok(None);
        };
        let last = login_token::Entity::find()
            .filter(login_token::Column::UserId.eq(user.id))
            .filter(login_token::Column::Kind.eq(kind))
            .order_by_desc(login_token::Column::CreatedAt)
            .one(&self.db.reader().await)
            .await?;
        if let Some(last) = last
            && self.clock.now() < after(last.created_at, throttle_secs)
        {
            tracing::debug!(target: "navree::auth", user_id = %user.id, kind, "passwordless mail throttled");
            return Ok(None);
        }
        Ok(Some(user))
    }

    /// Replaces the user's `kind` tokens with a new one and mails it after
    /// the commit.
    #[allow(clippy::too_many_arguments)]
    async fn store_login_token(
        &self,
        user: &User,
        kind: &'static str,
        id: Uuid,
        token_hash: String,
        ttl_secs: u64,
        mail: Option<(
            std::sync::Arc<navree_mail::Mailer>,
            Box<dyn navree_mail::Mailable>,
        )>,
        audit: navree_contracts::AuditEntry,
    ) -> Result<(), AuthError> {
        if mail.is_none() {
            tracing::warn!(
                target: "navree::auth",
                user_id = %user.id,
                kind,
                "no Mailer in the app (MailModule): the sign-in token was not sent"
            );
        }
        let now = self.clock.now();
        let row = login_token::ActiveModel {
            id: Set(id),
            user_id: Set(user.id),
            kind: Set(kind.to_owned()),
            token_hash: Set(token_hash),
            attempts: Set(0),
            expires_at: Set(after(now, ttl_secs)),
            used_at: Set(None),
            created_at: Set(now),
        };
        let user_id = user.id;
        self.write(Some(user_id), |uow| async move {
            {
                let tx = uow.tx().await?;
                login_token::Entity::delete_many()
                    .filter(login_token::Column::UserId.eq(user_id))
                    .filter(login_token::Column::Kind.eq(kind))
                    .exec(&*tx)
                    .await?;
                row.insert(&*tx).await?;
            }
            if let Some((mailer, mail)) = mail {
                send_after_commit(&uow, mailer, mail);
            }
            self.audit_after_commit(&uow, audit).await;
            Ok(())
        })
        .await
    }

    /// Marks the token used; `false` when a concurrent request got there
    /// first.
    async fn claim_login_token(&self, id: Uuid, now: DateTime<Utc>) -> Result<bool, AuthError> {
        self.write(None, |uow| async move {
            let tx = uow.tx().await?;
            let result = login_token::Entity::update_many()
                .col_expr(login_token::Column::UsedAt, Expr::value(now))
                .filter(login_token::Column::Id.eq(id))
                .filter(login_token::Column::UsedAt.is_null())
                .exec(&*tx)
                .await?;
            Ok(result.rows_affected == 1)
        })
        .await
    }

    /// Counts a wrong guess of `row`, burning it at the limit, in its own
    /// transaction (the failed request rolls its own back).
    async fn count_wrong_code(
        &self,
        row: &login_token::Model,
        now: DateTime<Utc>,
        meta: &ClientMeta,
    ) -> Result<(), AuthError> {
        let attempts = row.attempts.saturating_add(1);
        let burned = u32::try_from(attempts).unwrap_or(u32::MAX) >= self.options.otp.max_attempts;
        let id = row.id;
        self.own(
            Some(row.user_id),
            meta.request_id.clone(),
            |uow| async move {
                let tx = uow.tx().await?;
                let mut update = login_token::Entity::update_many()
                    .col_expr(login_token::Column::Attempts, Expr::value(attempts))
                    .filter(login_token::Column::Id.eq(id));
                if burned {
                    update = update.col_expr(login_token::Column::UsedAt, Expr::value(now));
                }
                update.exec(&*tx).await?;
                Ok(())
            },
        )
        .await
    }

    /// Verifies the address when it is not yet, then signs in like a
    /// password login (a challenge with two-factor authentication on).
    async fn finish_passwordless(
        &self,
        mut user: User,
        method: &'static str,
        meta: &ClientMeta,
        now: DateTime<Utc>,
    ) -> Result<LoginOutcome, AuthError> {
        if user.email_verified_at.is_none() {
            let row = user::ActiveModel {
                id: sea_orm::Unchanged(user.id),
                email_verified_at: Set(Some(now)),
                updated_at: Set(now),
                ..Default::default()
            };
            user = self
                .write(Some(user.id), |uow| async move {
                    crate::account::update_user(&uow, row).await
                })
                .await?;
        }
        if two_factor_enabled(&user) {
            let challenge_token = self.sign_challenge(&user)?;
            self.audit(
                entry(AuditKind::Auth, "auth.two_factor_challenge", meta)
                    .actor(user.id)
                    .metadata(json!({ "method": method })),
            )
            .await;
            return Ok(LoginOutcome::TwoFactorRequired {
                user,
                challenge_token,
            });
        }
        let tokens = self.issue_tokens(&user, meta.clone()).await?;
        self.audit(
            entry(AuditKind::Auth, "auth.login", meta)
                .actor(user.id)
                .metadata(json!({ "method": method })),
        )
        .await;
        Ok(LoginOutcome::Authenticated { user, tokens })
    }

    /// The stored digest of code `code` of token `id`.
    fn code_digest(&self, id: Uuid, code: &str) -> Result<String, AuthError> {
        let key = self.encrypter.derive_key(OTP_KEY_PURPOSE)?;
        let mac = hmac_sha256(&key, format!("{id}:{code}").as_bytes());
        Ok(sha256_hex(&mac))
    }
}

/// `len` random decimal digits (the operating system's CSPRNG).
fn random_digits(len: usize) -> Result<String, AuthError> {
    use ring::rand::{SecureRandom, SystemRandom};
    let rng = SystemRandom::new();
    let mut out = String::with_capacity(len);
    while out.len() < len {
        let mut byte = [0_u8; 1];
        rng.fill(&mut byte)
            .map_err(|_| AuthError::Signing("the system random generator failed".to_owned()))?;
        // 250 = 25 * 10: rejecting 250..=255 keeps every digit equally likely.
        if byte[0] < 250 {
            out.push(char::from(b'0' + byte[0] % 10));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_digits_of_the_requested_length() {
        for len in [4, 6, 10] {
            let code = random_digits(len).unwrap();
            assert_eq!(code.len(), len);
            assert!(code.chars().all(|c| c.is_ascii_digit()), "{code}");
        }
        let codes: std::collections::HashSet<String> =
            (0..50).map(|_| random_digits(8).unwrap()).collect();
        assert!(codes.len() > 45, "codes repeat too often");
    }
}
