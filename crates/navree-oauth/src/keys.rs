//! [`SigningKeys`]: the keys that sign ID tokens, Ed25519 (EdDSA) or RSA
//! (RS256), and the JWKS that publishes them, on [`navree_crypto::jose`]
//! (ring underneath: Navree reads RSA keys but does not make them).

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use navree_crypto::jose::{Algorithm, Jws, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The prefix of an encoded Ed25519 signing key (`ed25519:<base64>`).
pub const SIGNING_KEY_PREFIX: &str = "ed25519:";

/// The prefix of an encoded RSA signing key (`rsa:<base64 DER>`).
pub const RSA_KEY_PREFIX: &str = "rsa:";

/// An error reading a signing key or verifying a token.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    /// The key is not `ed25519:<base64 PKCS#8 or 32-byte seed>`.
    #[error("a signing key must be `ed25519:<base64>` (a PKCS#8 Ed25519 key or a 32-byte seed)")]
    Malformed,
    /// The key is not an RSA private key of 2048 to 4096 bits.
    #[error(
        "an RSA signing key must be `rsa:<base64 DER>` or a PEM private key (PKCS#8 or PKCS#1), of 2048 to 4096 bits"
    )]
    MalformedRsa,
    /// The key is neither form.
    #[error(
        "a signing key must be `ed25519:<base64>`, `rsa:<base64 DER>` or a PEM RSA private key"
    )]
    UnknownKind,
    /// The token is not a compact JWS signed by one of the keys.
    #[error("the token is not signed by a known key")]
    InvalidSignature,
}

/// How ID tokens are signed (`oauth.id_token_signing_alg`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IdTokenAlgorithm {
    /// Ed25519 signatures (RFC 8037): small and fast, with a key Navree
    /// makes (`navree oauth:keys`) or derives from `app.key`.
    EdDSA,
    /// RSA PKCS#1 v1.5 signatures with SHA-256: what OpenID Connect
    /// Discovery requires every provider to offer and every client library
    /// accepts. The RSA key is made outside Navree.
    RS256,
}

impl IdTokenAlgorithm {
    /// `EdDSA` or `RS256`, as JWS headers and discovery name it.
    pub fn as_str(self) -> &'static str {
        match self {
            IdTokenAlgorithm::EdDSA => "EdDSA",
            IdTokenAlgorithm::RS256 => "RS256",
        }
    }
}

impl fmt::Display for IdTokenAlgorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A key that signs ID tokens: Ed25519 (EdDSA) or RSA (RS256), with its
/// RFC 7638 thumbprint as key id.
#[derive(Clone)]
pub struct IdTokenKey {
    key: SigningKey,
}

impl IdTokenKey {
    fn new(key: SigningKey) -> Self {
        Self {
            key: key.with_thumbprint_kid(),
        }
    }

    /// Reads a key in any accepted form: `ed25519:<base64>` (what `navree
    /// oauth:keys` prints), `rsa:<base64 DER>` (a PKCS#8 or PKCS#1 RSA
    /// private key, the form for an environment variable), or a PEM RSA
    /// private key (`-----BEGIN PRIVATE KEY-----` or `-----BEGIN RSA
    /// PRIVATE KEY-----`). RSA keys must have 2048 to 4096 bits.
    pub fn parse(encoded: &str) -> Result<Self, KeyError> {
        let encoded = encoded.trim();
        if encoded.starts_with(SIGNING_KEY_PREFIX) {
            return SigningKey25519::parse(encoded).map(Self::from);
        }
        let rsa = if let Some(body) = encoded.strip_prefix(RSA_KEY_PREFIX) {
            // Base64 DER without armor: navree_crypto reads PKCS#8, then
            // PKCS#1.
            SigningKey::from_pem(body.trim())
        } else if encoded.starts_with("-----BEGIN") || encoded.contains("PRIVATE KEY-----") {
            SigningKey::from_pem(encoded)
        } else {
            return Err(KeyError::UnknownKind);
        };
        rsa.ok()
            .filter(|key| key.algorithm() == Algorithm::RS256)
            .map(Self::new)
            .ok_or(KeyError::MalformedRsa)
    }

    /// How the key signs.
    pub fn algorithm(&self) -> IdTokenAlgorithm {
        match self.key.algorithm() {
            Algorithm::RS256 => IdTokenAlgorithm::RS256,
            _ => IdTokenAlgorithm::EdDSA,
        }
    }

    /// The key id.
    pub fn kid(&self) -> &str {
        self.key.kid().unwrap_or_default()
    }

    /// The public key.
    pub fn verifying_key(&self) -> VerifyingKey {
        self.key.verifying_key()
    }

    /// The public key as a JWK, with `kid`, `alg` and `use: sig` (`kty:
    /// OKP` for Ed25519, `kty: RSA` with `n` and `e` for RSA).
    pub fn public_jwk(&self) -> Value {
        serde_json::to_value(self.key.public_jwk()).unwrap_or(Value::Null)
    }
}

