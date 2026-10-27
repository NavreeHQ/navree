//! Passkeys on [`AuthService`]: WebAuthn registration and sign-in with
//! ES256 (P-256) and EdDSA (Ed25519) credentials, verified with `ring`.
//!
//! - **Registration** (signed in, not while impersonating): `POST
//!   /auth/passkeys/register/options` returns the `publicKey` options for
//!   `navigator.credentials.create()`; `POST /auth/passkeys/register`
//!   checks the client data (type, single-use challenge, origin in
//!   `auth.passkeys.origins`) and the authenticator data (the RP id hash
//!   of `auth.passkeys.rp_id`, user presence, user verification when
//!   required, an ES256 or EdDSA key) and stores the credential. No
//!   attestation is requested (`attestation: "none"`), so attestation
//!   statements are not checked.
//! - **Sign-in** (public): `POST /auth/passkeys/login/options` (an email
//!   narrows `allowCredentials`; without it the browser offers discoverable
//!   passkeys), then `POST /auth/passkeys/login` checks the assertion the
//!   same way plus the signature over `authenticatorData ||
//!   SHA-256(clientDataJSON)` and the signature counter (a counter that
//!   does not grow is refused as a possibly cloned authenticator). A
//!   passkey with user verification is two factors: the session starts
//!   directly; without it, a user with two-factor authentication gets the
//!   usual challenge.
//! - `GET /auth/passkeys` lists the user's passkeys and `DELETE
//!   /auth/passkeys/{id}` removes one.
//! - [`VirtualAuthenticator`](crate::testing::VirtualAuthenticator) plays
//!   the browser and the authenticator in tests.

mod cbor;
pub(crate) mod controller;
pub(crate) mod webauthn;

use std::fmt;

use chrono::{DateTime, Utc};
use navree_contracts::AuditKind;
use navree_crypto::{random_token, sha256_hex};
use navree_orm::{Model as _, Record};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;
use uuid::Uuid;

pub(crate) use self::cbor::Cbor;
use self::webauthn::{
    AuthenticatorData, ClientData, CredentialKey, EDDSA, ES256, attestation_auth_data, b64url,
    sha256,
};
use crate::service::entry;
use crate::two_factor::two_factor_enabled;
use crate::{AuthError, AuthService, ClientMeta, LoginOutcome, Passkey, USER_SUBJECT, User};

/// Random characters in a ceremony challenge (about 285 bits).
const CHALLENGE_LEN: usize = 48;

/// The longest passkey name, in characters.
pub const MAX_PASSKEY_NAME: usize = 100;

/// A passkey as the passkey routes return it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct PasskeyResource {
    /// The id (`DELETE /auth/passkeys/{id}`).
    pub id: Uuid,
    /// The name the user gave it.
    pub name: String,
    /// `ES256` or `EdDSA`.
    pub algorithm: String,
    /// The transports the browser reported (`internal`, `hybrid`, ...).
    pub transports: Vec<String>,
    /// When it was registered.
    pub created_at: DateTime<Utc>,
    /// When it last signed the user in.
    pub last_used_at: Option<DateTime<Utc>>,
}

impl From<&Passkey> for PasskeyResource {
    fn from(passkey: &Passkey) -> Self {
        Self {
            id: passkey.id,
            name: passkey.name.clone(),
            algorithm: match passkey.algorithm {
                ES256 => "ES256",
                EDDSA => "EdDSA",
                _ => "unknown",
            }
            .to_owned(),
            transports: passkey
                .transports
                .as_deref()
                .map(|t| t.split(',').map(str::to_owned).collect())
                .unwrap_or_default(),
            created_at: passkey.created_at,
            last_used_at: passkey.last_used_at,
        }
    }
}

/// A pending ceremony, kept in the cache under its challenge.
#[derive(Serialize, Deserialize)]
struct Ceremony {
    /// `register` or `login`.
    kind: String,
    /// For a registration: the user.
    user_id: Option<Uuid>,
}

