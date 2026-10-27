//! Account flows on [`AuthService`]: password reset, email verification and
//! profile changes.

use std::sync::Arc;

use chrono::{DateTime, TimeDelta, Utc};
use navree_config::Config;
use navree_contracts::{AuditEntry, AuditKind};
use navree_crypto::{CryptoError, constant_time_eq, random_token, sha256_hex};
use navree_db::{Repo, Uow};
use navree_http::{HttpOptions, UrlSigner};
use navree_mail::{Mailable, Mailer};
use navree_orm::Model as _;
use sea_orm::{DbErr, Set, Unchanged};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::password_reset_token::Model as PasswordResetToken;
use crate::service::{entry, invalid, normalize_email, record, revoke_user};
use crate::throttle::LoginThrottle;
use crate::{AuthError, AuthService, ClientMeta, TokenPair, USER_SUBJECT, User, mail, user};

/// Random characters in a password reset token (about 380 bits).
const RESET_TOKEN_LEN: usize = 64;

impl AuthService {
    /// Starts a password reset (`POST /auth/forgot-password`): for a
    /// registered `email`, stores a new reset token (its SHA-256 only,
    /// replacing any earlier one) and mails it, once the unit of work
    /// commits, through the app's `Mailer` when there is one. Unknown
    /// emails do nothing, and so does a second request within
    /// `auth.password_reset.throttle_secs`, so the caller can always answer
    /// the same. Audited as `auth.password_reset_requested`.
    pub async fn send_password_reset_link(
        &self,
        email: &str,
        meta: &ClientMeta,
    ) -> Result<(), AuthError> {
        let Some(user) = self.find_by_email(email).await? else {
            tracing::debug!(target: "navree::auth", "password reset requested for an unknown email");
            return Ok(());
        };
        let now = self.clock.now();
        let options = &self.options.password_reset;
        let pending = PasswordResetToken::find(&self.db, user.email.clone()).await?;
        if let Some(pending) = pending
            && now < after(pending.created_at, options.throttle_secs)
        {
            tracing::debug!(target: "navree::auth", user_id = %user.id, "password reset mail throttled");
            return Ok(());
        }
        let token = random_token(RESET_TOKEN_LEN);
        let mail = self
            .mailer()
            .await
            .map(|mailer| (mailer, mail::password_reset(&user, &token, options)));
        if mail.is_none() {
            tracing::warn!(
                target: "navree::auth",
                user_id = %user.id,
                "no Mailer in the app (MailModule): the password reset token was not sent"
            );
        }
        let row = PasswordResetToken {
            email: user.email.clone(),
            token_hash: sha256_hex(token.as_bytes()),
            created_at: now,
        };
        let audit = entry(AuditKind::Auth, "auth.password_reset_requested", meta).actor(user.id);
        self.write(Some(user.id), |uow| async move {
            {
                let tx = uow.tx().await?;
                PasswordResetToken::query()
                    .where_key(user.email.clone())
                    .delete(&*tx)
                    .await?;
                PasswordResetToken::insert(&*tx, [row]).await?;
            }
            if let Some((mailer, mail)) = mail {
                send_after_commit(&uow, mailer, mail);
            }
            self.audit_after_commit(&uow, audit).await;
            Ok(())
        })
        .await
    }