impl fmt::Debug for IdTokenKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IdTokenKey")
            .field("alg", &self.algorithm())
            .field("kid", &self.kid())
            .finish_non_exhaustive()
    }
}

impl From<SigningKey25519> for IdTokenKey {
    fn from(key: SigningKey25519) -> Self {
        Self { key: key.key }
    }
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
    current: IdTokenKey,
    previous: Vec<IdTokenKey>,
}

impl SigningKeys {
    /// Ed25519 keys signing with `current`, publishing `previous` too.
    pub fn new(current: SigningKey25519, previous: Vec<SigningKey25519>) -> Self {
        Self::with_keys(
            current.into(),
            previous.into_iter().map(IdTokenKey::from).collect(),
        )
    }

    /// Keys signing with `current` (Ed25519 or RSA), publishing `previous`
    /// too, of either kind: switching from EdDSA to RS256 keeps the old
    /// Ed25519 key in `previous`, so tokens signed before still verify.
    pub fn with_keys(current: IdTokenKey, previous: Vec<IdTokenKey>) -> Self {
        Self { current, previous }
    }

    /// The key that signs.
    pub fn current(&self) -> &IdTokenKey {
        &self.current
    }

    /// The algorithm new ID tokens are signed with.
    pub fn algorithm(&self) -> IdTokenAlgorithm {
        self.current.algorithm()
    }

    /// The JWKS: `{"keys": [current, previous...]}`.
    pub fn jwks(&self) -> Value {
        let keys: Vec<Value> = std::iter::once(&self.current)
            .chain(&self.previous)
            .map(IdTokenKey::public_jwk)
            .collect();
        serde_json::json!({ "keys": keys })
    }

    /// `claims` as a compact JWS signed with the current key (`alg:
    /// EdDSA` or `RS256`, `typ: JWT`, `kid`).
    pub fn sign<T: Serialize>(&self, claims: &T) -> String {
        let claims = serde_json::to_value(claims).unwrap_or(Value::Null);
        self.current
            .key
            .sign_jwt(&claims)
            .expect("Ed25519 and RSA signing of JSON does not fail")
    }