/// The `PublicKeyCredential` JSON a browser returns
/// (`credential.toJSON()`), as far as it is read.
#[derive(Deserialize)]
struct CredentialJson {
    id: String,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    response: ResponseJson,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResponseJson {
    #[serde(rename = "clientDataJSON")]
    client_data_json: String,
    #[serde(default)]
    attestation_object: Option<String>,
    #[serde(default)]
    authenticator_data: Option<String>,
    #[serde(default)]
    signature: Option<String>,
    #[serde(default)]
    user_handle: Option<String>,
    #[serde(default)]
    transports: Vec<String>,
}

impl fmt::Debug for CredentialJson {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CredentialJson").finish_non_exhaustive()
    }
}

fn refused(reason: impl Into<String>) -> AuthError {
    AuthError::PasskeyRefused(reason.into())
}

impl AuthService {
    /// The `publicKey` options of a passkey registration for `user` (`POST
    /// /auth/passkeys/register/options`): a single-use challenge valid for
    /// `auth.passkeys.timeout_secs`, the relying party, the user, ES256 and
    /// EdDSA, no attestation, and the user's passkeys as
    /// `excludeCredentials`.
    pub async fn passkey_registration_options(&self, user: &User) -> Result<Value, AuthError> {
        let options = self.passkey_options()?;
        let challenge = self.start_ceremony("register", Some(user.id)).await?;
        let exclude: Vec<Value> = self
            .passkeys(user.id)
            .await?
            .iter()
            .map(|passkey| json!({ "type": "public-key", "id": passkey.credential_id }))
            .collect();
        Ok(json!({
            "challenge": challenge,
            "rp": { "id": options.rp_id, "name": options.rp_name },
            "user": {
                "id": base64_url(user.id.as_bytes()),
                "name": user.email,
                "displayName": user.name,
            },
            "pubKeyCredParams": [
                { "type": "public-key", "alg": EDDSA },
                { "type": "public-key", "alg": ES256 },
            ],
            "timeout": options.timeout_secs.saturating_mul(1000),
            "attestation": "none",
            "authenticatorSelection": {
                "residentKey": "preferred",
                "userVerification": options.user_verification,
            },
            "excludeCredentials": exclude,
        }))
    }

    /// Registers the passkey in `credential` (the browser's
    /// `PublicKeyCredential` JSON from `navigator.credentials.create()`)
    /// for `user` (`POST /auth/passkeys/register`); see the module
    /// documentation for the checks. Refusals are
    /// [`AuthError::PasskeyRefused`]. Audited as `auth.passkey_registered`.
    pub async fn register_passkey(
        &self,
        user: &User,
        name: Option<&str>,
        credential: &Value,
        meta: &ClientMeta,
    ) -> Result<Passkey, AuthError> {
        let result = self.check_registration(user, name, credential).await;
        let row = match result {
            Ok(row) => row,
            Err(error) => {
                if let AuthError::PasskeyRefused(reason) = &error {
                    tracing::warn!(
                        target: "navree::security",
                        user_id = %user.id,
                        %reason,
                        "passkey registration refused"
                    );
                }
                return Err(error);
            }
        };
        let audit = entry(AuditKind::Security, "auth.passkey_registered", meta)
            .actor(user.id)
            .subject(USER_SUBJECT, user.id.to_string());
        let user_id = user.id;
        self.write(Some(user_id), |uow| async move {
            let passkey = {
                let tx = uow.tx().await?;
                Passkey::insert(&*tx, [row.clone()]).await?;
                row
            };
            self.audit_after_commit(
                &uow,
                audit.metadata(json!({ "passkey_id": passkey.id, "name": passkey.name })),
            )
            .await;
            Ok(passkey)
        })
        .await
    }