    /// Finishes a password reset (`POST /auth/reset-password`): checks
    /// `token` against the pending reset of `email` (single use, valid for
    /// `auth.password_reset.ttl_secs`) and the password policy, sets the
    /// new password and revokes every refresh token of the user (signing
    /// out every session). [`AuthError::InvalidResetToken`] for a wrong,
    /// used or expired token. Audited as `auth.password_reset`.
    pub async fn reset_password(
        &self,
        email: &str,
        token: &str,
        new_password: &str,
        meta: &ClientMeta,
    ) -> Result<(), AuthError> {
        self.check_password(new_password)?;
        let email = normalize_email(email);
        let presented = sha256_hex(token.as_bytes());
        let now = self.clock.now();
        let pending = PasswordResetToken::find(&self.db, email.clone())
            .await?
            .ok_or(AuthError::InvalidResetToken)?;
        if !constant_time_eq(pending.token_hash.as_bytes(), presented.as_bytes())
            || now >= after(pending.created_at, self.options.password_reset.ttl_secs)
        {
            return Err(AuthError::InvalidResetToken);
        }
        let user = self
            .find_by_email(&email)
            .await?
            .ok_or(AuthError::InvalidResetToken)?;
        let hash = self.hasher.hash(new_password).await?;
        let audit = entry(AuditKind::Auth, "auth.password_reset", meta).actor(user.id);
        self.write(Some(user.id), |uow| async move {
            {
                // Claim the token: a concurrent reset with it finds nothing.
                let tx = uow.tx().await?;
                let claimed = PasswordResetToken::query()
                    .where_key(email.clone())
                    .where_(PasswordResetToken::TOKEN_HASH.eq(presented.as_str()))
                    .delete(&*tx)
                    .await?;
                if claimed != 1 {
                    return Err(AuthError::InvalidResetToken);
                }
                revoke_user(&*tx, user.id, now).await?;
            }
            update_user(
                &uow,
                user::ActiveModel {
                    id: Unchanged(user.id),
                    password_hash: Set(hash),
                    updated_at: Set(now),
                    ..Default::default()
                },
            )
            .await?;
            self.audit_after_commit(&uow, audit).await;
            Ok(())
        })
        .await
    }

    /// The signed link verifying `user`'s current email address:
    /// `{base}{api_prefix}{routes_prefix}/email/verify/{id}/{sha256(email)}?expires=..&signature=..`,
    /// signed with the app's `UrlSigner` (navree-http) and valid for
    /// `auth.verification.ttl_secs`. `{base}` is `auth.verification.base_url`,
    /// else the `app.url` setting, else empty (a relative link).
    pub async fn verification_url(&self, user: &User) -> Result<String, AuthError> {
        let app = self.app.upgrade().ok_or(AuthError::ShutDown)?;
        let signer = app.get::<UrlSigner>().await?;
        let api_prefix = match app.try_get::<HttpOptions>().await {
            Some(options) => options.api_prefix.clone(),
            None => String::new(),
        };
        let id = user.id.to_string();
        let hash = email_hash(&user.email);
        let path = join_path(&[
            &api_prefix,
            &self.options.routes_prefix,
            "email/verify",
            &id,
            &hash,
        ]);
        let options = &self.options.verification;
        let signed = signer.sign(&path, after(self.clock.now(), options.ttl_secs));
        let base = match &options.base_url {
            Some(base) => Some(base.clone()),
            None => match app.try_get::<Config>().await {
                Some(config) => config.get_string("app.url"),
                None => None,
            },
        };
        Ok(format!(
            "{}{signed}",
            base.as_deref().unwrap_or_default().trim_end_matches('/')
        ))
    }

    /// Mails `user` a [`verification_url`](AuthService::verification_url)
    /// (`POST /auth/email/verification`), after the current unit of work
    /// commits when there is one. `false` (nothing sent) when the address is
    /// already verified. Without a `Mailer` in the app nothing is sent and a
    /// warning is logged. Audited as `auth.email_verification_sent`.
    pub async fn send_verification_email(&self, user: &User) -> Result<bool, AuthError> {
        if user.email_verified_at.is_some() {
            return Ok(false);
        }
        let url = self.verification_url(user).await?;
        let Some(mailer) = self.mailer().await else {
            tracing::warn!(
                target: "navree::auth",
                user_id = %user.id,
                "no Mailer in the app (MailModule): the verification link was not sent"
            );
            return Ok(true);
        };
        let mail = mail::verification(user, &url, &self.options.verification);
        let mut audit = AuditEntry::new(AuditKind::Auth, "auth.email_verification_sent")
            .actor(user.id)
            .subject(USER_SUBJECT, user.id.to_string());
        match Uow::current() {
            Some(uow) if !uow.is_finished().await => {
                if let Some(request_id) = uow.request_id() {
                    audit = audit.request_id(request_id);
                }
                send_after_commit(&uow, mailer, mail);
                self.audit_after_commit(&uow, audit).await;
            }
            _ => {
                mailer.queue(&*mail).await?;
                self.audit(audit).await;
            }
        }
        Ok(true)
    }