    /// The claims of `token` when one of the keys signed it (its `kid`
    /// picks the key, and its `alg` must be that key's). Expiry and
    /// audience are the caller's to check.
    pub fn verify(&self, token: &str) -> Result<Value, KeyError> {
        let jws = Jws::parse(token).map_err(|_| KeyError::InvalidSignature)?;
        let kid = jws.header().kid.as_deref();
        let key = std::iter::once(&self.current)
            .chain(&self.previous)
            .find(|key| kid == Some(key.kid()))
            .ok_or(KeyError::InvalidSignature)?;
        if jws.header().alg != key.key.algorithm() {
            return Err(KeyError::InvalidSignature);
        }
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
        let token = before.sign(&json!({"sub": "anu"}));
        let after = SigningKeys::new(new.clone(), vec![old.clone()]);
        assert_eq!(after.verify(&token).unwrap()["sub"], "anu");
        assert_eq!(after.jwks()["keys"].as_array().unwrap().len(), 2);
        assert_eq!(after.jwks()["keys"][0]["kid"], new.kid());
        let fresh = after.sign(&json!({"sub": "gauri"}));
        assert!(after.verify(&fresh).is_ok());
        // Unknown key, tampered payload, garbage.
        let other = SigningKeys::new(SigningKey25519::generate(), Vec::new());
        assert!(other.verify(&token).is_err());
        let mut parts: Vec<&str> = fresh.split('.').collect();
        let forged = URL_SAFE_NO_PAD.encode(r#"{"sub":"manjeet"}"#);
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
        let token = hmac.sign_jwt(&json!({"sub": "manjeet"})).unwrap();
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

    /// A 2048-bit RSA test key (PKCS#8 PEM).
    const RSA_PEM: &str = include_str!("../tests/fixtures/rsa-2048.pem");

    /// The key as `rsa:<base64 DER>`, the form for an environment variable.
    fn rsa_env() -> String {
        let body: String = RSA_PEM
            .lines()
            .filter(|line| !line.starts_with("-----"))
            .collect();
        format!("{RSA_KEY_PREFIX}{body}")
    }

    #[test]
    fn rsa_keys_are_read_from_pem_or_base64_der() {
        let from_pem = IdTokenKey::parse(RSA_PEM).unwrap();
        let from_env = IdTokenKey::parse(&rsa_env()).unwrap();
        assert_eq!(from_pem.algorithm(), IdTokenAlgorithm::RS256);
        assert_eq!(from_pem.kid(), from_env.kid());
        let jwk = from_pem.public_jwk();
        assert_eq!(jwk["kty"], "RSA");
        assert_eq!(jwk["alg"], "RS256");
        assert_eq!(jwk["use"], "sig");
        assert_eq!(jwk["kid"], from_pem.kid());
        assert_eq!(jwk["e"], "AQAB");
        assert_eq!(
            URL_SAFE_NO_PAD
                .decode(jwk["n"].as_str().unwrap())
                .unwrap()
                .len(),
            256
        );
        assert!(
            jwk.get("d").is_none(),
            "the private part is never published"
        );
        // A literal `\n` (a key pasted into one variable) is accepted.
        assert!(IdTokenKey::parse(&RSA_PEM.replace('\n', "\\n")).is_ok());
        // Ed25519 keys still read the same way.
        let ed = SigningKey25519::generate();
        let parsed = IdTokenKey::parse(&ed.encode()).unwrap();
        assert_eq!(parsed.algorithm(), IdTokenAlgorithm::EdDSA);
        assert_eq!(parsed.kid(), ed.kid());
    }

    #[test]
    fn weak_or_wrong_rsa_keys_are_refused() {
        let weak = include_str!("../tests/fixtures/rsa-1024.pem");
        assert_eq!(IdTokenKey::parse(weak).unwrap_err(), KeyError::MalformedRsa);
        assert_eq!(
            IdTokenKey::parse("rsa:AAAA").unwrap_err(),
            KeyError::MalformedRsa
        );
        assert_eq!(
            IdTokenKey::parse("rsa:").unwrap_err(),
            KeyError::MalformedRsa
        );
        // A P-256 key is not an RSA key.
        let ec = SigningKey::generate_es256().to_pkcs8_der().unwrap();
        assert_eq!(
            IdTokenKey::parse(&format!("rsa:{}", STANDARD.encode(ec))).unwrap_err(),
            KeyError::MalformedRsa
        );
        assert_eq!(
            IdTokenKey::parse("hs256:x").unwrap_err(),
            KeyError::UnknownKind
        );
        assert_eq!(IdTokenKey::parse("").unwrap_err(), KeyError::UnknownKind);
    }

    #[test]
    fn rs256_tokens_verify_with_ring_from_the_published_jwk() {
        let keys = SigningKeys::with_keys(IdTokenKey::parse(RSA_PEM).unwrap(), Vec::new());
        assert_eq!(keys.algorithm(), IdTokenAlgorithm::RS256);
        let token = keys.sign(&json!({"sub": "harpreet", "aud": "crm"}));
        let parts: Vec<&str> = token.split('.').collect();
        let header: Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
        assert_eq!(
            header,
            json!({"alg": "RS256", "kid": keys.current().kid(), "typ": "JWT"})
        );
        // Checked with ring's RSA verifier and the JWK's n and e, apart
        // from navree_crypto's verification.
        let jwk = &keys.jwks()["keys"][0];
        let n = URL_SAFE_NO_PAD.decode(jwk["n"].as_str().unwrap()).unwrap();
        let e = URL_SAFE_NO_PAD.decode(jwk["e"].as_str().unwrap()).unwrap();
        ring::signature::RsaPublicKeyComponents { n: &n, e: &e }
            .verify(
                &ring::signature::RSA_PKCS1_2048_8192_SHA256,
                format!("{}.{}", parts[0], parts[1]).as_bytes(),
                &URL_SAFE_NO_PAD.decode(parts[2]).unwrap(),
            )
            .expect("the RS256 signature verifies");
        assert_eq!(keys.verify(&token).unwrap()["sub"], "harpreet");
    }

    #[test]
    fn switching_to_rs256_keeps_eddsa_tokens_verifiable() {
        let old = SigningKey25519::generate();
        let before = SigningKeys::new(old.clone(), Vec::new());
        let old_token = before.sign(&json!({"sub": "anu"}));
        let after = SigningKeys::with_keys(
            IdTokenKey::parse(RSA_PEM).unwrap(),
            vec![IdTokenKey::from(old)],
        );
        assert_eq!(after.verify(&old_token).unwrap()["sub"], "anu");
        let jwks = after.jwks();
        assert_eq!(jwks["keys"][0]["kty"], "RSA");
        assert_eq!(jwks["keys"][1]["kty"], "OKP");
        let new_token = after.sign(&json!({"sub": "gauri"}));
        assert_eq!(after.verify(&new_token).unwrap()["sub"], "gauri");
        // A token whose header names the RSA key but another algorithm is
        // refused.
        let mut parts: Vec<String> = new_token.split('.').map(str::to_owned).collect();
        parts[0] = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(
                &json!({"alg": "PS256", "kid": after.current().kid(), "typ": "JWT"}),
            )
            .unwrap(),
        );
        assert_eq!(
            after.verify(&parts.join(".")),
            Err(KeyError::InvalidSignature)
        );
    }
}
