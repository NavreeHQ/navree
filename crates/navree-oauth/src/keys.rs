//! [`SigningKeys`]: the Ed25519 keys that sign ID tokens (EdDSA JWS) and
//! the JWKS that publishes them.

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use ed25519_dalek::ed25519::signature::Signer as _;
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

/// The prefix of an encoded signing key (`ed25519:<base64>`).
pub const SIGNING_KEY_PREFIX: &str = "ed25519:";

/// The DER prefix of an Ed25519 private key in PKCS#8 v1 (RFC 8410 §7),
/// followed by the 32-byte seed.
const PKCS8_PREFIX: [u8; 16] = [
    0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20,
];

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
    key: SigningKey,
    kid: String,
}

impl SigningKey25519 {
    /// The key whose 32-byte seed is `seed`.
    pub fn from_seed(seed: [u8; 32]) -> Self {
        let key = SigningKey::from_bytes(&seed);
        let kid = thumbprint(&key.verifying_key());
        Self { key, kid }
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
        let seed: [u8; 32] = match bytes.len() {
            32 => bytes.try_into().map_err(|_| KeyError::Malformed)?,
            48 if bytes[..16] == PKCS8_PREFIX => {
                bytes[16..].try_into().map_err(|_| KeyError::Malformed)?
            }
            _ => return Err(KeyError::Malformed),
        };
        Ok(Self::from_seed(seed))
    }

    /// A new random key.
    pub fn generate() -> Self {
        let mut seed = [0_u8; 32];
        rand::fill(&mut seed);
        Self::from_seed(seed)
    }

    /// The key encoded as `ed25519:<base64 PKCS#8>`, for
    /// `APP_OAUTH__SIGNING_KEY`.
    pub fn encode(&self) -> String {
        let mut der = PKCS8_PREFIX.to_vec();
        der.extend_from_slice(&self.key.to_bytes());
        format!("{SIGNING_KEY_PREFIX}{}", STANDARD.encode(der))
    }

    /// The key id.
    pub fn kid(&self) -> &str {
        &self.kid
    }

    /// The public key.
    pub fn verifying_key(&self) -> VerifyingKey {
        self.key.verifying_key()
    }

    /// The public key as a JWK (`kty: OKP`, `crv: Ed25519`).
    pub fn public_jwk(&self) -> Value {
        json!({
            "kty": "OKP",
            "crv": "Ed25519",
            "x": URL_SAFE_NO_PAD.encode(self.verifying_key().to_bytes()),
            "kid": self.kid,
            "alg": "EdDSA",
            "use": "sig",
        })
    }
}

impl fmt::Debug for SigningKey25519 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SigningKey25519")
            .field("kid", &self.kid)
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
        json!({ "keys": keys })
    }

    /// `claims` as a compact JWS signed with the current key (`alg:
    /// EdDSA`, `typ: JWT`, `kid`).
    pub fn sign<T: Serialize>(&self, claims: &T) -> String {
        let header = json!({ "alg": "EdDSA", "typ": "JWT", "kid": self.current.kid });
        let encode = |value: &Value| URL_SAFE_NO_PAD.encode(value.to_string());
        let claims = serde_json::to_value(claims).unwrap_or(Value::Null);
        let input = format!("{}.{}", encode(&header), encode(&claims));
        let signature: Signature = self.current.key.sign(input.as_bytes());
        format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()))
    }

    /// The claims of `token` when one of the keys signed it (its `kid`
    /// picks the key). Expiry and audience are the caller's to check.
    pub fn verify(&self, token: &str) -> Result<Value, KeyError> {
        let mut parts = token.split('.');
        let (Some(header), Some(payload), Some(signature), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(KeyError::InvalidSignature);
        };
        let decode = |part: &str| {
            URL_SAFE_NO_PAD
                .decode(part)
                .map_err(|_| KeyError::InvalidSignature)
        };
        let header: Value =
            serde_json::from_slice(&decode(header)?).map_err(|_| KeyError::InvalidSignature)?;
        if header["alg"] != "EdDSA" {
            return Err(KeyError::InvalidSignature);
        }
        let key = std::iter::once(&self.current)
            .chain(&self.previous)
            .find(|key| header["kid"].as_str() == Some(key.kid()))
            .ok_or(KeyError::InvalidSignature)?;
        let signature =
            Signature::from_slice(&decode(signature)?).map_err(|_| KeyError::InvalidSignature)?;
        let input = &token[..token.rfind('.').unwrap_or(0)];
        key.verifying_key()
            .verify_strict(input.as_bytes(), &signature)
            .map_err(|_| KeyError::InvalidSignature)?;
        serde_json::from_slice(&decode(payload)?).map_err(|_| KeyError::InvalidSignature)
    }
}

/// The RFC 7638 thumbprint of an Ed25519 public key's JWK.
fn thumbprint(key: &VerifyingKey) -> String {
    let canonical = format!(
        r#"{{"crv":"Ed25519","kty":"OKP","x":"{}"}}"#,
        URL_SAFE_NO_PAD.encode(key.to_bytes())
    );
    URL_SAFE_NO_PAD.encode(Sha256::digest(canonical.as_bytes()))
}

#[cfg(test)]
mod tests {
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
    }

    #[test]
    fn rfc_8037_signature_matches() {
        // RFC 8037 Appendix A.4: signing "Example of Ed25519 signing".
        let seed = URL_SAFE_NO_PAD
            .decode("nWGxne_9WmC6hEr0kuwsxERJxWl7MmkZcDusAxyuf2A")
            .unwrap();
        let key = SigningKey25519::from_seed(seed.try_into().unwrap());
        let input = "eyJhbGciOiJFZERTQSJ9.RXhhbXBsZSBvZiBFZDI1NTE5IHNpZ25pbmc";
        let signature: Signature = key.key.sign(input.as_bytes());
        assert_eq!(
            URL_SAFE_NO_PAD.encode(signature.to_bytes()),
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
        let seed = format!("ed25519:{}", STANDARD.encode(key.key.to_bytes()));
        assert_eq!(SigningKey25519::parse(&seed).unwrap().kid(), key.kid());
        for bad in ["", "ed25519:", "ed25519:!!!", "rsa:AAAA", "ed25519:AAAA"] {
            assert_eq!(
                SigningKey25519::parse(bad).unwrap_err(),
                KeyError::Malformed,
                "{bad}"
            );
        }
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
}