    /// Marks the user's email address verified (the signed
    /// `GET /auth/email/verify/{id}/{hash}` link; the route checks the
    /// signature): [`AuthError::InvalidVerificationLink`] unless `hash` is
    /// the SHA-256 of the user's current address. Verifying twice is
    /// harmless. Audited as `auth.email_verified`.
    pub async fn verify_email(
        &self,
        user_id: Uuid,
        hash: &str,
        meta: &ClientMeta,
    ) -> Result<User, AuthError> {
        let user = self
            .find_user(user_id)
            .await?
            .ok_or(AuthError::InvalidVerificationLink)?;
        if !constant_time_eq(email_hash(&user.email).as_bytes(), hash.as_bytes()) {
            return Err(AuthError::InvalidVerificationLink);
        }
        if user.email_verified_at.is_some() {
            return Ok(user);
        }
        let now = self.clock.now();
        let audit = entry(AuditKind::Auth, "auth.email_verified", meta)
            .actor(user.id)
            .subject(USER_SUBJECT, user.id.to_string());
        self.write(Some(user.id), |uow| async move {
            let user = update_user(
                &uow,
                user::ActiveModel {
                    id: Unchanged(user.id),
                    email_verified_at: Set(Some(now)),
                    updated_at: Set(now),
                    ..Default::default()
                },
            )
            .await?;
            self.audit_after_commit(&uow, audit).await;
            Ok(user)
        })
        .await
    }

    /// Changes the password of `user_id` (`PATCH /auth/me/password`):
    /// checks `current_password` (a field error on `current_password` when
    /// wrong; counted by the login throttle of the user's email and the
    /// client's IP) and the password policy, sets the new password, revokes
    /// every refresh token of the user (signing out every other session),
    /// cancels a pending password reset and returns a fresh token pair for
    /// the caller. Audited as `auth.password_changed`.
    pub async fn change_password(
        &self,
        user_id: Uuid,
        current_password: &str,
        new_password: &str,
        meta: ClientMeta,
    ) -> Result<TokenPair, AuthError> {
        let user = self
            .find_user(user_id)
            .await?
            .ok_or(AuthError::UserNotFound)?;
        self.confirm_password(&user, current_password, "current_password", &meta)
            .await?;
        self.check_password(new_password)?;
        let hash = self.hasher.hash(new_password).await?;
        let now = self.clock.now();
        let audit = entry(AuditKind::Auth, "auth.password_changed", &meta)
            .actor(user.id)
            .subject(USER_SUBJECT, user.id.to_string());
        self.write(Some(user.id), |uow| async move {
            {
                let tx = uow.tx().await?;
                revoke_user(&*tx, user.id, now).await?;
                PasswordResetToken::query()
                    .where_key(user.email.clone())
                    .delete(&*tx)
                    .await?;
            }
            let user = update_user(
                &uow,
                user::ActiveModel {
                    id: Unchanged(user.id),
                    password_hash: Set(hash),
                    updated_at: Set(now),
                    ..Default::default()
                },
            )
            .await?;
            let tokens = self.issue_tokens(&user, meta).await?;
            self.audit_after_commit(&uow, audit).await;
            Ok(tokens)
        })
        .await
    }

