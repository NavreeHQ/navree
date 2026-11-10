//! Test helpers for apps using the auth module.
//!
//! [`VirtualAuthenticator`] plays the browser and a passkey authenticator:
//! it answers the `publicKey` options of the passkey routes with the
//! `PublicKeyCredential` JSON a browser would post, signed with real ES256
//! or Ed25519 keys (ring), so passkey registration and sign-in run end to
//! end in `TestApp` tests.
//!
//! ```
//! use navree_auth::testing::VirtualAuthenticator;
//! use serde_json::json;
//!
//! let authenticator = VirtualAuthenticator::new("example.com", "https://app.example.com");
//! // `options` is the body of `POST /api/auth/passkeys/register/options`.
//! let options = json!({
//!     "challenge": "Y2hhbGxlbmdl",
//!     "user": { "id": "AZJhJhJhJhJhJhJhJhJhJg", "name": "anu@example.com" },
//! });
//! let credential = authenticator.create(&options);
//! assert_eq!(credential["type"], "public-key");
//! assert_eq!(authenticator.credential_ids().len(), 1);
//! ```

use std::fmt;
use std::sync::{Mutex, PoisonError};

use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, Ed25519KeyPair, KeyPair};
use serde_json::{Value, json};

use crate::passkeys::webauthn::{FLAG_AT, FLAG_UP, FLAG_UV, sha256};
use crate::passkeys::{Cbor, base64_url};

/// A software passkey authenticator for tests (see the module docs).
pub struct VirtualAuthenticator {
    rp_id: String,
    origin: String,
    ed25519: bool,
    user_verification: bool,
    rng: SystemRandom,
    credentials: Mutex<Vec<Credential>>,
}

enum Key {
    Es256(EcdsaKeyPair),
    Ed25519(Ed25519KeyPair),
}

struct Credential {
    id: Vec<u8>,
    key: Key,
    sign_count: u32,
    user_handle: Option<String>,
}

impl VirtualAuthenticator {
    /// An authenticator for relying party `rp_id` (`example.com`), as used
    /// from `origin` (`https://app.example.com`), creating ES256 keys and
    /// verifying the user.
    pub fn new(rp_id: &str, origin: &str) -> Self {
        Self {
            rp_id: rp_id.to_owned(),
            origin: origin.to_owned(),
            ed25519: false,
            user_verification: true,
            rng: SystemRandom::new(),
            credentials: Mutex::new(Vec::new()),
        }
    }

    /// The same authenticator creating Ed25519 (EdDSA) keys.
    pub fn ed25519(mut self) -> Self {
        self.ed25519 = true;
        self
    }

    /// The same authenticator without user verification (presence only).
    pub fn without_user_verification(mut self) -> Self {
        self.user_verification = false;
        self
    }

