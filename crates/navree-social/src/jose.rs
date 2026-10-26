//! JSON Web Signatures on `ring`: ID token verification against a JSON Web
//! Key Set (RS256/384/512, PS256/384/512, ES256, ES384, EdDSA) and ES256
//! signing for Sign in with Apple's client secret.
//!
//! `none` and the HMAC algorithms are never accepted for ID tokens: a token
//! signed with a shared secret could be minted by anyone holding the client
//! secret.

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use ring::rand::SystemRandom;
use ring::signature::{self, EcdsaKeyPair, UnparsedPublicKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Why a JWS was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum JoseError {
    /// Not three base64url parts, or not JSON.
    #[error("malformed token: {0}")]
    Malformed(&'static str),
    /// An algorithm that is not accepted.
    #[error("unsupported algorithm `{0}`")]
    UnsupportedAlg(String),
    /// No key of the set matches the token.
    #[error("no matching key")]
    NoKey,
    /// The signature does not verify.
    #[error("bad signature")]
    BadSignature,
    /// A key could not be read.
    #[error("invalid key: {0}")]
    Key(&'static str),
}

/// The protected header fields Navree reads.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Header {
    pub(crate) alg: String,
    pub(crate) kid: Option<String>,
}

/// A decoded (not yet verified) compact JWS.
#[derive(Clone, Debug)]
pub(crate) struct Jws {
    pub(crate) header: Header,
    pub(crate) claims: Value,
    signing_input: String,
    signature: Vec<u8>,
}

/// One JSON Web Key (the public members Navree uses).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Jwk {
    pub(crate) kty: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) kid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) alg: Option<String>,
    #[serde(default, rename = "use", skip_serializing_if = "Option::is_none")]
    pub(crate) use_: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) n: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) e: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) crv: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) x: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) y: Option<String>,
}

/// A JSON Web Key Set (`{"keys": [...]}`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct JwkSet {
    #[serde(default)]
    pub(crate) keys: Vec<Jwk>,
}

impl JwkSet {
    /// Whether the set has a key with `kid`.
    pub(crate) fn has_kid(&self, kid: &str) -> bool {
        self.keys.iter().any(|key| key.kid.as_deref() == Some(kid))
    }
}

fn b64(part: &str) -> Result<Vec<u8>, JoseError> {
    URL_SAFE_NO_PAD
        .decode(part.trim_end_matches('='))
        .map_err(|_| JoseError::Malformed("bad base64url"))
}

/// Decodes a compact JWS without checking it.
pub(crate) fn decode(token: &str) -> Result<Jws, JoseError> {
    let mut parts = token.split('.');
    let (Some(header), Some(payload), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(JoseError::Malformed("expected three parts"));
    };
    let header: Header =
        serde_json::from_slice(&b64(header)?).map_err(|_| JoseError::Malformed("bad header"))?;
    let claims: Value =
        serde_json::from_slice(&b64(payload)?).map_err(|_| JoseError::Malformed("bad claims"))?;
    if !claims.is_object() {
        return Err(JoseError::Malformed("claims are not an object"));
    }
    let signing_input_len = token.len() - token.rsplit('.').next().map_or(0, str::len) - 1;
    Ok(Jws {
        header,
        claims,
        signing_input: token[..signing_input_len].to_owned(),
        signature: b64(signature)?,
    })
}

impl Jws {
    /// Verifies the signature with a key of `keys`: the key with the
    /// header's `kid`, or (without a `kid`) any key of the right type.
    pub(crate) fn verify(&self, keys: &JwkSet) -> Result<(), JoseError> {
        let alg = self.header.alg.as_str();
        let kty = key_type(alg)?;
        let candidates: Vec<&Jwk> = keys
            .keys
            .iter()
            .filter(|key| key.kty == kty)
            .filter(|key| key.use_.as_deref().is_none_or(|u| u == "sig"))
            .filter(|key| key.alg.as_deref().is_none_or(|a| a == alg))
            .filter(|key| match &self.header.kid {
                Some(kid) => key.kid.as_deref() == Some(kid.as_str()),
                None => true,
            })
            .collect();
        if candidates.is_empty() {
            return Err(JoseError::NoKey);
        }
        let mut last = JoseError::BadSignature;
        for key in candidates {
            match verify_with(alg, key, self.signing_input.as_bytes(), &self.signature) {
                Ok(()) => return Ok(()),
                Err(error) => last = error,
            }
        }
        Err(last)
    }
}

/// The `kty` of the keys that verify `alg`.
fn key_type(alg: &str) -> Result<&'static str, JoseError> {
    match alg {
        "RS256" | "RS384" | "RS512" | "PS256" | "PS384" | "PS512" => Ok("RSA"),
        "ES256" | "ES384" => Ok("EC"),
        "EdDSA" => Ok("OKP"),
        other => Err(JoseError::UnsupportedAlg(other.to_owned())),
    }
}

