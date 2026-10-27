//! [`SigningKeys`]: the Ed25519 keys that sign ID tokens (EdDSA JWS) and
//! the JWKS that publishes them, on [`navree_crypto::jose`].

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use navree_crypto::jose::{Algorithm, Jws, SigningKey, VerifyingKey};
use serde::Serialize;
use serde_json::Value;

/// The prefix of an encoded signing key (`ed25519:<base64>`).
pub const SIGNING_KEY_PREFIX: &str = "ed25519:";

/// An error reading a signing key or verifying a token.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    /// The key is not `ed25519:<base64 PKCS#8 or 32-byte seed>`.
    #[error("a signing key must be `ed25519:<base64>` (a PKCS#8 Ed25519 key or a 32-byte seed)")]
    Malformed,
    /// The token is not a compact JWS signed by one of the keys.
    #[error("the token is not signed by a known key")]
    InvalidSignature,
}

/// One Ed25519 key and its key id (the RFC 7638 thumbprint of its public
/// JWK).
#[derive(Clone)]
pub struct SigningKey25519 {
    /// An Ed25519 key whose kid is its thumbprint.
    key: SigningKey,
}

impl SigningKey25519 {
    fn new(key: SigningKey) -> Self {
        Self {
            key: key.with_thumbprint_kid(),
        }
    }

    /// The key whose 32-byte seed is `seed`.
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self::new(SigningKey::ed25519_from_seed(&seed))
    }

    /// Parses `ed25519:<base64>`: a PKCS#8 Ed25519 private key (what
    /// `navree oauth:keys` prints) or a raw 32-byte seed.
    pub fn parse(encoded: &str) -> Result<Self, KeyError> {
        let body = encoded
            .trim()
            .strip_prefix(SIGNING_KEY_PREFIX)
            .ok_or(KeyError::Malformed)?;
        let bytes = STANDARD
            .decode(body)
            .or_else(|_| URL_SAFE_NO_PAD.decode(body.trim_end_matches('=')))
            .map_err(|_| KeyError::Malformed)?;
        let key = match <[u8; 32]>::try_from(bytes.as_slice()) {
            Ok(seed) => SigningKey::ed25519_from_seed(&seed),
            Err(_) => SigningKey::from_pkcs8_der(&bytes)
                .ok()
                .filter(|key| key.algorithm() == Algorithm::EdDSA)
                .ok_or(KeyError::Malformed)?,
        };
        Ok(Self::new(key))
    }

    /// A new random key.
    pub fn generate() -> Self {
        Self::new(SigningKey::generate_ed25519())
    }

    /// The key encoded as `ed25519:<base64 PKCS#8>`, for
    /// `APP_OAUTH__SIGNING_KEY`.
    pub fn encode(&self) -> String {
        let der = self
            .key
            .to_pkcs8_der()
            .expect("an Ed25519 key exports as PKCS#8");
        format!("{SIGNING_KEY_PREFIX}{}", STANDARD.encode(der))
    }

    /// The key id.
    pub fn kid(&self) -> &str {
        self.key.kid().unwrap_or_default()
    }

    /// The public key.
    pub fn verifying_key(&self) -> VerifyingKey {
        self.key.verifying_key()
    }

    /// The public key as a JWK (`kty: OKP`, `crv: Ed25519`).
    pub fn public_jwk(&self) -> Value {
        serde_json::to_value(self.key.public_jwk()).unwrap_or(Value::Null)
    }
}

impl fmt::Debug for SigningKey25519 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SigningKey25519")
            .field("kid", &self.kid())
            .finish_non_exhaustive()
    }
}

/// The current signing key and the older ones still published.
#[derive(Clone, Debug)]
pub struct SigningKeys {
    current: SigningKey25519,
    previous: Vec<SigningKey25519>,
}

impl SigningKeys {
    /// Keys signing with `current`, publishing `previous` too.
    pub fn new(current: SigningKey25519, previous: Vec<SigningKey25519>) -> Self {
        Self { current, previous }
    }

    /// The key that signs.
    pub fn current(&self) -> &SigningKey25519 {
        &self.current
    }

    /// The JWKS: `{"keys": [current, previous...]}`.
    pub fn jwks(&self) -> Value {
        let keys: Vec<Value> = std::iter::once(&self.current)
            .chain(&self.previous)
            .map(SigningKey25519::public_jwk)
            .collect();
        serde_json::json!({ "keys": keys })
    }

    /// `claims` as a compact JWS signed with the current key (`alg:
    /// EdDSA`, `typ: JWT`, `kid`).
    pub fn sign<T: Serialize>(&self, claims: &T) -> String {
        let claims = serde_json::to_value(claims).unwrap_or(Value::Null);
        self.current
            .key
            .sign_jwt(&claims)
            .expect("Ed25519 signing of JSON does not fail")
    }