    /// The `PublicKeyCredential` JSON of a new passkey for the creation
    /// `options` (the `challenge` and `user.id` are read), as
    /// `navigator.credentials.create()` and `toJSON()` would give it.
    ///
    /// # Panics
    ///
    /// When the system random generator fails.
    pub fn create(&self, options: &Value) -> Value {
        let mut id = vec![0_u8; 32];
        self.rng.fill(&mut id).expect("random credential id");
        let (key, cose) = if self.ed25519 {
            let pkcs8 = Ed25519KeyPair::generate_pkcs8(&self.rng).expect("an Ed25519 key");
            let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).expect("an Ed25519 key");
            let cose = Cbor::Map(vec![
                (Cbor::Int(1), Cbor::Int(1)),
                (Cbor::Int(3), Cbor::Int(-8)),
                (Cbor::Int(-1), Cbor::Int(6)),
                (
                    Cbor::Int(-2),
                    Cbor::Bytes(pair.public_key().as_ref().to_vec()),
                ),
            ]);
            (Key::Ed25519(pair), cose)
        } else {
            let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &self.rng)
                .expect("a P-256 key");
            let pair = EcdsaKeyPair::from_pkcs8(
                &ECDSA_P256_SHA256_ASN1_SIGNING,
                pkcs8.as_ref(),
                &self.rng,
            )
            .expect("a P-256 key");
            let point = pair.public_key().as_ref();
            let cose = Cbor::Map(vec![
                (Cbor::Int(1), Cbor::Int(2)),
                (Cbor::Int(3), Cbor::Int(-7)),
                (Cbor::Int(-1), Cbor::Int(1)),
                (Cbor::Int(-2), Cbor::Bytes(point[1..33].to_vec())),
                (Cbor::Int(-3), Cbor::Bytes(point[33..].to_vec())),
            ]);
            (Key::Es256(pair), cose)
        };
        let mut auth_data = self.auth_data_head(FLAG_AT, 0);
        auth_data.extend_from_slice(&[0; 16]);
        auth_data.extend_from_slice(&u16::try_from(id.len()).unwrap_or(0).to_be_bytes());
        auth_data.extend_from_slice(&id);
        auth_data.extend_from_slice(&cose.encode());
        let attestation = Cbor::Map(vec![
            (Cbor::Text("fmt".into()), Cbor::Text("none".into())),
            (Cbor::Text("attStmt".into()), Cbor::Map(Vec::new())),
            (Cbor::Text("authData".into()), Cbor::Bytes(auth_data)),
        ])
        .encode();
        let client = self.client_data("webauthn.create", options);
        let encoded_id = base64_url(&id);
        self.lock().push(Credential {
            id,
            key,
            sign_count: 0,
            user_handle: options["user"]["id"].as_str().map(str::to_owned),
        });
        json!({
            "id": encoded_id,
            "rawId": encoded_id,
            "type": "public-key",
            "authenticatorAttachment": "platform",
            "response": {
                "clientDataJSON": base64_url(client.as_bytes()),
                "attestationObject": base64_url(&attestation),
                "transports": ["internal", "hybrid"],
            },
            "clientExtensionResults": {},
        })
    }

    /// The `PublicKeyCredential` JSON of an assertion for the request
    /// `options` (the `challenge` and `allowCredentials` are read), signed
    /// by the first of this authenticator's passkeys that `allowCredentials`
    /// allows (any, when it is empty), as `navigator.credentials.get()`
    /// would give it. `None` without a matching passkey.
    pub fn get(&self, options: &Value) -> Option<Value> {
        let allowed: Vec<String> = options["allowCredentials"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|c| c["id"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let mut credentials = self.lock();
        let credential = credentials
            .iter_mut()
            .find(|c| allowed.is_empty() || allowed.contains(&base64_url(&c.id)))?;
        credential.sign_count += 1;
        let auth_data = self.auth_data_head(0, credential.sign_count);
        let client = self.client_data("webauthn.get", options);
        let mut signed = auth_data.clone();
        signed.extend_from_slice(&sha256(client.as_bytes()));
        let signature = match &credential.key {
            Key::Es256(pair) => pair
                .sign(&self.rng, &signed)
                .expect("an ES256 signature")
                .as_ref()
                .to_vec(),
            Key::Ed25519(pair) => pair.sign(&signed).as_ref().to_vec(),
        };
        let id = base64_url(&credential.id);
        Some(json!({
            "id": id,
            "rawId": id,
            "type": "public-key",
            "response": {
                "clientDataJSON": base64_url(client.as_bytes()),
                "authenticatorData": base64_url(&auth_data),
                "signature": base64_url(&signature),
                "userHandle": credential.user_handle,
            },
            "clientExtensionResults": {},
        }))
    }

    /// The ids (base64url) of the passkeys created so far.
    pub fn credential_ids(&self) -> Vec<String> {
        self.lock().iter().map(|c| base64_url(&c.id)).collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Credential>> {
        self.credentials
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn auth_data_head(&self, extra_flags: u8, sign_count: u32) -> Vec<u8> {
        let mut flags = FLAG_UP | extra_flags;
        if self.user_verification {
            flags |= FLAG_UV;
        }
        let mut data = sha256(self.rp_id.as_bytes()).to_vec();
        data.push(flags);
        data.extend_from_slice(&sign_count.to_be_bytes());
        data
    }

    fn client_data(&self, kind: &str, options: &Value) -> String {
        json!({
            "type": kind,
            "challenge": options["challenge"].as_str().unwrap_or_default(),
            "origin": self.origin,
            "crossOrigin": false,
        })
        .to_string()
    }
}

impl fmt::Debug for VirtualAuthenticator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VirtualAuthenticator")
            .field("rp_id", &self.rp_id)
            .field("origin", &self.origin)
            .field("credentials", &self.lock().len())
            .finish_non_exhaustive()
    }
}