fn verify_with(alg: &str, key: &Jwk, message: &[u8], sig: &[u8]) -> Result<(), JoseError> {
    let member = |value: &Option<String>| -> Result<Vec<u8>, JoseError> {
        b64(value.as_deref().ok_or(JoseError::Key("missing member"))?)
            .map_err(|_| JoseError::Key("bad base64url member"))
    };
    let checked = match alg {
        "RS256" | "RS384" | "RS512" | "PS256" | "PS384" | "PS512" => {
            let params: &signature::RsaParameters = match alg {
                "RS256" => &signature::RSA_PKCS1_2048_8192_SHA256,
                "RS384" => &signature::RSA_PKCS1_2048_8192_SHA384,
                "RS512" => &signature::RSA_PKCS1_2048_8192_SHA512,
                "PS256" => &signature::RSA_PSS_2048_8192_SHA256,
                "PS384" => &signature::RSA_PSS_2048_8192_SHA384,
                _ => &signature::RSA_PSS_2048_8192_SHA512,
            };
            let (n, e) = (member(&key.n)?, member(&key.e)?);
            signature::RsaPublicKeyComponents { n: &n, e: &e }.verify(params, message, sig)
        }
        "ES256" | "ES384" => {
            let (curve, algorithm): (&str, &'static signature::EcdsaVerificationAlgorithm) =
                if alg == "ES256" {
                    ("P-256", &signature::ECDSA_P256_SHA256_FIXED)
                } else {
                    ("P-384", &signature::ECDSA_P384_SHA384_FIXED)
                };
            if key.crv.as_deref() != Some(curve) {
                return Err(JoseError::Key("wrong curve"));
            }
            let mut point = vec![0x04];
            point.extend(member(&key.x)?);
            point.extend(member(&key.y)?);
            UnparsedPublicKey::new(algorithm, point).verify(message, sig)
        }
        "EdDSA" => {
            if key.crv.as_deref() != Some("Ed25519") {
                return Err(JoseError::Key("wrong curve"));
            }
            UnparsedPublicKey::new(&signature::ED25519, member(&key.x)?).verify(message, sig)
        }
        other => return Err(JoseError::UnsupportedAlg(other.to_owned())),
    };
    checked.map_err(|_| JoseError::BadSignature)
}

/// A P-256 private key (PKCS#8, such as Apple's `.p8` files) that signs
/// ES256 JWTs.
pub(crate) struct EcSigningKey {
    pair: EcdsaKeyPair,
}

impl EcSigningKey {
    /// Reads a PEM `PRIVATE KEY` (PKCS#8). Literal `\n` sequences (a key
    /// pasted into one environment variable) and missing armor lines are
    /// accepted.
    pub(crate) fn from_pem(pem: &str) -> Result<Self, JoseError> {
        let body: String = pem
            .replace("\\n", "\n")
            .lines()
            .filter(|line| !line.trim_start().starts_with("-----"))
            .flat_map(|line| line.chars().filter(|c| !c.is_whitespace()))
            .collect();
        let der = STANDARD
            .decode(body)
            .map_err(|_| JoseError::Key("the key is not PEM"))?;
        Self::from_pkcs8(&der)
    }

    /// Reads a PKCS#8 DER document.
    pub(crate) fn from_pkcs8(der: &[u8]) -> Result<Self, JoseError> {
        let pair = EcdsaKeyPair::from_pkcs8(
            &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
            der,
            &SystemRandom::new(),
        )
        .map_err(|_| JoseError::Key("not a PKCS#8 P-256 key"))?;
        Ok(Self { pair })
    }

    /// A compact JWS of `claims` with header `{"alg":"ES256","kid":kid}`.
    pub(crate) fn sign(&self, kid: &str, claims: &Value) -> Result<String, JoseError> {
        let header = serde_json::json!({ "alg": "ES256", "kid": kid });
        let signing_input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        );
        let signature = self
            .pair
            .sign(&SystemRandom::new(), signing_input.as_bytes())
            .map_err(|_| JoseError::Key("signing failed"))?;
        Ok(format!(
            "{signing_input}.{}",
            URL_SAFE_NO_PAD.encode(signature.as_ref())
        ))
    }

    /// The public key as a JWK.
    #[cfg(test)]
    pub(crate) fn public_jwk(&self, kid: &str) -> Jwk {
        use ring::signature::KeyPair as _;
        let point = self.pair.public_key().as_ref();
        Jwk {
            kty: "EC".to_owned(),
            kid: Some(kid.to_owned()),
            alg: Some("ES256".to_owned()),
            use_: Some("sig".to_owned()),
            crv: Some("P-256".to_owned()),
            x: Some(URL_SAFE_NO_PAD.encode(&point[1..33])),
            y: Some(URL_SAFE_NO_PAD.encode(&point[33..65])),
            ..Jwk::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair, RSA_PKCS1_SHA256, RsaKeyPair};
    use serde_json::json;

    use super::*;

    const APPLE_P8: &str = include_str!("../tests/fixtures/apple-authkey.p8");
    const RSA_PEM: &str = include_str!("../tests/fixtures/rsa-2048.pem");

    fn pem_der(pem: &str) -> Vec<u8> {
        let body: String = pem
            .lines()
            .filter(|line| !line.starts_with("-----"))
            .collect();
        STANDARD.decode(body).unwrap()
    }

    fn rs256(claims: &Value, kid: &str) -> (String, Jwk) {
        let pair = RsaKeyPair::from_pkcs8(&pem_der(RSA_PEM)).unwrap();
        let header = json!({ "alg": "RS256", "kid": kid });
        let input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        );
        let mut sig = vec![0; pair.public().modulus_len()];
        pair.sign(
            &RSA_PKCS1_SHA256,
            &SystemRandom::new(),
            input.as_bytes(),
            &mut sig,
        )
        .unwrap();
        let public: signature::RsaPublicKeyComponents<Vec<u8>> = pair.public().into();
        let jwk = Jwk {
            kty: "RSA".to_owned(),
            kid: Some(kid.to_owned()),
            n: Some(URL_SAFE_NO_PAD.encode(&public.n)),
            e: Some(URL_SAFE_NO_PAD.encode(&public.e)),
            ..Jwk::default()
        };
        (format!("{input}.{}", URL_SAFE_NO_PAD.encode(sig)), jwk)
    }

    #[test]
    fn rs256_tokens_verify_against_their_key_only() {
        let claims = json!({ "sub": "42", "iss": "https://idp.test" });
        let (token, jwk) = rs256(&claims, "k1");
        let jws = decode(&token).unwrap();
        assert_eq!(jws.claims, claims);
        assert_eq!(jws.header.kid.as_deref(), Some("k1"));
        let keys = JwkSet {
            keys: vec![jwk.clone()],
        };
        jws.verify(&keys).unwrap();

        // Another kid: no key.
        let other = JwkSet {
            keys: vec![Jwk {
                kid: Some("k2".to_owned()),
                ..jwk.clone()
            }],
        };
        assert_eq!(jws.verify(&other), Err(JoseError::NoKey));

        // A changed payload: bad signature.
        let mut parts: Vec<&str> = token.split('.').collect();
        let forged = URL_SAFE_NO_PAD.encode(json!({ "sub": "1" }).to_string());
        parts[1] = &forged;
        let tampered = decode(&parts.join(".")).unwrap();
        assert_eq!(tampered.verify(&keys), Err(JoseError::BadSignature));
    }

    #[test]
    fn es256_signatures_round_trip_with_an_apple_p8_key() {
        let key = EcSigningKey::from_pem(APPLE_P8).unwrap();
        let claims = json!({ "iss": "TEAMID", "sub": "com.acme.web" });
        let token = key.sign("KEYID", &claims).unwrap();
        let jws = decode(&token).unwrap();
        assert_eq!(jws.header.alg, "ES256");
        assert_eq!(jws.header.kid.as_deref(), Some("KEYID"));
        jws.verify(&JwkSet {
            keys: vec![key.public_jwk("KEYID")],
        })
        .unwrap();
        // The same key in one line with `\n` escapes (an environment variable).
        let escaped = APPLE_P8.trim().replace('\n', "\\n");
        assert!(EcSigningKey::from_pem(&escaped).is_ok());
        assert!(EcSigningKey::from_pem("not a key").is_err());
        assert!(
            EcSigningKey::from_pem(RSA_PEM).is_err(),
            "an RSA key is refused"
        );
    }

    #[test]
    fn eddsa_signatures_verify() {
        let rng = SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(json!({ "alg": "EdDSA" }).to_string()),
            URL_SAFE_NO_PAD.encode(json!({ "sub": "1" }).to_string())
        );
        let token = format!(
            "{input}.{}",
            URL_SAFE_NO_PAD.encode(pair.sign(input.as_bytes()).as_ref())
        );
        let jwk = Jwk {
            kty: "OKP".to_owned(),
            crv: Some("Ed25519".to_owned()),
            x: Some(URL_SAFE_NO_PAD.encode(pair.public_key().as_ref())),
            ..Jwk::default()
        };
        decode(&token)
            .unwrap()
            .verify(&JwkSet { keys: vec![jwk] })
            .unwrap();
    }

    #[test]
    fn none_and_hmac_are_refused() {
        for alg in ["none", "HS256"] {
            let token = format!(
                "{}.{}.",
                URL_SAFE_NO_PAD.encode(json!({ "alg": alg }).to_string()),
                URL_SAFE_NO_PAD.encode(json!({ "sub": "1" }).to_string())
            );
            let jws = decode(&token).unwrap();
            assert_eq!(
                jws.verify(&JwkSet::default()),
                Err(JoseError::UnsupportedAlg(alg.to_owned()))
            );
        }
    }

    #[test]
    fn malformed_tokens_are_refused() {
        assert!(decode("a.b").is_err());
        assert!(decode("a.b.c.d").is_err());
        assert!(decode("!!.e30.").is_err());
        let not_object = format!(
            "{}.{}.",
            URL_SAFE_NO_PAD.encode(json!({ "alg": "RS256" }).to_string()),
            URL_SAFE_NO_PAD.encode("[1]")
        );
        assert!(decode(&not_object).is_err());
    }
}
