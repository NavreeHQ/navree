//! Confirming a sensitive action with a passkey: a user-verified assertion
//! over a challenge derived from the action's own details, used once,
//! within [`CONFIRM_TTL_SECS`] seconds, by the user and the session it was
//! issued to.
//!
//! The challenge is `SHA-256("navree-confirm-v1" || nonce || digest)`,
//! where `digest` is the [`ConfirmAction`]'s: who it is for, what is done,
//! to which record, for how much. The handler that does the action builds
//! the same [`ConfirmAction`] from its own data and passes it to
//! [`AuthService::confirm_action`], so a confirmation of one thing (₹100,
//! order A-1042) never approves another (₹1,00,000, order A-1043).

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use chrono::TimeDelta;
use navree_contracts::AuditKind;
use navree_crypto::{random_token, sha256_hex};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use super::webauthn::{ClientData, sha256};
use super::{base64_url, refused};
use crate::service::entry;
use crate::{AuthError, AuthService, ClientMeta, User};

/// How long a confirmation challenge can be answered, in seconds.
pub const CONFIRM_TTL_SECS: u64 = 120;

/// Domain separation of confirmation challenges.
const CHALLENGE_DOMAIN: &[u8] = b"navree-confirm-v1";

/// Domain separation of action digests.
const ACTION_DOMAIN: &[u8] = b"navree-action-v1";

/// The action of a plain step-up (`POST /auth/confirm/passkey`): no record,
/// no amount.
pub(crate) const STEP_UP_ACTION: &str = "auth.confirm";

/// The details of a sensitive action a confirmation approves: a name (what
/// is done) and fields (to which record, for how much, to whom). Its
/// [`digest`](ConfirmAction::digest) changes with every one of them, so a
/// confirmation is good for exactly one action.
///
/// ```
/// use navree_auth::ConfirmAction;
///
/// let pay = |amount: u64| {
///     ConfirmAction::new("payments.approve")
///         .field("payment", "PAY-2024")
///         .field("amount_paise", amount)
///         .field("payee", "Gurpreet Kaur, HDFC ••4821")
/// };
/// assert_eq!(pay(10_000_000).digest(), pay(10_000_000).digest());
/// assert_ne!(pay(10_000_000).digest(), pay(10_000).digest());
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct ConfirmAction {
    name: String,
    fields: BTreeMap<String, String>,
}

impl ConfirmAction {
    /// The action `name` (`"payments.approve"`), with no fields yet.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            fields: BTreeMap::new(),
        }
    }

    /// Adds the detail `key` (`"amount_paise"`) with `value`, replacing an
    /// earlier value of the same key. The order fields are added in does
    /// not matter.
    #[must_use]
    pub fn field(mut self, key: impl Into<String>, value: impl fmt::Display) -> Self {
        self.fields.insert(key.into(), value.to_string());
        self
    }

    /// The action's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The SHA-256 (hex) of the name and the fields, each length-prefixed so
    /// no two different actions share one.
    pub fn digest(&self) -> String {
        let mut bytes = ACTION_DOMAIN.to_vec();
        let mut push = |part: &str| {
            let len = u32::try_from(part.len()).unwrap_or(u32::MAX);
            bytes.extend_from_slice(&len.to_be_bytes());
            bytes.extend_from_slice(part.as_bytes());
        };
        push(&self.name);
        for (key, value) in &self.fields {
            push(key);
            push(value);
        }
        sha256_hex(&bytes)
    }
}