    /// Changes the name and/or email address of `user_id` (`PATCH
    /// /auth/me`). The name is trimmed and must not be empty; the address
    /// is stored trimmed and lower-cased. A new address needs the user's
    /// `current_password` (a field error on `current_password` when missing
    /// or wrong, counted by the login throttle), so a stolen access token
    /// cannot move the account to an address whose password reset mail the
    /// thief reads. It must be free ([`AuthError::EmailTaken`]) and is
    /// unverified until the user follows the verification link mailed to
    /// it; the old address is told about the change (both when the app has
    /// a `Mailer`), and a pending password reset of the old address is
    /// cancelled. Audited as `auth.profile_updated` with the old and new
    /// values of the changed fields.
    pub async fn update_profile(
        &self,
        user_id: Uuid,
        update: ProfileUpdate,
        meta: &ClientMeta,
    ) -> Result<User, AuthError> {
        let user = self
            .find_user(user_id)
            .await?
            .ok_or(AuthError::UserNotFound)?;
        let ProfileUpdate {
            name,
            email,
            current_password,
        } = update;
        let now = self.clock.now();
        let mut row = user::ActiveModel {
            id: Unchanged(user.id),
            ..Default::default()
        };
        let (mut old, mut new) = (Map::new(), Map::new());
        if let Some(name) = name {
            let name = name.trim();
            if name.is_empty() {
                return Err(invalid("name", "required", &[]));
            }
            if name != user.name {
                old.insert("name".to_owned(), json!(user.name));
                new.insert("name".to_owned(), json!(name));
                row.name = Set(name.to_owned());
            }
        }
        let mut new_email = None;
        if let Some(email) = email {
            let email = normalize_email(&email);
            if email.is_empty() || !email.contains('@') {
                return Err(invalid("email", "email", &[]));
            }
            if email != user.email {
                // Re-authentication first, so a bearer token alone learns
                // nothing (not even whether the address is taken).
                let Some(password) = current_password.as_deref().filter(|p| !p.is_empty()) else {
                    return Err(invalid("current_password", "required", &[]));
                };
                self.confirm_password(&user, password, "current_password", meta)
                    .await?;
                old.insert("email".to_owned(), json!(user.email));
                new.insert("email".to_owned(), json!(email));
                row.email = Set(email.clone());
                row.email_verified_at = Set(None);
                new_email = Some(email);
            }
        }
        if new.is_empty() {
            return Ok(user);
        }
        row.updated_at = Set(now);
        let mail = match &new_email {
            Some(email) => self.verification_mail_for(&user, email).await?,
            None => None,
        };
        // The old address hears of the change, so a takeover does not go
        // unnoticed.
        let notice = match &new_email {
            Some(email) => self
                .mailer()
                .await
                .map(|mailer| (mailer, mail::email_changed(&user, email))),
            None => None,
        };
        let audit = entry(AuditKind::Auth, "auth.profile_updated", meta)
            .actor(user.id)
            .subject(USER_SUBJECT, user.id.to_string())
            .old_values(Value::Object(old))
            .new_values(Value::Object(new));
        self.write(Some(user.id), |uow| async move {
            if let Some(email) = &new_email {
                let taken = {
                    let tx = uow.tx().await?;
                    User::query()
                        .where_(User::EMAIL.eq(email.as_str()))
                        .where_key_not(user.id)
                        .exists(&*tx)
                        .await?
                };
                if taken {
                    return Err(AuthError::EmailTaken);
                }
                // A reset token mailed to the old address must not reset
                // whoever takes that address next.
                let tx = uow.tx().await?;
                PasswordResetToken::query()
                    .where_key(user.email.clone())
                    .delete(&*tx)
                    .await?;
            }
            let user = update_user(&uow, row).await?;
            if let Some((mailer, mail)) = mail {
                send_after_commit(&uow, mailer, mail);
            }
            if let Some((mailer, notice)) = notice {
                send_after_commit(&uow, mailer, notice);
            }
            self.audit_after_commit(&uow, audit).await;
            Ok(user)
        })
        .await
    }

    // -- internals ----------------------------------------------------------

    /// Checks `password` against `user`'s (re-authentication for sensitive
    /// changes): a field error on `field` when it is wrong. Wrong passwords
    /// count towards the login throttle of the user's email and the
    /// client's IP, which refuses further attempts with
    /// [`AuthError::TooManyAttempts`].
    pub(crate) async fn confirm_password(
        &self,
        user: &User,
        password: &str,
        field: &'static str,
        meta: &ClientMeta,
    ) -> Result<(), AuthError> {
        let key = LoginThrottle::key(&user.email, meta.ip.as_deref());
        // Counted before the (slow) check, so parallel guesses cannot all
        // pass; a correct password gives the slot back.
        self.throttle_attempt(
            &self.throttle,
            &key,
            meta,
            Some(user.id),
            json!({ "flow": "password_confirmation", "field": field }),
        )
        .await?;
        let valid = match self.hasher.verify(password, &user.password_hash).await {
            Ok(valid) => valid,
            Err(CryptoError::InvalidHash) => false,
            Err(error) => return Err(error.into()),
        };
        if valid {
            if let Err(error) = self.throttle.release(&key).await {
                tracing::warn!(target: "navree::auth", %error, "could not release a password confirmation attempt");
            }
            return Ok(());
        }
        self.audit(
            entry(
                AuditKind::Security,
                "auth.password_confirmation_failed",
                meta,
            )
            .actor(user.id)
            .metadata(json!({ "field": field })),
        )
        .await;
        Err(AuthError::Invalid {
            field,
            message: format!("{field} is incorrect"),
        })
    }