    /// The claims of `token` when one of the keys signed it (its `kid`
    /// picks the key). Expiry and audience are the caller's to check.
    pub fn verify(&self, token: &str) -> Result<Value, KeyError> {
        let jws = Jws::parse(token).map_err(|_| KeyError::InvalidSignature)?;
        if jws.header().alg != Algorithm::EdDSA {
            return Err(KeyError::InvalidSignature);
        }
        let kid = jws.header().kid.as_deref();
        let key = std::iter::once(&self.current)
            .chain(&self.previous)
            .find(|key| kid == Some(key.kid()))
            .ok_or(KeyError::InvalidSignature)?;
        jws.verify(&key.verifying_key())
            .map_err(|_| KeyError::InvalidSignature)?;
        jws.claims().map_err(|_| KeyError::InvalidSignature)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn rfc_8037_key_has_the_published_thumbprint() {
        // RFC 8037 Appendix A.1 and A.3.
        let seed = URL_SAFE_NO_PAD
            .decode("nWGxne_9WmC6hEr0kuwsxERJxWl7MmkZcDusAxyuf2A")
            .unwrap();
        let key = SigningKey25519::from_seed(seed.try_into().unwrap());
        assert_eq!(
            key.public_jwk()["x"],
            "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo"
        );
        assert_eq!(key.kid(), "kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k");
        assert_eq!(key.public_jwk()["alg"], "EdDSA");
        assert_eq!(key.public_jwk()["use"], "sig");
        assert_eq!(key.public_jwk()["kid"], key.kid());
    }

    #[test]
    fn rfc_8037_signature_matches() {
        // RFC 8037 Appendix A.4: signing "Example of Ed25519 signing".
        let seed = URL_SAFE_NO_PAD
            .decode("nWGxne_9WmC6hEr0kuwsxERJxWl7MmkZcDusAxyuf2A")
            .unwrap();
        let key = SigningKey25519::from_seed(seed.try_into().unwrap());
        let input = "eyJhbGciOiJFZERTQSJ9.RXhhbXBsZSBvZiBFZDI1NTE5IHNpZ25pbmc";
        let signature = key.key.sign(input.as_bytes()).unwrap();
        assert_eq!(
            URL_SAFE_NO_PAD.encode(signature),
            "hgyY0il_MGCjP0JzlnLWG1PPOt7-09PGcvMg3AIbQR6dWbhijcNR4ki4iylGjg5BhVsPt9g7sVvpAr_MuM0KAg"
        );
    }

    #[test]
    fn keys_round_trip_through_their_encoding() {
        let key = SigningKey25519::generate();
        let encoded = key.encode();
        assert!(
            encoded.starts_with("ed25519:MC4CAQAwBQYDK2VwBCIEI"),
            "{encoded}"
        );
        let parsed = SigningKey25519::parse(&encoded).unwrap();
        assert_eq!(parsed.kid(), key.kid());
        let der = key.key.to_pkcs8_der().unwrap();
        let seed = format!("ed25519:{}", STANDARD.encode(&der[16..]));
        assert_eq!(SigningKey25519::parse(&seed).unwrap().kid(), key.kid());
        for bad in ["", "ed25519:", "ed25519:!!!", "rsa:AAAA", "ed25519:AAAA"] {
            assert_eq!(
                SigningKey25519::parse(bad).unwrap_err(),
                KeyError::Malformed,
                "{bad}"
            );
        }
        // A PKCS#8 key of another type is not an Ed25519 key.
        let es256 = SigningKey::generate_es256().to_pkcs8_der().unwrap();
        assert_eq!(
            SigningKey25519::parse(&format!("ed25519:{}", STANDARD.encode(es256))).unwrap_err(),
            KeyError::Malformed
        );
    }

    #[test]
    fn tokens_verify_with_current_and_previous_keys() {
        let (old, new) = (SigningKey25519::generate(), SigningKey25519::generate());
        let before = SigningKeys::new(old.clone(), Vec::new());
        let token = before.sign(&json!({"sub": "ada"}));
        let after = SigningKeys::new(new.clone(), vec![old.clone()]);
        assert_eq!(after.verify(&token).unwrap()["sub"], "ada");
        assert_eq!(after.jwks()["keys"].as_array().unwrap().len(), 2);
        assert_eq!(after.jwks()["keys"][0]["kid"], new.kid());
        let fresh = after.sign(&json!({"sub": "grace"}));
        assert!(after.verify(&fresh).is_ok());
        // Unknown key, tampered payload, garbage.
        let other = SigningKeys::new(SigningKey25519::generate(), Vec::new());
        assert!(other.verify(&token).is_err());
        let mut parts: Vec<&str> = fresh.split('.').collect();
        let forged = URL_SAFE_NO_PAD.encode(r#"{"sub":"mallory"}"#);
        parts[1] = &forged;
        assert!(after.verify(&parts.join(".")).is_err());
        assert!(after.verify("a.b").is_err());
    }

    #[test]
    fn tokens_of_other_algorithms_are_refused() {
        // A token naming the current key's kid but signed with HMAC.
        let keys = SigningKeys::new(SigningKey25519::generate(), Vec::new());
        let hmac = SigningKey::hmac(Algorithm::HS256, [1_u8; 32])
            .unwrap()
            .with_kid(keys.current().kid());
        let token = hmac.sign_jwt(&json!({"sub": "mallory"})).unwrap();
        assert_eq!(keys.verify(&token), Err(KeyError::InvalidSignature));
        // The header is `{"alg":"EdDSA","kid":..,"typ":"JWT"}`.
        let header: Value = serde_json::from_slice(
            &URL_SAFE_NO_PAD
                .decode(keys.sign(&json!({})).split('.').next().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            header,
            json!({"alg": "EdDSA", "kid": keys.current().kid(), "typ": "JWT"})
        );
    }
}