    /// The row of the passkey in a valid registration `credential`.
    async fn check_registration(
        &self,
        user: &User,
        name: Option<&str>,
        credential: &Value,
    ) -> Result<Passkey, AuthError> {
        let options = self.passkey_options()?;
        let credential: CredentialJson = serde_json::from_value(credential.clone())
            .map_err(|_| refused("malformed credential"))?;
        if credential
            .kind
            .as_deref()
            .is_some_and(|k| k != "public-key")
        {
            return Err(refused("not a public-key credential"));
        }
        let client_json = b64url(&credential.response.client_data_json)
            .ok_or_else(|| refused("malformed client data"))?;
        self.check_client_data(&client_json, "webauthn.create", Some(user.id))
            .await?;
        let attestation = credential
            .response
            .attestation_object
            .as_deref()
            .and_then(b64url)
            .ok_or_else(|| refused("no attestation object"))?;
        let auth_data = attestation_auth_data(&attestation)
            .and_then(|bytes| AuthenticatorData::parse(&bytes))
            .ok_or_else(|| refused("malformed authenticator data"))?;
        self.check_authenticator_data(&auth_data, options)?;
        let Some((credential_id, key)) = auth_data.credential else {
            return Err(refused("no attested credential"));
        };
        let credential_hash = sha256_hex(&credential_id);
        let taken = Passkey::query()
            .where_(Passkey::CREDENTIAL_HASH.eq(credential_hash.as_str()))
            .exists(&self.db)
            .await?;
        if taken {
            return Err(refused("the credential is already registered"));
        }
        let name = name
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(|name| name.chars().take(MAX_PASSKEY_NAME).collect())
            .unwrap_or_else(|| "Passkey".to_owned());
        let now = self.clock.now();
        let transports = credential
            .response
            .transports
            .iter()
            .filter(|t| t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join(",");
        Ok(Passkey {
            id: Uuid::now_v7(),
            user_id: user.id,
            credential_id: base64_url(&credential_id),
            credential_hash,
            public_key: base64_url(&key.bytes),
            algorithm: key.algorithm,
            sign_count: i64::from(auth_data.sign_count),
            transports: (!transports.is_empty()).then_some(transports),
            name,
            created_at: now,
            last_used_at: None,
        })
    }

    /// The passkeys of `user_id`, oldest first.
    pub async fn passkeys(&self, user_id: Uuid) -> Result<Vec<Passkey>, AuthError> {
        Ok(Passkey::query()
            .where_(Passkey::USER_ID.eq(user_id))
            .order_by(Passkey::CREATED_AT)
            .get(&self.db)
            .await?
            .into_models())
    }

    /// Removes passkey `id` of `user_id` (`DELETE /auth/passkeys/{id}`);
    /// [`AuthError::PasskeyNotFound`] when it is not theirs. Audited as
    /// `auth.passkey_deleted`.
    pub async fn delete_passkey(
        &self,
        user_id: Uuid,
        id: Uuid,
        meta: &ClientMeta,
    ) -> Result<(), AuthError> {
        let deleted = self
            .write(Some(user_id), |uow| async move {
                let tx = uow.tx().await?;
                Ok(Passkey::query()
                    .where_key(id)
                    .where_(Passkey::USER_ID.eq(user_id))
                    .delete(&*tx)
                    .await?)
            })
            .await?;
        if deleted == 0 {
            return Err(AuthError::PasskeyNotFound);
        }
        self.audit(
            entry(AuditKind::Security, "auth.passkey_deleted", meta)
                .actor(user_id)
                .subject(USER_SUBJECT, user_id.to_string())
                .metadata(json!({ "passkey_id": id })),
        )
        .await;
        Ok(())
    }

    /// The `publicKey` options of a passkey sign-in (`POST
    /// /auth/passkeys/login/options`): a single-use challenge, the RP id,
    /// and, for a registered `email`, its passkeys as `allowCredentials`
    /// (otherwise an empty list: the browser offers discoverable passkeys).
    pub async fn passkey_login_options(&self, email: Option<&str>) -> Result<Value, AuthError> {
        let options = self.passkey_options()?;
        let challenge = self.start_ceremony("login", None).await?;
        let allow: Vec<Value> = match email {
            Some(email) => match self.find_by_email(email).await? {
                Some(user) => self
                    .passkeys(user.id)
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
                    .collect(),
                None => Vec::new(),
            },
            None => Vec::new(),
        };
        Ok(json!({
            "challenge": challenge,
            "rpId": options.rp_id,
            "timeout": options.timeout_secs.saturating_mul(1000),
            "userVerification": options.user_verification,
            "allowCredentials": allow,
        }))
    }

    /// Signs in with the passkey assertion in `credential` (the browser's
    /// `PublicKeyCredential` JSON from `navigator.credentials.get()`) (`POST
    /// /auth/passkeys/login`); see the module documentation for the checks.
    /// Refusals are [`AuthError::PasskeyRefused`] (audited as
    /// `auth.login_failed` with `method: passkey`). Audited as `auth.login`
    /// with `method: passkey`.
    pub async fn login_with_passkey(
        &self,
        credential: &Value,
        meta: &ClientMeta,
    ) -> Result<LoginOutcome, AuthError> {
        match self.check_assertion(credential).await {
            Ok((user, verified)) => {
                if two_factor_enabled(&user) && !verified {
                    let challenge_token = self.sign_challenge(&user)?;
                    self.audit(
                        entry(AuditKind::Auth, "auth.two_factor_challenge", meta)
                            .actor(user.id)
                            .metadata(json!({ "method": "passkey" })),
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
                        .metadata(json!({ "method": "passkey", "user_verified": verified })),
                )
                .await;
                Ok(LoginOutcome::Authenticated { user, tokens })
            }
            Err(error) => {
                if let AuthError::PasskeyRefused(reason) = &error {
                    tracing::warn!(
                        target: "navree::security",
                        ip = meta.ip.as_deref().unwrap_or("-"),
                        %reason,
                        "passkey sign-in refused"
                    );
                    self.audit(
                        entry(AuditKind::Security, "auth.login_failed", meta)
                            .metadata(json!({ "method": "passkey", "reason": reason })),
                    )
                    .await;
                }
                Err(error)
            }
        }
    }

    // -- internals ----------------------------------------------------------

    fn passkey_options(&self) -> Result<&crate::PasskeyOptions, AuthError> {
        let options = &self.options.passkeys;
        if options.enabled {
            Ok(options)
        } else {
            Err(AuthError::Disabled("passkeys"))
        }
    }

    /// A new challenge for a `kind` ceremony, kept until the timeout.
    async fn start_ceremony(&self, kind: &str, user_id: Option<Uuid>) -> Result<String, AuthError> {
        let challenge = random_token(CHALLENGE_LEN);
        let ttl = std::time::Duration::from_secs(self.options.passkeys.timeout_secs);
        self.cache
            .put(
                &ceremony_key(&challenge),
                &Ceremony {
                    kind: kind.to_owned(),
                    user_id,
                },
                ttl,
            )
            .await?;
        Ok(base64_url(challenge.as_bytes()))
    }

    /// Checks `client_json`: its type, its challenge (a pending ceremony of
    /// the matching kind and user, used up now) and its origin.
    async fn check_client_data(
        &self,
        client_json: &[u8],
        kind: &str,
        user_id: Option<Uuid>,
    ) -> Result<(), AuthError> {
        let client =
            ClientData::parse(client_json).ok_or_else(|| refused("malformed client data"))?;
        if client.kind != kind {
            return Err(refused(format!("client data of type {}", client.kind)));
        }
        if client.cross_origin {
            return Err(refused("cross-origin ceremony"));
        }
        if !self.options.passkeys.origins.contains(&client.origin) {
            return Err(refused(format!("origin {} not allowed", client.origin)));
        }
        let challenge = b64url(&client.challenge)
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .ok_or_else(|| refused("unknown challenge"))?;
        let key = ceremony_key(&challenge);
        let ceremony: Option<Ceremony> = self.cache.get(&key).await?;
        let wanted = if kind == "webauthn.create" {
            "register"
        } else {
            "login"
        };
        match ceremony {
            Some(ceremony) if ceremony.kind == wanted && ceremony.user_id == user_id => {
                if self.cache.forget(&key).await? {
                    Ok(())
                } else {
                    Err(refused("challenge already used"))
                }
            }
            _ => Err(refused("unknown or expired challenge")),
        }
    }

    /// The RP id hash, user presence and (when required) user verification.
    fn check_authenticator_data(
        &self,
        data: &AuthenticatorData,
        options: &crate::PasskeyOptions,
    ) -> Result<(), AuthError> {
        let rp_id = options.rp_id.as_deref().unwrap_or_default();
        if data.rp_id_hash != sha256(rp_id.as_bytes()) {
            return Err(refused("wrong relying party"));
        }
        if !data.user_present() {
            return Err(refused("user not present"));
        }
        if options.user_verification == "required" && !data.user_verified() {
            return Err(refused("user not verified"));
        }
        Ok(())
    }

    /// The user of a valid assertion and whether the authenticator verified
    /// them; records the new signature counter.
    async fn check_assertion(&self, credential: &Value) -> Result<(User, bool), AuthError> {
        let options = self.passkey_options()?;
        let credential: CredentialJson = serde_json::from_value(credential.clone())
            .map_err(|_| refused("malformed credential"))?;
        let raw_id = b64url(&credential.id).ok_or_else(|| refused("malformed credential id"))?;
        let stored = Passkey::query()
            .where_(Passkey::CREDENTIAL_HASH.eq(sha256_hex(&raw_id)))
            .first(&self.db)
            .await?
            .map(Record::into_inner)
            .ok_or_else(|| refused("unknown credential"))?;
        let client_json = b64url(&credential.response.client_data_json)
            .ok_or_else(|| refused("malformed client data"))?;
        self.check_client_data(&client_json, "webauthn.get", None)
            .await?;
        let auth_bytes = credential
            .response
            .authenticator_data
            .as_deref()
            .and_then(b64url)
            .ok_or_else(|| refused("no authenticator data"))?;
        let data = AuthenticatorData::parse(&auth_bytes)
            .ok_or_else(|| refused("malformed authenticator data"))?;
        self.check_authenticator_data(&data, options)?;
        let signature = credential
            .response
            .signature
            .as_deref()
            .and_then(b64url)
            .ok_or_else(|| refused("no signature"))?;
        let key = CredentialKey {
            algorithm: stored.algorithm,
            bytes: b64url(&stored.public_key).ok_or_else(|| refused("unreadable stored key"))?,
        };
        let mut signed = auth_bytes.clone();
        signed.extend_from_slice(&sha256(&client_json));
        if !key.verify(&signed, &signature) {
            return Err(refused("bad signature"));
        }
        if let Some(handle) = credential
            .response
            .user_handle
            .as_deref()
            .filter(|h| !h.is_empty())
            && b64url(handle).as_deref() != Some(stored.user_id.as_bytes().as_slice())
        {
            return Err(refused("user handle mismatch"));
        }
        let counter = i64::from(data.sign_count);
        if (counter != 0 || stored.sign_count != 0) && counter <= stored.sign_count {
            return Err(refused("signature counter did not increase"));
        }
        let now = self.clock.now();
        let (id, previous) = (stored.id, stored.sign_count);
        let claimed = self
            .write(Some(stored.user_id), |uow| async move {
                let tx = uow.tx().await?;
                let claimed = Passkey::query()
                    .where_key(id)
                    .where_(Passkey::SIGN_COUNT.eq(previous))
                    .update(
                        &*tx,
                        [
                            Passkey::SIGN_COUNT.set(counter),
                            Passkey::LAST_USED_AT.set(now),
                        ],
                    )
                    .await?;
                Ok(claimed == 1)
            })
            .await?;
        if !claimed {
            return Err(refused("concurrent sign-in with the same counter"));
        }
        let user = self
            .find_user(stored.user_id)
            .await?
            .ok_or_else(|| refused("the user no longer exists"))?;
        Ok((user, data.user_verified()))
    }
}

fn ceremony_key(challenge: &str) -> String {
    format!("navree-auth:passkey:{}", sha256_hex(challenge.as_bytes()))
}

/// base64url without padding.
pub(crate) fn base64_url(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}