    /// The verification mail for `user` once their address is `email`, if
    /// the app can send mail.
    async fn verification_mail_for(
        &self,
        user: &User,
        email: &str,
    ) -> Result<Option<(Arc<Mailer>, Box<dyn Mailable>)>, AuthError> {
        let Some(mailer) = self.mailer().await else {
            return Ok(None);
        };
        let pending = User {
            email: email.to_owned(),
            email_verified_at: None,
            ..user.clone()
        };
        let url = self.verification_url(&pending).await?;
        let mail = mail::verification(&pending, &url, &self.options.verification);
        Ok(Some((mailer, mail)))
    }

    /// The app's `Mailer`, when MailModule is in the app.
    pub(crate) async fn mailer(&self) -> Option<Arc<Mailer>> {
        self.app.upgrade()?.try_get::<Mailer>().await
    }

    /// Records `entry` once `uow` commits (dropped on rollback), with the
    /// unit of work's impersonator when the entry has none.
    pub(crate) async fn audit_after_commit(&self, uow: &Uow, mut entry: AuditEntry) {
        if entry.impersonator_id.is_none() {
            entry.impersonator_id = uow.impersonator();
        }
        if let Some(sink) = self.audit_sink().await {
            uow.after_commit(move || Box::pin(record(sink, entry)));
        }
    }
}

/// Sends `mail` once `uow` commits; a failure is logged (the request has
/// succeeded by then).
pub(crate) fn send_after_commit(uow: &Uow, mailer: Arc<Mailer>, mail: Box<dyn Mailable>) {
    uow.after_commit(move || {
        Box::pin(async move {
            if let Err(error) = mailer.queue(&*mail).await {
                tracing::warn!(target: "navree::auth", %error, "could not send an account mail");
            }
        })
    });
}

/// The changes of [`AuthService::update_profile`]; `None` keeps a field.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct ProfileUpdate {
    /// The new display name.
    pub name: Option<String>,
    /// The new email address.
    pub email: Option<String>,
    /// The password in use: required when the email address changes.
    pub current_password: Option<String>,
}

/// The password is a credential: never in debug output.
impl std::fmt::Debug for ProfileUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProfileUpdate")
            .field("name", &self.name)
            .field("email", &self.email)
            .field(
                "current_password",
                &self.current_password.as_ref().map(|_| "[redacted]"),
            )
            .finish()
    }
}

/// Updates a user row through `uow` (recording the change), mapping a
/// missing row to [`AuthError::UserNotFound`].
pub(crate) async fn update_user(uow: &Uow, row: user::ActiveModel) -> Result<User, AuthError> {
    Repo::<user::Entity>::update(uow, row)
        .await
        .map_err(|error| match error {
            DbErr::RecordNotFound(_) | DbErr::RecordNotUpdated => AuthError::UserNotFound,
            error => error.into(),
        })
}

/// The hash in verification links: SHA-256 of the address, lower-case hex.
fn email_hash(email: &str) -> String {
    sha256_hex(email.as_bytes())
}

/// `parts` joined with single slashes, with a leading slash and no trailing
/// one.
fn join_path(parts: &[&str]) -> String {
    let mut out = String::new();
    for segment in parts.iter().flat_map(|part| part.split('/')) {
        if !segment.is_empty() {
            out.push('/');
            out.push_str(segment);
        }
    }
    if out.is_empty() {
        out.push('/');
    }
    out
}

/// `at` plus `secs` seconds (saturating).
pub(crate) fn after(at: DateTime<Utc>, secs: u64) -> DateTime<Utc> {
    let secs = i64::try_from(secs).unwrap_or(i64::MAX);
    at.checked_add_signed(TimeDelta::try_seconds(secs).unwrap_or(TimeDelta::MAX))
        .unwrap_or(DateTime::<Utc>::MAX_UTC)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn paths_join_with_single_slashes() {
        assert_eq!(
            join_path(&["/api/", "/auth", "email/verify", "1", "ab"]),
            "/api/auth/email/verify/1/ab"
        );
        assert_eq!(join_path(&["", "", "x"]), "/x");
        assert_eq!(join_path(&[""]), "/");
    }

    #[test]
    fn after_adds_seconds_and_saturates() {
        let at = Utc.with_ymd_and_hms(2026, 10, 25, 12, 0, 0).unwrap();
        assert_eq!(
            after(at, 90),
            Utc.with_ymd_and_hms(2026, 10, 25, 12, 1, 30).unwrap()
        );
        assert_eq!(after(at, u64::MAX), DateTime::<Utc>::MAX_UTC);
    }
}