/// The details may be private (amounts, payees): only the name and the
/// field names in debug output.
impl fmt::Debug for ConfirmAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConfirmAction")
            .field("name", &self.name)
            .field("fields", &self.fields.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// A confirmation waiting for its assertion, kept in the cache under its
/// challenge.
#[derive(Serialize, Deserialize)]
struct Pending {
    user_id: Uuid,
    session_id: Option<Uuid>,
    /// The digest of the action.
    action: String,
    /// When it stops being accepted (Unix seconds, the app's clock).
    expires_at: i64,
}

fn pending_key(challenge: &str) -> String {
    format!("navree-auth:confirm:{}", sha256_hex(challenge.as_bytes()))
}

/// Whether `digest` looks like an action digest (64 lower-case hex).
pub(crate) fn is_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

impl AuthService {
    /// The `publicKey` options of a confirmation of `action` by user
    /// `user_id` in session `session_id` (the `sid` of their access claims):
    /// a challenge derived from the action, valid [`CONFIRM_TTL_SECS`]
    /// seconds, the user's passkeys as `allowCredentials`, and
    /// `userVerification: "required"`. Answer it with
    /// `navigator.credentials.get({publicKey})` on the web, or the system's
    /// passkey request in an app, and pass the assertion to
    /// [`confirm_action`](AuthService::confirm_action) with the same action.
    /// [`AuthError::Disabled`] without `auth.passkeys.enabled`.
    pub async fn confirmation_options(
        &self,
        user_id: Uuid,
        session_id: Option<Uuid>,
        action: &ConfirmAction,
    ) -> Result<Value, AuthError> {
        self.confirmation_options_for(user_id, session_id, action.digest())
            .await
    }

    /// [`confirmation_options`](AuthService::confirmation_options) for an
    /// action known by its digest (a ticket's binding).
    pub(crate) async fn confirmation_options_for(
        &self,
        user_id: Uuid,
        session_id: Option<Uuid>,
        digest: String,
    ) -> Result<Value, AuthError> {
        let options = self.passkey_options()?;
        let mut seed = CHALLENGE_DOMAIN.to_vec();
        seed.extend_from_slice(random_token(43).as_bytes());
        seed.extend_from_slice(digest.as_bytes());
        let challenge = base64_url(&sha256(&seed));
        let ttl = TimeDelta::seconds(i64::try_from(CONFIRM_TTL_SECS).unwrap_or(120));
        self.cache
            .put(
                &pending_key(&challenge),
                &Pending {
                    user_id,
                    session_id,
                    action: digest,
                    expires_at: (self.clock.now() + ttl).timestamp(),
                },
                Duration::from_secs(CONFIRM_TTL_SECS + 5),
            )
            .await?;
        let allow: Vec<Value> = self
            .passkeys(user_id)
            .await?
            .iter()
            .map(|passkey| {
                let transports: Vec<String> = passkey
                    .transports
                    .as_deref()
                    .map(|t| t.split(',').map(str::to_owned).collect())
                    .unwrap_or_default();
                json!({
                    "type": "public-key",
                    "id": passkey.credential_id,
                    "transports": transports,
                })
            })
            .collect();
        Ok(json!({
            "challenge": challenge,
            "rpId": options.rp_id,
            "timeout": CONFIRM_TTL_SECS * 1000,
            "userVerification": "required",
            "allowCredentials": allow,
        }))
    }

    /// Checks the passkey assertion `credential` as a confirmation of
    /// `action` by `user` in session `session_id`. Refusals are
    /// [`AuthError::ConfirmationRefused`], logged and audited as
    /// `auth.confirmation_refused` with the reason; the challenge is used up
    /// by the first attempt, right or wrong. The passkey's id.
    pub(crate) async fn check_confirmation(
        &self,
        user: &User,
        session_id: Option<Uuid>,
        action: &ConfirmAction,
        credential: &Value,
        meta: &ClientMeta,
    ) -> Result<Uuid, AuthError> {
        match self
            .confirmation_assertion(user, session_id, action, credential)
            .await
        {
            Ok(passkey) => Ok(passkey),
            Err(AuthError::PasskeyRefused(reason) | AuthError::ConfirmationRefused(reason)) => {
                tracing::warn!(
                    target: "navree::security",
                    user_id = %user.id,
                    action = action.name(),
                    %reason,
                    "a confirmation was refused"
                );
                self.audit(
                    entry(AuditKind::Security, "auth.confirmation_refused", meta)
                        .actor(user.id)
                        .metadata(json!({
                            "method": "passkey",
                            "action": action.name(),
                            "reason": reason,
                            "session_id": session_id,
                        })),
                )
                .await;
                Err(AuthError::ConfirmationRefused(reason))
            }
            Err(error) => Err(error),
        }
    }

    async fn confirmation_assertion(
        &self,
        user: &User,
        session_id: Option<Uuid>,
        action: &ConfirmAction,
        credential: &Value,
    ) -> Result<Uuid, AuthError> {
        let options = self.passkey_options()?;
        let (credential, stored, client_json) = self.assertion_parts(credential).await?;
        let client =
            ClientData::parse(&client_json).ok_or_else(|| refused("malformed client data"))?;
        if client.kind != "webauthn.get" {
            return Err(refused(format!("client data of type {}", client.kind)));
        }
        if client.cross_origin {
            return Err(refused("cross-origin ceremony"));
        }
        if !self.passkey_origins.contains(&client.origin) {
            return Err(refused(format!("origin {} not allowed", client.origin)));
        }
        // The challenge is used up now, whatever follows.
        let key = pending_key(&client.challenge);
        let pending: Pending = self
            .cache
            .get(&key)
            .await?
            .ok_or_else(|| refused("unknown or expired challenge"))?;
        if !self.cache.forget(&key).await? {
            return Err(refused("challenge already used"));
        }
        if self.clock.now().timestamp() >= pending.expires_at {
            return Err(refused("challenge expired"));
        }
        if pending.user_id != user.id {
            return Err(refused("challenge of another user"));
        }
        if pending.session_id != session_id {
            return Err(refused("challenge of another session"));
        }
        if pending.action != action.digest() {
            return Err(refused("challenge of another action"));
        }
        if stored.user_id != user.id {
            return Err(refused("a passkey of another user"));
        }
        let id = stored.id;
        self.finish_assertion(&credential, stored, &client_json, options, true)
            .await?;
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_digests_change_with_every_detail() {
        let base = ConfirmAction::new("payments.approve")
            .field("payment", "PAY-2024")
            .field("amount_paise", 10_000_000);
        let digest = base.digest();
        assert!(is_digest(&digest));
        // Field order does not matter; every value, key and the name do.
        let reordered = ConfirmAction::new("payments.approve")
            .field("amount_paise", 10_000_000)
            .field("payment", "PAY-2024");
        assert_eq!(reordered.digest(), digest);
        for other in [
            base.clone().field("amount_paise", 10_000),
            base.clone().field("payment", "PAY-2025"),
            base.clone().field("payee", "Gurpreet Kaur"),
            ConfirmAction::new("payments.refund")
                .field("payment", "PAY-2024")
                .field("amount_paise", 10_000_000),
            // Length prefixes: moving characters between parts changes it.
            ConfirmAction::new("payments.approve")
                .field("payment", "PAY-202")
                .field("4amount_paise", 10_000_000),
            ConfirmAction::new("payments.approvepayment"),
        ] {
            assert_ne!(other.digest(), digest, "{other:?}");
        }
        assert!(!format!("{base:?}").contains("10000000"));
        assert!(!is_digest("ABC"));
        assert!(!is_digest(&digest.to_uppercase()));
    }
}
